//! Scoped package handling: literal and percent-encoded scope separators must
//! converge, and double-encoding must be rejected.

mod common;

use common::{TestProxy, OLD, TARBALL_BYTES};

#[tokio::test]
async fn encoded_and_literal_scope_paths_serve_the_same_packument() {
    let proxy = TestProxy::builder().start().await;
    proxy.mock_packument("@scope/pkg", &[("1.0.0", OLD)]).await;

    let literal = proxy.get("/@scope/pkg").await;
    assert_eq!(literal.status(), 200);
    let literal_body = literal.text().await.unwrap();

    // npm itself requests `/@scope%2fpkg`; one decode makes them identical.
    let encoded = proxy.get("/@scope%2fpkg").await;
    assert_eq!(encoded.status(), 200);
    assert_eq!(encoded.text().await.unwrap(), literal_body);

    // Same package: served from the same cache, upstream hit only once.
    assert_eq!(proxy.upstream_hits("/@scope/pkg").await, 1);
}

#[tokio::test]
async fn scoped_tarball_downloads_and_caches() {
    let proxy = TestProxy::builder().start().await;
    proxy
        .mock_tarball("@scope/pkg", "pkg-1.0.0.tgz", TARBALL_BYTES)
        .await;

    let resp = proxy.download_tarball("@scope/pkg", "pkg-1.0.0.tgz").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), TARBALL_BYTES);

    let path = proxy.tarball_cache_path("@scope/pkg", "pkg-1.0.0.tgz");
    assert_eq!(std::fs::read(&path).unwrap(), TARBALL_BYTES);
}

#[tokio::test]
async fn double_encoded_scope_separator_is_rejected() {
    let proxy = TestProxy::builder().start().await;

    let resp = proxy.get("/@scope%252fpkg").await;
    assert_eq!(resp.status(), 404);
    assert_eq!(proxy.upstream_total().await, 0);
}
