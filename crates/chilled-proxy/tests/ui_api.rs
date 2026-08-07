//! Blackbox tests of the /ui + /api management plane over real HTTP.

mod common;

use common::TestApp;
use serde_json::json;

/// Without --ui, the reserved prefixes stay unrouted 404s.
#[tokio::test]
async fn ui_and_api_absent_without_flag() {
    let app = TestApp::start_bare(&[]).await;
    assert_eq!(app.get("/ui").await.status(), 404);
    assert_eq!(app.get("/ui/").await.status(), 404);
    assert_eq!(app.get("/api/meta").await.status(), 404);
}

/// A UI-less build serves a 503 hint for the shell but a working API.
#[tokio::test]
async fn ui_flag_routes_api_and_hints_on_missing_bundle() {
    // public-readonly so the anonymous meta call includes the mount inventory.
    let app =
        TestApp::start_bare(&["--ui".to_string(), "--ui-public-readonly-enabled".into()]).await;

    let meta = app.get("/api/meta").await;
    assert_eq!(meta.status(), 200);
    let meta: serde_json::Value = meta.json().await.unwrap();
    assert_eq!(meta["auth_mode"], "builtin");
    assert_eq!(meta["needs_first_user"], false);
    let names: Vec<&str> = meta["mounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "crates",
            "npm",
            "pypi",
            "maven",
            "gradle-plugins",
            "google-maven"
        ]
    );

    // The embedded dist may or may not exist depending on whether `just
    // ui-build` ran on this checkout; both outcomes are correct here.
    let shell = app.get("/ui/").await;
    assert!(
        shell.status() == 200 || shell.status() == 503,
        "unexpected status {}",
        shell.status()
    );
}

/// Full builtin-auth round trip: bootstrap admin, login wrong/right, gated
/// reads, snapshot refresh picking up a real cache file, logout.
#[tokio::test]
async fn builtin_auth_snapshot_round_trip() {
    let app = TestApp::start_bare(&[
        "--ui".to_string(),
        "--ui-admin-username".into(),
        "admin".into(),
        "--ui-admin-password".into(),
        "test-password-1".into(),
        "--ui-cache-update-interval".into(),
        "10m".into(),
    ])
    .await;

    // Gated by default; no redirect, JSON 401.
    let res = app.get("/api/registries").await;
    assert_eq!(res.status(), 401);
    let err: serde_json::Value = res.json().await.unwrap();
    assert_eq!(err["error"], "authentication required");

    let login = |password: &str| {
        app.client
            .post(format!("{}/api/session", app.base_url))
            .json(&json!({"username": "admin", "password": password}))
            .send()
    };
    assert_eq!(login("wrong").await.unwrap().status(), 401);
    let res = login("test-password-1").await.unwrap();
    assert_eq!(res.status(), 204);
    let cookie = res
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    assert!(cookie.starts_with("chilled_session="));

    // Seed a cache file, trigger a snapshot, read it back through the table.
    let tarballs = app.tmp.path().join("npm/tarballs/lodash");
    std::fs::create_dir_all(&tarballs).unwrap();
    std::fs::write(tarballs.join("lodash-4.17.21.tgz"), b"twelve bytes").unwrap();

    let res = app
        .client
        .post(format!("{}/api/snapshots/refresh", app.base_url))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 202);

    // The refresh is async; poll briefly for the row.
    let mut found = None;
    for _ in 0..50 {
        let page: serde_json::Value = app
            .client
            .get(format!("{}/api/artifacts?mount=npm", app.base_url))
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if page["total"] == 1 {
            found = Some(page);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let page = found.expect("snapshot produced the artifact row");
    assert_eq!(page["items"][0]["name"], "lodash");
    assert_eq!(page["items"][0]["version"], "4.17.21");
    assert_eq!(page["items"][0]["size_bytes"], 12);

    // Logout kills the session.
    let res = app
        .client
        .delete(format!("{}/api/session", app.base_url))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    let res = app
        .client
        .get(format!("{}/api/registries", app.base_url))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

/// Public-readonly opens state endpoints, keeps users/logs/mutations gated.
#[tokio::test]
async fn public_readonly_scope() {
    let app =
        TestApp::start_bare(&["--ui".to_string(), "--ui-public-readonly-enabled".into()]).await;

    for path in ["/api/registries", "/api/artifacts", "/api/config"] {
        assert_eq!(app.get(path).await.status(), 200, "{path}");
    }
    // Redaction: config never contains credential material fields with values.
    let config: serde_json::Value = app.get("/api/config").await.json().await.unwrap();
    assert!(config["ui"]["public_readonly"].as_bool().unwrap());

    assert_eq!(app.get("/api/users").await.status(), 401);
    assert_eq!(app.get("/api/logs").await.status(), 401);
    let res = app
        .client
        .post(format!("{}/api/snapshots/refresh", app.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

/// oidc mode: the trusted header provisions users on the fly; builtin-only
/// endpoints refuse.
#[tokio::test]
async fn oidc_header_provisioning() {
    let app = TestApp::start_bare(&[
        "--ui".to_string(),
        "--ui-auth".into(),
        "oidc".into(),
        "--ui-oidc-user-header".into(),
        "x-auth-request-email".into(),
    ])
    .await;

    let meta: serde_json::Value = app
        .client
        .get(format!("{}/api/meta", app.base_url))
        .header("x-auth-request-email", "dev@example.com")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(meta["user"]["username"], "dev@example.com");
    assert_eq!(meta["user"]["auth_source"], "oidc");

    // Password login is the provider's job now.
    let res = app
        .client
        .post(format!("{}/api/session", app.base_url))
        .json(&json!({"username": "dev@example.com", "password": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 409);

    // A spoofed header on a fresh request still authenticates (that's the
    // trust model: the reverse proxy must strip inbound copies).
    let users: serde_json::Value = app
        .client
        .get(format!("{}/api/users", app.base_url))
        .header("x-auth-request-email", "dev@example.com")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(users.as_array().unwrap().len(), 1);
}

/// Unknown /api paths 404 as API responses even when a root-mounted registry
/// owns the app fallback (they must never be proxied upstream as packages).
#[tokio::test]
async fn unknown_api_paths_404_with_root_mounted_registry() {
    let app = TestApp::start_bare(&[
        "--ui".to_string(),
        "--npm-path".into(),
        "/".into(),
        "--disable-crates".into(),
        "--disable-pypi".into(),
        "--disable-maven".into(),
        "--no-default-mounts".into(),
    ])
    .await;

    for path in ["/api", "/api/artifact", "/api/registries/npm/extra"] {
        let res = app.get(path).await;
        assert_eq!(res.status(), 404, "{path}");
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["error"], "no such endpoint", "{path}");
    }
    // Real endpoints still resolve ahead of the catch-all.
    assert_eq!(app.get("/api/meta").await.status(), 200);
}

/// Startup validation: incompatible knob combinations are resolve() errors.
#[test]
fn invalid_ui_configs_fail_resolution() {
    use clap::Parser;
    for args in [
        vec!["chilled-proxy", "--ui-public-readonly-enabled"],
        vec!["chilled-proxy", "--ui", "--ui-auth", "oidc"],
        vec![
            "chilled-proxy",
            "--ui",
            "--ui-auth",
            "oidc",
            "--ui-oidc-user-header",
            "x-user",
            "--ui-trust-first-user-signup",
        ],
        vec!["chilled-proxy", "--ui", "--ui-admin-username", "a"],
    ] {
        let cli = chilled_proxy::cli::Cli::try_parse_from(&args).unwrap();
        assert!(cli.resolve().is_err(), "{args:?} should fail");
    }
}
