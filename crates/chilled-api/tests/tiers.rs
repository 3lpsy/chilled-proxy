//! Auth-tier enforcement: readonly bypass under public-readonly, 401 JSON
//! (never a redirect) otherwise, mutating always gated.

mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use chilled_api::AuthMode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn readonly_requires_auth_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _state) = router(config(AuthMode::Builtin, &dir)).await;

    for path in [
        "/api/registries",
        "/api/registries/npm",
        "/api/config",
        "/api/snapshots/latest",
    ] {
        let res = send(&app, get(path)).await;
        assert_status(&res, StatusCode::UNAUTHORIZED);
        assert!(res.headers().get(header::LOCATION).is_none(), "no redirect");
    }
}

#[tokio::test]
async fn public_readonly_opens_state_but_not_mutations() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(AuthMode::Builtin, &dir);
    cfg.public_readonly = true;
    let (app, _state) = router(cfg).await;

    let res = send(&app, get("/api/registries")).await;
    assert_status(&res, StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body[0]["name"], "npm");
    // Value-free auth summary: basic flag + header names only.
    assert_eq!(body[0]["auth"]["basic"], true);
    assert_eq!(body[0]["auth"]["header_names"][0], "x-corp-token");

    let res = send(&app, get("/api/config")).await;
    assert_status(&res, StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["ui"]["public_readonly"], true);

    // Mutations stay gated even in public-readonly mode.
    let res = send(
        &app,
        Request::post("/api/snapshots/refresh")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_status(&res, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_user_reaches_both_tiers() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(AuthMode::Builtin, &dir);
    cfg.admin_username = Some("admin".into());
    cfg.admin_password = Some("swordfish-1".into());
    let (app, _state) = router(cfg).await;

    let res = send(
        &app,
        post_json(
            "/api/session",
            &json!({"username": "admin", "password": "swordfish-1"}),
        ),
    )
    .await;
    let cookie = session_cookie(&res);

    let res = send(&app, get_with_cookie("/api/registries", &cookie)).await;
    assert_status(&res, StatusCode::OK);

    let res = send(
        &app,
        Request::post("/api/snapshots/refresh")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_status(&res, StatusCode::ACCEPTED);
}

/// Per-mount refresh is write-protected: 401 anonymous (even with public
/// readonly on), 404 unknown mount, 202 when authenticated.
#[tokio::test]
async fn per_mount_refresh_is_write_protected() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config_with_admin(&dir);
    cfg.public_readonly = true;
    let (app, _state) = router(cfg).await;

    let refresh = |cookie: Option<&str>, name: &str| {
        let mut req = Request::post(format!("/api/registries/{name}/refresh"));
        if let Some(cookie) = cookie {
            req = req.header(header::COOKIE, cookie);
        }
        req.body(Body::empty()).unwrap()
    };

    // Public-readonly does not open mutations.
    let res = send(&app, refresh(None, "npm")).await;
    assert_status(&res, StatusCode::UNAUTHORIZED);

    let cookie = login(&app).await;
    let res = send(&app, refresh(Some(&cookie), "nope")).await;
    assert_status(&res, StatusCode::NOT_FOUND);
    let res = send(&app, refresh(Some(&cookie), "npm")).await;
    assert_status(&res, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn unknown_mount_is_404_when_authorized() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(AuthMode::Builtin, &dir);
    cfg.public_readonly = true;
    let (app, _state) = router(cfg).await;
    let res = send(&app, get("/api/registries/nope")).await;
    assert_status(&res, StatusCode::NOT_FOUND);
}
