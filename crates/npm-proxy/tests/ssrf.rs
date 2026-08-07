//! SSRF / URL-injection / traversal hardening: hostile names, versions, and
//! tarball file names must be rejected with `404` *before* any upstream
//! request, since these segments feed `Url::join` and cache paths.

mod common;

use common::StartProxy;
use common::TestProxy;

/// Paths that must be rejected without contacting upstream.
const VECTORS: &[&str] = &[
    "/..",                         // traversal as a name
    "/.hidden",                    // leading dot
    "/_underscore-start",          // leading underscore
    "/a/b/c",                      // extra path segments
    "/http:",                      // scheme prefix
    "/127.0.0.1:8080",             // host:port
    "/user@host",                  // userinfo@host
    "/%2e%2e%2f",                  // encoded ../
    "/a%5Cb",                      // encoded backslash
    "/@evil.com",                  // scope without a name
    "/@/pkg",                      // empty scope
    "/@.bad/pkg",                  // leading dot in scope
    "/@scope%252fpkg",             // double-encoded scope separator
    "/lodash/-/other-1.0.0.tgz",   // tarball file mismatch
    "/lodash/-/..%2F..%2Fetc.tgz", // traversal in the tarball file
    "/lodash/1.0%2F0",             // slash smuggled into a version
    "/lodash/1.0.0@x",             // @ in a version
];

#[tokio::test]
async fn injection_vectors_are_rejected_before_upstream() {
    let proxy = TestProxy::builder().start_proxy().await;
    for path in VECTORS {
        assert_eq!(proxy.get(path).await.status(), 404, "path: {path}");
    }
    // A 215-char name must also be rejected (npm caps names at 214).
    let long = format!("/{}", "a".repeat(215));
    assert_eq!(proxy.get(&long).await.status(), 404);

    // None of them may have reached the (mock) upstream.
    assert_eq!(proxy.upstream_total().await, 0);
}
