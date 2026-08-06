//! Packument serve path: caching, conditional GET / ETag, TTL revalidation,
//! and URL rewriting. Cooldown is left off here (covered separately), so
//! bodies pass through the rewrite-only path.

mod common;

use std::fs;
use std::time::Duration;

use common::StartProxy;
use common::{TestProxy, ETAG, OLD};

/// The `.rw` marker for the default upstream ETag (rewritten, unfiltered).
const RW_MARKER: &str = "W/\"etag123.rw\"";

#[tokio::test]
async fn not_cached_fetches_upstream_and_writes_disk() {
    let proxy = TestProxy::builder().start_proxy().await;
    let body = proxy.mock_packument("lodash", &[("1.0.0", OLD)]).await;

    let resp = proxy.get_packument("lodash", &[]).await;
    assert_eq!(resp.status(), 200);
    let served: serde_json::Value = resp.json().await.unwrap();
    assert!(served["versions"].get("1.0.0").is_some());

    // Exactly one upstream fetch, and the PRISTINE body landed on disk.
    assert_eq!(proxy.upstream_hits("/lodash").await, 1);
    let path = proxy.packument_cache_path("lodash");
    assert_eq!(fs::read_to_string(&path).unwrap(), body);
}

#[tokio::test]
async fn cached_within_ttl_serves_without_upstream() {
    let proxy = TestProxy::builder()
        .cache_ttl(Duration::from_secs(3600))
        .start_proxy()
        .await;
    proxy.mock_packument("lodash", &[("1.0.0", OLD)]).await;

    assert_eq!(proxy.get_packument("lodash", &[]).await.status(), 200);
    let second = proxy.get_packument("lodash", &[]).await;
    assert_eq!(second.status(), 200);
    let served: serde_json::Value = second.json().await.unwrap();
    assert!(served["versions"].get("1.0.0").is_some());

    // Second request served from the warm cache — upstream hit only once.
    assert_eq!(proxy.upstream_hits("/lodash").await, 1);
}

#[tokio::test]
async fn ttl_zero_revalidates_and_serves_from_disk_on_304() {
    let proxy = TestProxy::builder()
        .cache_ttl(Duration::ZERO)
        .start_proxy()
        .await;
    proxy.mock_packument("lodash", &[("1.0.0", OLD)]).await;
    proxy.mock_packument_304("lodash", ETAG).await;

    assert_eq!(proxy.get_packument("lodash", &[]).await.status(), 200);
    // Expired immediately -> conditional revalidation; upstream 304, body
    // still served from the disk cache.
    let second = proxy.get_packument("lodash", &[]).await;
    assert_eq!(second.status(), 200);
    let served: serde_json::Value = second.json().await.unwrap();
    assert!(served["versions"].get("1.0.0").is_some());

    assert_eq!(proxy.upstream_hits("/lodash").await, 2);
}

#[tokio::test]
async fn client_revalidation_with_marked_etag_gets_304() {
    let proxy = TestProxy::builder().start_proxy().await;
    proxy.mock_packument("lodash", &[("1.0.0", OLD)]).await;

    let first = proxy.get_packument("lodash", &[]).await;
    // Rewritten body: weak `.rw`-marked ETag, and never the upstream Last-Modified.
    assert_eq!(first.headers()["etag"], RW_MARKER);
    assert!(first.headers().get("last-modified").is_none());

    // Client presents the validator it was given -> 304, marker echoed.
    let second = proxy
        .get_packument("lodash", &[("if-none-match", RW_MARKER)])
        .await;
    assert_eq!(second.status(), 304);
    assert_eq!(second.headers()["etag"], RW_MARKER);
    assert!(second.text().await.unwrap().is_empty());
    assert_eq!(proxy.upstream_hits("/lodash").await, 1);
}

#[tokio::test]
async fn tarball_urls_rewritten_with_cooldown_off_and_on() {
    for days in [0, 7] {
        let proxy = TestProxy::builder().cooldown_days(days).start_proxy().await;
        proxy.mock_packument("lodash", &[("1.0.0", OLD)]).await;

        let served: serde_json::Value = proxy
            .get_packument("lodash", &[])
            .await
            .json()
            .await
            .unwrap();
        assert_eq!(
            served["versions"]["1.0.0"]["dist"]["tarball"],
            "http://localhost:3080/npm/lodash/-/lodash-1.0.0.tgz",
            "cooldown_days: {days}"
        );
    }
}

#[tokio::test]
async fn full_packument_requested_even_for_corgi_clients() {
    let proxy = TestProxy::builder().start_proxy().await;
    proxy.mock_packument("lodash", &[("1.0.0", OLD)]).await;

    // npm sends the abbreviated "corgi" Accept; the proxy must still request
    // the full doc upstream (the corgi form lacks the `time` map).
    let resp = proxy
        .get_packument(
            "lodash",
            &[("accept", "application/vnd.npm.install-v1+json")],
        )
        .await;
    assert_eq!(resp.status(), 200);
    let served: serde_json::Value = resp.json().await.unwrap();
    assert!(served.get("time").is_some(), "full packument served");

    let requests = proxy
        .server
        .mock_upstream
        .received_requests()
        .await
        .unwrap();
    let upstream_req = requests.iter().find(|r| r.url.path() == "/lodash").unwrap();
    assert_eq!(
        upstream_req.headers.get("accept").unwrap(),
        "application/json"
    );
}
