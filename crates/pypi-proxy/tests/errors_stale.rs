//! Failure modes: stale-serve from the pristine disk cache, upstream errors
//! forwarded, and the fail-closed handling of HTML-only upstreams.

mod common;

use std::time::SystemTime;

use common::{simple_json, TestProxy, OLD, SHA, SIMPLE_CTYPE, TOO_NEW};

const JSON_ACCEPT: &[(&str, &str)] = &[("accept", SIMPLE_CTYPE)];

#[tokio::test]
async fn dead_upstream_serves_stale_cache_filtered_and_rewritten() {
    let proxy = TestProxy::builder()
        .cooldown_days(1)
        .dead_upstream()
        .start()
        .await;
    let body = simple_json(
        "foo",
        &[
            ("foo-1.0.0.tar.gz", OLD, SHA),
            ("foo-2.0.0.tar.gz", TOO_NEW, SHA),
        ],
    );
    proxy.seed_simple_file("foo", &body, SystemTime::now());

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    // Stale, but still age-gated and rewritten.
    let files = doc["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0]["url"],
        "http://localhost:3080/pypi/files/foo/packages/aa/bb/cc/foo-1.0.0.tar.gz"
    );
    assert_eq!(doc["versions"], serde_json::json!(["1.0.0"]));
}

#[tokio::test]
async fn dead_upstream_without_cache_is_502() {
    let proxy = TestProxy::builder().dead_upstream().start().await;

    assert_eq!(proxy.get_simple("foo", JSON_ACCEPT).await.status(), 502);
}

#[tokio::test]
async fn upstream_500_is_forwarded() {
    let proxy = TestProxy::builder().start().await;
    proxy.mock_simple_status("foo", 500).await;

    assert_eq!(proxy.get_simple("foo", JSON_ACCEPT).await.status(), 500);
}

#[tokio::test]
async fn upstream_404_is_forwarded() {
    let proxy = TestProxy::builder().start().await;
    proxy.mock_simple_status("foo", 404).await;

    assert_eq!(proxy.get_simple("foo", JSON_ACCEPT).await.status(), 404);
}

#[tokio::test]
async fn html_only_upstream_fails_closed_under_cooldown() {
    let proxy = TestProxy::builder().cooldown_days(1).start().await;
    proxy
        .mock_simple_ctype("foo", "<html>mirror</html>", "\"e1\"", "text/html")
        .await;

    // Cannot gate what it cannot parse -> refuse rather than serve ungated.
    assert_eq!(proxy.get_simple("foo", JSON_ACCEPT).await.status(), 502);
}

#[tokio::test]
async fn html_only_upstream_passes_through_without_cooldown() {
    let proxy = TestProxy::builder().start().await;
    proxy
        .mock_simple_ctype("foo", "<html>mirror</html>", "\"e1\"", "text/html")
        .await;

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    assert!(resp.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    assert_eq!(resp.text().await.unwrap(), "<html>mirror</html>");
}
