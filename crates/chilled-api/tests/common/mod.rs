//! Shared harness: an in-memory-ish UiState and request helpers.
//!
//! Each integration-test target compiles this separately and uses a subset,
//! so unused-helper warnings are expected noise — silenced here.
#![allow(dead_code)]

use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use axum::Router;
use chilled_api::{AuthMode, MountView, ServerView, UiConfig, UiState};
use chilled_wire::AuthSummary;
use http_body_util::BodyExt;
use tower::ServiceExt;

pub fn config(auth_mode: AuthMode, dir: &tempfile::TempDir) -> UiConfig {
    UiConfig {
        auth_mode,
        oidc_user_header: (auth_mode == AuthMode::Oidc).then(|| "x-auth-request-email".into()),
        oidc_login_url: None,
        public_readonly: false,
        cache_update_interval: Duration::from_secs(600),
        trust_first_user_signup: false,
        admin_username: None,
        admin_password: None,
        db_path: dir.path().join("test.db"),
        session_ttl: Duration::from_secs(3600),
        dev_dist_dir: None,
    }
}

pub fn sample_mounts() -> Vec<MountView> {
    vec![MountView {
        name: "npm".into(),
        kind: "npm".into(),
        path: "/npm".into(),
        upstream: "https://registry.npmjs.org/".into(),
        secondary: None,
        proxy_url: "http://localhost:3080/npm".into(),
        cooldown_secs: 0,
        cache_ttl_secs: 3600,
        restrict_downloads: false,
        auth: AuthSummary {
            basic: true,
            header_names: vec!["x-corp-token".into()],
        },
    }]
}

pub fn sample_server() -> ServerView {
    ServerView {
        listen: "127.0.0.1:3080".into(),
        log_level: "info".into(),
        metrics_enabled: false,
        disabled: vec![],
    }
}

pub async fn router(cfg: UiConfig) -> (Router, UiState) {
    router_with_scanners(cfg, vec![]).await
}

pub async fn router_with_scanners(
    cfg: UiConfig,
    scanners: Vec<(String, chilled_api::Scanner)>,
) -> (Router, UiState) {
    let ops = scanners
        .into_iter()
        .map(|(name, scan)| (name, chilled_api::MountOps::scan_only(scan)))
        .collect();
    router_with_ops(cfg, ops).await
}

pub async fn router_with_ops(
    cfg: UiConfig,
    ops: Vec<(String, chilled_api::MountOps)>,
) -> (Router, UiState) {
    let state = chilled_api::startup(
        cfg,
        "test".into(),
        sample_server(),
        sample_mounts(),
        ops,
        None,
    )
    .await
    .expect("ui startup");
    (chilled_api::ui_router(state.clone()), state)
}

/// Logs in as a freshly bootstrapped user and returns the session cookie.
pub async fn login(app: &Router) -> String {
    let res = send(
        app,
        post_json(
            "/api/session",
            &serde_json::json!({"username": "admin", "password": "swordfish-1"}),
        ),
    )
    .await;
    assert_status(&res, StatusCode::NO_CONTENT);
    session_cookie(&res)
}

/// A config with the bootstrap admin knobs set.
pub fn config_with_admin(dir: &tempfile::TempDir) -> UiConfig {
    let mut cfg = config(AuthMode::Builtin, dir);
    cfg.admin_username = Some("admin".into());
    cfg.admin_password = Some("swordfish-1".into());
    cfg
}

pub async fn send(app: &Router, req: Request<Body>) -> Response<Body> {
    app.clone().oneshot(req).await.expect("request runs")
}

pub async fn body_json(res: Response<Body>) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("json body")
}

pub fn get(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).unwrap()
}

pub fn get_with_cookie(path: &str, cookie: &str) -> Request<Body> {
    Request::get(path)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}

pub fn post_json(path: &str, body: &serde_json::Value) -> Request<Body> {
    Request::post(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// The `name=value` pair from a Set-Cookie response header.
pub fn session_cookie(res: &Response<Body>) -> String {
    let raw = res
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie present")
        .to_str()
        .unwrap();
    raw.split(';').next().unwrap().to_string()
}

pub fn assert_status(res: &Response<Body>, expected: StatusCode) {
    assert_eq!(res.status(), expected, "unexpected status");
}
