//! Upstream failure handling: stale-serve from the pristine cache, 502 with
//! the npm error envelope, and upstream error status forwarding.

mod common;

use std::time::SystemTime;

use common::StartProxy;
use common::{packument, TestProxy, OLD, TOO_NEW};

#[tokio::test]
async fn stale_cache_served_when_upstream_unreachable_still_filtered() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .dead_upstream()
        .start_proxy()
        .await;
    // Pre-seed a pristine packument; upstream is a refused port.
    let body = packument(
        "lodash",
        &[("1.0.0", OLD), ("2.0.0", TOO_NEW)],
        "http://dead.invalid/",
    );
    proxy.seed_packument("lodash", &body, SystemTime::now());

    let resp = proxy.get_packument("lodash", &[]).await;
    assert_eq!(resp.status(), 200);
    let served: serde_json::Value = resp.json().await.unwrap();
    assert!(
        served["versions"].get("1.0.0").is_some(),
        "stale copy served"
    );
    assert!(served["versions"].get("2.0.0").is_none(), "still filtered");
}

#[tokio::test]
async fn no_cache_and_unreachable_upstream_is_502_envelope() {
    let proxy = TestProxy::builder().dead_upstream().start_proxy().await;

    // Nothing seeded, nothing cached, upstream dead.
    let resp = proxy.get_packument("lodash", &[]).await;
    assert_eq!(resp.status(), 502);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("error").is_some(), "npm error envelope");
}

#[tokio::test]
async fn upstream_error_status_is_forwarded() {
    let proxy = TestProxy::builder().start_proxy().await;
    proxy.mock_packument_status("lodash", 500).await;

    assert_eq!(proxy.get_packument("lodash", &[]).await.status(), 500);
    assert_eq!(proxy.upstream_hits("/lodash").await, 1);
}

#[tokio::test]
async fn upstream_5xx_serves_cached_copy() {
    let proxy = TestProxy::builder()
        .cache_ttl(std::time::Duration::ZERO)
        .start_proxy()
        .await;
    proxy.mock_packument("foo", &[("1.0.0", OLD)]).await;
    assert_eq!(proxy.get_packument("foo", &[]).await.status(), 200);

    // Upstream degrades to 503; the zero TTL forces a refetch, which must
    // fall back to the cached copy instead of forwarding the outage.
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/foo"))
        .respond_with(wiremock::ResponseTemplate::new(503))
        .with_priority(1)
        .mount(&proxy.server.mock_upstream)
        .await;

    let resp = proxy.get_packument("foo", &[]).await;
    assert_eq!(resp.status(), 200);
    let served: serde_json::Value = resp.json().await.unwrap();
    assert!(served["versions"].get("1.0.0").is_some());
}
