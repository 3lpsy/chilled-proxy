//! Failure modes: stale-serve from the pristine disk cache, upstream errors
//! forwarded, and the fail-closed handling of HTML-only upstreams.

mod common;

use std::time::SystemTime;

use common::StartProxy;
use common::{simple_json, TestProxy, OLD, SHA, SIMPLE_CTYPE, TOO_NEW};

const JSON_ACCEPT: &[(&str, &str)] = &[("accept", SIMPLE_CTYPE)];

#[tokio::test]
async fn dead_upstream_serves_stale_cache_filtered_and_rewritten() {
    let proxy = TestProxy::builder()
        .cooldown_days(1)
        .dead_upstream()
        .start_proxy()
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
    let proxy = TestProxy::builder().dead_upstream().start_proxy().await;

    assert_eq!(proxy.get_simple("foo", JSON_ACCEPT).await.status(), 502);
}

#[tokio::test]
async fn upstream_500_is_forwarded() {
    let proxy = TestProxy::builder().start_proxy().await;
    proxy.mock_simple_status("foo", 500).await;

    assert_eq!(proxy.get_simple("foo", JSON_ACCEPT).await.status(), 500);
}

#[tokio::test]
async fn upstream_404_is_forwarded() {
    let proxy = TestProxy::builder().start_proxy().await;
    proxy.mock_simple_status("foo", 404).await;

    assert_eq!(proxy.get_simple("foo", JSON_ACCEPT).await.status(), 404);
}

#[tokio::test]
async fn html_only_upstream_fails_closed_under_cooldown() {
    let proxy = TestProxy::builder().cooldown_days(1).start_proxy().await;
    proxy
        .mock_simple_ctype("foo", "<html>mirror</html>", "\"e1\"", "text/html")
        .await;

    // Nothing datable -> withhold every file rather than serve ungated. Served
    // as an empty index, not an error, so a resolver can fall through to an
    // index that does date the package instead of aborting.
    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert!(doc["files"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn html_only_upstream_is_normalized_not_passed_through() {
    // An HTML index is parsed into the PEP 691 model, so the client gets the
    // representation it asked for instead of whatever dialect upstream spoke.
    // A page with no links normalizes to an index with no files.
    let proxy = TestProxy::builder().start_proxy().await;
    proxy
        .mock_simple_ctype("foo", "<html>mirror</html>", "\"e1\"", "text/html")
        .await;

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    assert!(resp.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("application/vnd.pypi.simple.v1+json"));
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert!(doc["files"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_unrecognized_body_still_passes_through_without_cooldown() {
    // Passthrough now covers only bodies that are neither JSON nor HTML.
    let proxy = TestProxy::builder().start_proxy().await;
    proxy
        .mock_simple_ctype("foo", "raw bytes", "\"e1\"", "application/octet-stream")
        .await;

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "raw bytes");
}

#[tokio::test]
async fn upstream_5xx_serves_cached_copy() {
    let proxy = TestProxy::builder()
        .cache_ttl(std::time::Duration::ZERO)
        .start_proxy()
        .await;
    let body = simple_json("foo", &[("foo-1.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("foo", &body, "\"e1\"").await;
    assert_eq!(proxy.get_simple("foo", JSON_ACCEPT).await.status(), 200);

    // Upstream degrades to 503; the zero TTL forces a refetch, which must
    // fall back to the cached copy instead of forwarding the outage.
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/foo/"))
        .respond_with(wiremock::ResponseTemplate::new(503))
        .with_priority(1)
        .mount(&proxy.server.mock_upstream)
        .await;

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    let served: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(served["files"][0]["filename"], "foo-1.0.0.tar.gz");
}
