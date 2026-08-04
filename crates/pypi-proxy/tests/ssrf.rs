//! SSRF / traversal / URL-injection hardening.
//!
//! Project names and file paths are interpolated into `Url::join` (to build
//! upstream requests) and into filesystem cache paths, so a crafted segment
//! must never change the upstream host or escape the cache dir. Paths are
//! percent-decoded exactly once (residual `%` rejects), names are held to the
//! PEP 503 charset, and file paths to the `packages/…` shape — every vector
//! here must be rejected *before* any upstream request.

mod common;

use common::TestProxy;

/// Simple-index vectors that must be rejected without contacting upstream.
fn simple_vectors() -> Vec<String> {
    let long_name = "a".repeat(129);
    vec![
        "/simple/../".into(),            // traversal
        "/simple/.hidden/".into(),       // leading dot
        "/simple/-leading/".into(),      // leading separator
        format!("/simple/{long_name}/"), // over the length cap
        "/simple/%2e%2e/".into(),        // encoded ..
        "/simple/%252e%252e/".into(),    // double-encoded (residual %)
        "/simple/evil.com%2Fx/".into(),  // encoded slash
        "/simple/a%20b/".into(),         // encoded space
        "/simple/a%5Cb/".into(),         // encoded backslash
    ]
}

/// Files-route vectors that must be rejected without contacting upstream.
fn file_vectors() -> Vec<String> {
    vec![
        "/files/foo/packages/aa/bb/cc/f.exe".into(), // wrong extension
        "/files/foo/https://evil.com/a/b/f.whl".into(), // absolute URL smuggle
        "/files/foo/packages/aa%5Cbb/cc/dd/f.whl".into(), // encoded backslash
        "/files/Foo.Bar/packages/aa/bb/cc/f.whl".into(), // non-normalized project
        "/files/foo/packages/aa/bb/cc/%252e.whl".into(), // residual %
        "/files/../packages/aa/bb/cc/f.whl".into(),  // project traversal
    ]
}

#[tokio::test]
async fn simple_injection_vectors_are_rejected() {
    let proxy = TestProxy::builder().start().await;
    for path in simple_vectors() {
        let resp = proxy.get_no_redirect(&path).await;
        assert_eq!(resp.status(), 404, "path: {path}");
    }
    // None of them may have reached the (mock) upstream.
    assert_eq!(proxy.upstream_total().await, 0);
}

#[tokio::test]
async fn file_injection_vectors_are_rejected() {
    let proxy = TestProxy::builder().start().await;
    for path in file_vectors() {
        let resp = proxy.get_no_redirect(&path).await;
        assert_eq!(resp.status(), 404, "path: {path}");
    }
    assert_eq!(proxy.upstream_total().await, 0);
}

#[tokio::test]
async fn a_clean_path_of_an_unknown_layout_is_forwarded_and_404s() {
    // Supporting indexes whose file layout is not PyPI's (PyTorch serves
    // `whl/cpu/<file>`) means a clean, bounded path no longer has to match
    // `packages/<a>/<b>/<hash>` to be tried. Such a path reaches the *pinned*
    // files host and 404s there; it can never name a different host, escape the
    // host's root, or carry a traversal segment — those stay rejected locally,
    // which `file_injection_vectors_are_rejected` and the `validate_fhp_path`
    // unit tests both cover.
    let proxy = TestProxy::builder().start().await;
    proxy.mock_file_status("f.whl", 404).await;

    let resp = proxy.get_no_redirect("/files/foo/whl/cpu/f.whl").await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn query_and_fragment_are_not_forwarded_upstream() {
    let proxy = TestProxy::builder().start().await;
    let body = common::simple_json("foo", &[("foo-1.0.0.tar.gz", common::OLD, common::SHA)]);
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let resp = proxy.get("/simple/foo/?x=http://evil.com#frag").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(proxy.upstream_hits("/foo/").await, 1);
    assert_eq!(proxy.upstream_total().await, 1);
}
