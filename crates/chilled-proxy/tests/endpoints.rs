//! Top-level surface of the unified binary: `/`, `/healthz`, `/metrics`
//! (gated), registry mounting, and --disable-* flags.

mod common;

use common::TestApp;
use serde_json::Value;
use wiremock::matchers::{method, path as match_path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn home_reports_running_and_mounted_registries() {
    let app = TestApp::start(&[]).await;

    let resp = app.get("/").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()["content-type"],
        "application/json; charset=utf-8"
    );
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["status"], "running");
    let ids: Vec<_> = json["registries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    // Maven brings its built-in Gradle mounts along.
    assert_eq!(
        ids,
        [
            "crates",
            "npm",
            "pypi",
            "maven",
            "gradle-plugins",
            "google-maven"
        ]
    );
}

#[tokio::test]
async fn healthz_is_ok() {
    let app = TestApp::start(&[]).await;

    let resp = app.get("/healthz").await;
    assert_eq!(resp.status(), 200);
    assert!(resp.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    assert_eq!(resp.text().await.unwrap(), "ok\n");
}

#[tokio::test]
async fn metrics_404_when_disabled() {
    let app = TestApp::start(&[]).await;
    assert_eq!(app.get("/metrics").await.status(), 404);
}

#[tokio::test]
async fn metrics_empty_when_enabled_with_no_cache() {
    let app = TestApp::start(&["--enable-metrics"]).await;

    let resp = app.get("/metrics").await;
    assert_eq!(resp.status(), 200);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["service"], "chilled-proxy");
    for id in ["crates", "npm", "pypi", "maven"] {
        assert_eq!(json["registries"][id]["cached_count"], 0, "registry {id}");
    }
}

#[tokio::test]
async fn metrics_lists_cached_crate_after_download() {
    let app = TestApp::start(&["--enable-metrics"]).await;

    Mock::given(method("GET"))
        .and(match_path("/api/v1/crates/serde/1.0.0/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"crate-bytes".to_vec()))
        .mount(app.mock_upstream.as_ref().unwrap())
        .await;

    // Populate the crates cache through the mounted registry.
    let resp = app.get("/crates/api/v1/crates/serde/1.0.0/download").await;
    assert_eq!(resp.status(), 200);

    let json: Value = app.get("/metrics").await.json().await.unwrap();
    assert_eq!(json["registries"]["crates"]["cached_count"], 1);
    let entry = &json["registries"]["crates"]["artifacts"][0];
    assert_eq!(entry["name"], "serde");
    assert_eq!(entry["version"], "1.0.0");
    assert!(entry["cached_at"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn crates_registry_is_mounted_under_prefix() {
    let app = TestApp::start(&[]).await;

    let resp = app.get("/crates/index/config.json").await;
    assert_eq!(resp.status(), 200);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["dl"], "http://127.0.0.1:3080/crates/api/v1/crates");

    // The old un-prefixed layout is gone.
    assert_eq!(app.get("/index/config.json").await.status(), 404);
}

#[tokio::test]
async fn disabled_registry_is_not_mounted_or_listed() {
    let app = TestApp::start(&["--disable-npm"]).await;

    let json: Value = app.get("/").await.json().await.unwrap();
    let ids: Vec<_> = json["registries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        ids,
        ["crates", "pypi", "maven", "gradle-plugins", "google-maven"]
    );

    // With metrics enabled, a disabled registry is absent from the report too.
    let app = TestApp::start(&["--disable-npm", "--enable-metrics"]).await;
    let json: Value = app.get("/metrics").await.json().await.unwrap();
    assert!(json["registries"].get("npm").is_none());
}

#[tokio::test]
async fn unknown_top_level_route_is_404() {
    let app = TestApp::start(&[]).await;
    assert_eq!(app.get("/nope").await.status(), 404);
}

#[tokio::test]
async fn registries_serve_under_custom_mounts() {
    let app = TestApp::start(&["--crates-path", "/rust", "--npm-path", "/registry/npm"]).await;

    // The crates registry answers on its new mount...
    let resp = app.get("/rust/index/config.json").await;
    assert_eq!(resp.status(), 200);
    let json: Value = resp.json().await.unwrap();
    // ...and the generated download URL follows the mount, not the registry id.
    assert_eq!(json["dl"], "http://127.0.0.1:3080/rust/api/v1/crates");

    // The default path no longer routes.
    assert_eq!(app.get("/crates/index/config.json").await.status(), 404);
    // An untouched registry keeps its default mount.
    assert_eq!(app.get("/pypi/simple/").await.status(), 200);
    // A multi-segment mount works.
    // Routed to npm (no upstream mock, so it 404s) rather than falling through.
    assert_eq!(app.get("/registry/npm/lodash").await.status(), 404);
    assert!(!app
        .mock_upstream
        .as_ref()
        .unwrap()
        .received_requests()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_lone_registry_can_own_the_root() {
    let app = TestApp::start(&[
        "--pypi-path",
        "/",
        "--disable-crates",
        "--disable-npm",
        "--disable-maven",
    ])
    .await;

    // PyPI's own routes are served straight off the root.
    let resp = app.get("/simple/").await;
    assert_eq!(resp.status(), 200);

    // The server surface still works alongside it.
    assert_eq!(app.get("/healthz").await.status(), 200);
    let home: Value = app.get("/").await.json().await.unwrap();
    assert_eq!(home["status"], "running");
    assert_eq!(home["registries"][0], "pypi");

    // Unknown paths fall through to the registry, which 404s them itself.
    assert_eq!(app.get("/not-a-project").await.status(), 404);
}

#[tokio::test]
async fn root_mount_rewrites_urls_without_a_prefix() {
    let app = TestApp::start(&[
        "--crates-path",
        "/",
        "--disable-npm",
        "--disable-pypi",
        "--disable-maven",
    ])
    .await;

    let json: Value = app.get("/index/config.json").await.json().await.unwrap();
    assert_eq!(json["dl"], "http://127.0.0.1:3080/api/v1/crates");
}
