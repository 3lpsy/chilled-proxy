//! Auth-tier integration tests: login/logout, first-user setup, oidc
//! provisioning — against a real sqlite file and the full ui_router.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chilled_api::AuthMode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn meta_is_public_but_hides_inventory_from_anonymous() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _state) = router(config(AuthMode::Builtin, &dir)).await;

    // Anonymous on a private deployment: enough to render the login page,
    // but no version and no mount inventory.
    let res = send(&app, get("/api/meta")).await;
    assert_status(&res, StatusCode::OK);
    let meta = body_json(res).await;
    assert_eq!(meta["auth_mode"], "builtin");
    assert_eq!(meta["user"], serde_json::Value::Null);
    assert_eq!(meta["mounts"].as_array().unwrap().len(), 0);
    assert_eq!(meta["version"], "");
}

#[tokio::test]
async fn meta_lists_mounts_when_public_readonly() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(AuthMode::Builtin, &dir);
    cfg.public_readonly = true;
    let (app, _state) = router(cfg).await;
    let meta = body_json(send(&app, get("/api/meta")).await).await;
    assert_eq!(meta["mounts"][0]["name"], "npm");
    assert_eq!(meta["version"], "test");
}

#[tokio::test]
async fn login_logout_cookie_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(AuthMode::Builtin, &dir);
    cfg.admin_username = Some("admin".into());
    cfg.admin_password = Some("swordfish-1".into());
    let (app, _state) = router(cfg).await;

    // Wrong password → 401, no cookie.
    let res = send(
        &app,
        post_json(
            "/api/session",
            &json!({"username": "admin", "password": "wrong"}),
        ),
    )
    .await;
    assert_status(&res, StatusCode::UNAUTHORIZED);

    // Right password → 204 + session cookie.
    let res = send(
        &app,
        post_json(
            "/api/session",
            &json!({"username": "admin", "password": "swordfish-1"}),
        ),
    )
    .await;
    assert_status(&res, StatusCode::NO_CONTENT);
    let cookie = session_cookie(&res);

    // The cookie authenticates /api/meta.
    let res = send(&app, get_with_cookie("/api/meta", &cookie)).await;
    let meta = body_json(res).await;
    assert_eq!(meta["user"]["username"], "admin");

    // Logout deletes the session; the old cookie no longer authenticates.
    let res = send(
        &app,
        Request::delete("/api/session")
            .header(axum::http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_status(&res, StatusCode::NO_CONTENT);
    let res = send(&app, get_with_cookie("/api/meta", &cookie)).await;
    let meta = body_json(res).await;
    assert_eq!(meta["user"], serde_json::Value::Null);
}

#[tokio::test]
async fn first_user_signup_gate() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(AuthMode::Builtin, &dir);
    cfg.trust_first_user_signup = true;
    let (app, _state) = router(cfg).await;

    let meta = body_json(send(&app, get("/api/meta")).await).await;
    assert_eq!(meta["needs_first_user"], true);

    // Weak password rejected.
    let res = send(
        &app,
        post_json(
            "/api/setup/first-user",
            &json!({"username": "me", "password": "short"}),
        ),
    )
    .await;
    assert_status(&res, StatusCode::UNPROCESSABLE_ENTITY);

    // Creation succeeds once and starts a session.
    let res = send(
        &app,
        post_json(
            "/api/setup/first-user",
            &json!({"username": "me", "password": "long-enough"}),
        ),
    )
    .await;
    assert_status(&res, StatusCode::CREATED);
    let cookie = session_cookie(&res);
    let meta = body_json(send(&app, get_with_cookie("/api/meta", &cookie)).await).await;
    assert_eq!(meta["user"]["username"], "me");
    assert_eq!(meta["needs_first_user"], false);

    // A second attempt conflicts.
    let res = send(
        &app,
        post_json(
            "/api/setup/first-user",
            &json!({"username": "sneaky", "password": "long-enough"}),
        ),
    )
    .await;
    assert_status(&res, StatusCode::CONFLICT);
}

#[tokio::test]
async fn first_user_signup_disabled_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _state) = router(config(AuthMode::Builtin, &dir)).await;
    let res = send(
        &app,
        post_json(
            "/api/setup/first-user",
            &json!({"username": "me", "password": "long-enough"}),
        ),
    )
    .await;
    assert_status(&res, StatusCode::CONFLICT);
}

#[tokio::test]
async fn oidc_header_provisions_users_on_the_fly() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _state) = router(config(AuthMode::Oidc, &dir)).await;

    // No header → anonymous.
    let meta = body_json(send(&app, get("/api/meta")).await).await;
    assert_eq!(meta["user"], serde_json::Value::Null);

    // Trusted header → user created on first sight.
    let req = Request::get("/api/meta")
        .header("x-auth-request-email", "dev@example.com")
        .body(Body::empty())
        .unwrap();
    let meta = body_json(send(&app, req).await).await;
    assert_eq!(meta["user"]["username"], "dev@example.com");
    assert_eq!(meta["user"]["auth_source"], "oidc");

    // Login endpoint refuses in oidc mode.
    let res = send(
        &app,
        post_json(
            "/api/session",
            &json!({"username": "dev@example.com", "password": "x"}),
        ),
    )
    .await;
    assert_status(&res, StatusCode::CONFLICT);
}
