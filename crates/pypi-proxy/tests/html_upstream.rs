//! HTML-only upstreams: a PEP 503 index is normalized to the PEP 691 model at
//! ingest, so it is age-gated, rewritten, cached, and rendered exactly like a
//! JSON one. Indexes that publish no upload times stay fail-closed.

mod common;

use common::{simple_html, TestProxy, OLD, SHA, SIMPLE_CTYPE, TOO_NEW};

/// The content type an HTML-only upstream answers with.
const HTML_CTYPE: &str = "text/html; charset=utf-8";

const JSON_ACCEPT: &[(&str, &str)] = &[("accept", SIMPLE_CTYPE)];
const HTML_ACCEPT: &[(&str, &str)] = &[("accept", "text/html")];

/// Mounts an HTML-only upstream for `foo` with the given files.
async fn html_proxy(cooldown_days: u64, files: &[(&str, &str, &str)]) -> TestProxy {
    let proxy = TestProxy::builder()
        .cooldown_days(cooldown_days)
        .start()
        .await;
    let body = simple_html("foo", files);
    proxy
        .mock_simple_ctype("foo", &body, "\"e1\"", HTML_CTYPE)
        .await;
    proxy
}

#[tokio::test]
async fn an_html_upstream_is_age_gated_like_json() {
    // The whole point: an index that only speaks HTML is still gated.
    let proxy = html_proxy(
        1,
        &[
            ("foo-1.0.0.tar.gz", OLD, SHA),
            ("foo-2.0.0.tar.gz", TOO_NEW, SHA),
        ],
    )
    .await;

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    let files = doc["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "the too-new file must be withheld");
    assert_eq!(files[0]["filename"], "foo-1.0.0.tar.gz");
    // Versions are recomputed from what survived, as with a JSON upstream.
    let versions = doc["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0], "1.0.0");
}

#[tokio::test]
async fn an_html_upstream_serves_both_representations() {
    let proxy = html_proxy(1, &[("foo-1.0.0.tar.gz", OLD, SHA)]).await;

    let json = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(json.status(), 200);
    assert!(json.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with(SIMPLE_CTYPE));

    let html = proxy.get_simple("foo", HTML_ACCEPT).await;
    assert_eq!(html.status(), 200);
    let body = html.text().await.unwrap();
    assert!(body.contains("foo-1.0.0.tar.gz"), "body: {body}");
}

#[tokio::test]
async fn file_urls_from_html_are_rewritten_through_the_proxy() {
    // Unrewritten URLs would let clients fetch straight from upstream, which is
    // exactly how gating gets bypassed.
    let proxy = html_proxy(1, &[("foo-1.0.0.tar.gz", OLD, SHA)]).await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    let url = doc["files"][0]["url"].as_str().unwrap();
    assert!(url.contains("/pypi/files/foo/"), "url: {url}");
    assert!(!url.contains("files.pythonhosted.org"), "url: {url}");
    // The hash survives normalization; clients verify against it.
    assert_eq!(doc["files"][0]["hashes"]["sha256"], SHA);
}

#[tokio::test]
async fn an_html_index_without_upload_times_is_refused_under_cooldown() {
    // Fail-closed: nothing datable means the cooldown cannot be honored, and a
    // loud 502 beats silently serving an ungated index.
    let proxy = html_proxy(1, &[("foo-1.0.0.tar.gz", "", SHA)]).await;

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 502);
    let body = resp.text().await.unwrap();
    assert!(body.contains("upload times"), "body: {body}");
}

#[tokio::test]
async fn an_html_index_without_upload_times_still_serves_ungated() {
    // With no cooldown there is nothing to gate, so the index is usable — and
    // now normalized, so its URLs are rewritten and its files cached.
    let proxy = html_proxy(0, &[("foo-1.0.0.tar.gz", "", SHA)]).await;

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(doc["files"].as_array().unwrap().len(), 1);
    let url = doc["files"][0]["url"].as_str().unwrap();
    assert!(url.contains("/pypi/files/foo/"), "url: {url}");
}

#[tokio::test]
async fn a_partially_dated_html_index_drops_only_the_undatable_files() {
    // Per-file fail-closed, matching the JSON path: an entry with no upload
    // time cannot be shown to be old enough, so it goes.
    let proxy = html_proxy(
        1,
        &[
            ("foo-1.0.0.tar.gz", OLD, SHA),
            ("foo-2.0.0.tar.gz", "", SHA),
        ],
    )
    .await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    let files = doc["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["filename"], "foo-1.0.0.tar.gz");
}

#[tokio::test]
async fn a_downloaded_file_from_an_html_index_is_gated_and_cached() {
    // The download side of the same mount: restrict-downloads reads the cached
    // pristine document, which is the normalized one.
    let proxy = TestProxy::builder()
        .cooldown_days(1)
        .restrict_downloads()
        .start()
        .await;
    let body = simple_html(
        "foo",
        &[
            ("foo-1.0.0.tar.gz", OLD, SHA),
            ("foo-2.0.0.tar.gz", TOO_NEW, SHA),
        ],
    );
    proxy
        .mock_simple_ctype("foo", &body, "\"e1\"", HTML_CTYPE)
        .await;
    proxy.mock_file("foo-1.0.0.tar.gz", b"payload").await;

    // Populate the pristine cache the gate reads.
    assert_eq!(proxy.get_simple("foo", JSON_ACCEPT).await.status(), 200);

    let old = proxy.download("foo", "foo-1.0.0.tar.gz").await;
    assert_eq!(old.status(), 200);
    assert_eq!(old.bytes().await.unwrap().as_ref(), b"payload");

    let too_new = proxy.download("foo", "foo-2.0.0.tar.gz").await;
    assert_eq!(too_new.status(), 403);
}

#[tokio::test]
async fn a_non_index_body_is_still_refused_under_cooldown() {
    // Normalizing HTML must not weaken the guarantee for bodies that are
    // neither JSON nor HTML.
    let proxy = TestProxy::builder().cooldown_days(1).start().await;
    proxy
        .mock_simple_ctype("foo", "not an index", "\"e1\"", "application/octet-stream")
        .await;

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 502);
}
