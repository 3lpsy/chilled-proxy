//! Generated `config.json` (the `/`, `/healthz`, and `/metrics` surface lives
//! in the `chilled-proxy` bin crate and is tested there).

mod common;

use common::TestProxy;
use serde_json::Value;

#[tokio::test]
async fn config_json_points_downloads_at_proxy() {
    let proxy = TestProxy::builder()
        .proxy_url("http://proxy.test/crates/")
        .start()
        .await;

    let resp = proxy.get_config_json().await;
    assert_eq!(resp.status(), 200);
    let json: Value = resp.json().await.unwrap();

    // `dl` is rewritten to this proxy's mount; `api` is the upstream, trimmed.
    assert_eq!(json["dl"], "http://proxy.test/crates/api/v1/crates");
    assert_eq!(
        json["api"],
        proxy.mock_upstream().uri().trim_end_matches('/')
    );
}

#[tokio::test]
async fn config_json_default_proxy_url_includes_mount() {
    let proxy = TestProxy::builder().start().await;

    let json: Value = proxy.get_config_json().await.json().await.unwrap();
    assert_eq!(json["dl"], "http://localhost:3080/crates/api/v1/crates");
}
