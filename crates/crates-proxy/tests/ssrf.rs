//! SSRF / URL-injection hardening: hostile crate names/versions (userinfo,
//! scheme/port, traversal, percent-encoded delimiters) must 404 *before* any
//! upstream request, since these segments feed `Url::join` and cache paths.
//! Positive tests confirm queries/fragments are dropped, not forwarded.

mod common;

use common::StartProxy;
use common::{ndjson, TestProxy, OLD};

/// Index-path name vectors that must be rejected without contacting upstream.
const INDEX_VECTORS: &[&str] = &[
    "/index/se/rd/a@b",            // userinfo separator
    "/index/se/rd/@evil.com",      // bare userinfo@host
    "/index/se/rd/a:b",            // scheme/port separator
    "/index/se/rd/http:",          // scheme prefix
    "/index/se/rd/127.0.0.1:8080", // host:port
    "/index/se/rd/evil.com",       // host-like (dot)
    "/index/se/rd/a%40b",          // encoded @
    "/index/se/rd/a%23b",          // encoded # (fragment)
    "/index/se/rd/a%3Fb",          // encoded ? (query)
    "/index/se/rd/a%2Fb",          // encoded / (segment injection)
    "/index/se/rd/a%5Cb",          // encoded backslash
    "/index/se/rd/a%20b",          // encoded space
];

/// Download name/version vectors that must be rejected without contacting upstream.
const DOWNLOAD_VECTORS: &[&str] = &[
    "/api/v1/crates/a@b/1.0.0/download",            // @ in name
    "/api/v1/crates/user@host/1.0.0/download",      // userinfo@host name
    "/api/v1/crates/evil.com/1.0.0/download",       // host-like name
    "/api/v1/crates/127.0.0.1:8080/1.0.0/download", // host:port name
    "/api/v1/crates/http:/1.0.0/download",          // scheme name
    "/api/v1/crates/serde/1.0.0@x/download",        // @ in version
    "/api/v1/crates/serde/1.0.0%23frag/download",   // encoded # in version
    "/api/v1/crates/serde/1.0.0%3Fq/download",      // encoded ? in version
    "/api/v1/crates/serde/1.0.0%2Fx/download",      // encoded / in version
    "/api/v1/crates/serde/..%2F..%2Fetc/download",  // traversal in version
];

#[tokio::test]
async fn index_injection_vectors_are_rejected() {
    let proxy = TestProxy::builder().start_proxy().await;
    for path in INDEX_VECTORS {
        assert_eq!(proxy.get(path).await.status(), 404, "path: {path}");
    }
    // None of them may have reached the (mock) upstream.
    assert_eq!(proxy.upstream_total().await, 0);
}

#[tokio::test]
async fn download_injection_vectors_are_rejected() {
    let proxy = TestProxy::builder().start_proxy().await;
    for path in DOWNLOAD_VECTORS {
        assert_eq!(proxy.get(path).await.status(), 404, "path: {path}");
    }
    assert_eq!(proxy.upstream_total().await, 0);
}

#[tokio::test]
async fn query_string_is_not_forwarded_upstream() {
    let proxy = TestProxy::builder().start_proxy().await;
    proxy
        .mock_index(
            "serde",
            &ndjson("serde", &[("1.0.0", OLD)]),
            "\"e\"",
            "Thu, 01 Jan 1970 00:00:00 GMT",
        )
        .await;

    // A query string attempting to smuggle a host is ignored; the request is
    // just crate `serde`, and upstream is hit only at the safe path.
    let resp = proxy.get("/index/se/rd/serde?dl=http://evil.com/x").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(proxy.upstream_hits("/se/rd/serde").await, 1);
    assert_eq!(proxy.upstream_total().await, 1);
}

#[tokio::test]
async fn fragment_is_not_forwarded_upstream() {
    let proxy = TestProxy::builder().start_proxy().await;
    proxy
        .mock_index(
            "serde",
            &ndjson("serde", &[("1.0.0", OLD)]),
            "\"e\"",
            "Thu, 01 Jan 1970 00:00:00 GMT",
        )
        .await;

    // The fragment is client-side only — the proxy never sees it, and certainly
    // never uses it to pick an upstream host.
    let resp = proxy.get("/index/se/rd/serde#http://evil.com").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(proxy.upstream_hits("/se/rd/serde").await, 1);
    assert_eq!(proxy.upstream_total().await, 1);
}
