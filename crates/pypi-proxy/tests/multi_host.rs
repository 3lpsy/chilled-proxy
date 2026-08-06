//! Indexes that spread their files across several hosts.
//!
//! PyPI keeps every file on one host, but an index is free not to: the PyTorch
//! index links `torch` at its own CDN, its dependencies at PyPI's, and some
//! wheels relatively. A mount reconstructing every download against one pinned
//! host can only ever serve one of those slices, so the file's host is read
//! from the index document instead — but only when the operator allows it.

mod common;

use common::StartProxy;
use common::{simple_json, TestProxy, OLD, SHA, SIMPLE_CTYPE};
use serde_json::json;
use wiremock::matchers::{method, path as match_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const JSON_ACCEPT: &[(&str, &str)] = &[("accept", SIMPLE_CTYPE)];

/// A second file host, standing in for the index's other CDN.
async fn other_cdn(body: &[u8]) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(match_path("/elsewhere/foo-1.0.0.tar.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
        .mount(&server)
        .await;
    server
}

/// A simple doc whose one file lives at `url`.
fn doc_pointing_at(url: &str) -> String {
    json!({
        "meta": {"api-version": "1.0"},
        "name": "foo",
        "versions": ["1.0.0"],
        "files": [{
            "filename": "foo-1.0.0.tar.gz",
            "url": url,
            "hashes": {"sha256": SHA},
            "upload-time": OLD,
        }],
    })
    .to_string()
}

#[tokio::test]
async fn a_file_host_named_by_the_index_is_used_when_allowed() {
    let cdn = other_cdn(b"from-the-other-cdn").await;
    let cdn_url = format!("{}/elsewhere/foo-1.0.0.tar.gz", cdn.uri());
    // 127.0.0.1 is the mock's host, so allowing it stands in for an operator
    // declaring the index's second CDN.
    let proxy = TestProxy::builder()
        .start_proxy_with_hosts(&["127.0.0.1"])
        .await;
    proxy
        .mock_simple("foo", &doc_pointing_at(&cdn_url), "\"e1\"")
        .await;

    // The rewritten URL keeps the *other* host's path, not the pinned host's.
    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    let served = doc["files"][0]["url"].as_str().unwrap().to_owned();
    assert!(
        served.contains("/pypi/files/foo/elsewhere/"),
        "url: {served}"
    );

    let resp = proxy
        .get(&served[served.find("/pypi").unwrap() + 5..])
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"from-the-other-cdn");
}

#[tokio::test]
async fn an_unallowed_file_host_falls_back_to_the_pinned_files_url() {
    // Substituting the pinned host is what `--pypi-files-url` is for (an
    // operator mirroring PyPI's file host), so an undeclared host must not be
    // honored — and must not fail either.
    let proxy = TestProxy::builder().start_proxy().await;
    let body = simple_json("foo", &[("foo-1.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("foo", &body, "\"e1\"").await;
    // The fixture doc names files.pythonhosted.org; the mount allows only the
    // mock. The file is served from the mock, at the doc's path.
    proxy
        .mock_file("foo-1.0.0.tar.gz", b"from-the-pinned-host")
        .await;

    let resp = proxy.download("foo", "foo-1.0.0.tar.gz").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.bytes().await.unwrap().as_ref(),
        b"from-the-pinned-host"
    );
}

#[tokio::test]
async fn an_allowed_host_is_matched_case_insensitively_and_ignores_port() {
    let cdn = other_cdn(b"payload").await;
    let cdn_url = format!("{}/elsewhere/foo-1.0.0.tar.gz", cdn.uri());
    // A host is a host whatever its case; the mock's port differs from the
    // mount's pinned one, which is the point.
    let proxy = TestProxy::builder()
        .start_proxy_with_hosts(&["127.0.0.1"])
        .await;
    proxy
        .mock_simple("foo", &doc_pointing_at(&cdn_url), "\"e1\"")
        .await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    let served = doc["files"][0]["url"].as_str().unwrap().to_owned();
    let resp = proxy
        .get(&served[served.find("/pypi").unwrap() + 5..])
        .await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn a_multi_host_index_serves_every_slice() {
    // The shape that broke in production: one index, two file hosts. Both must
    // work from the same mount.
    let cdn = other_cdn(b"cdn-hosted").await;
    let cdn_url = format!("{}/elsewhere/foo-1.0.0.tar.gz", cdn.uri());
    let proxy = TestProxy::builder()
        .start_proxy_with_hosts(&["127.0.0.1"])
        .await;

    // `foo` lives on the other CDN...
    proxy
        .mock_simple("foo", &doc_pointing_at(&cdn_url), "\"e1\"")
        .await;
    // ...and `bar` on the pinned files host, via the ordinary fixture layout.
    proxy
        .mock_simple(
            "bar",
            &simple_json("bar", &[("bar-1.0.0.tar.gz", OLD, SHA)]),
            "\"e2\"",
        )
        .await;
    proxy.mock_file("bar-1.0.0.tar.gz", b"pinned-hosted").await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    let foo_url = doc["files"][0]["url"].as_str().unwrap().to_owned();
    let foo = proxy
        .get(&foo_url[foo_url.find("/pypi").unwrap() + 5..])
        .await;
    assert_eq!(foo.status(), 200);
    assert_eq!(foo.bytes().await.unwrap().as_ref(), b"cdn-hosted");

    let bar = proxy.download("bar", "bar-1.0.0.tar.gz").await;
    assert_eq!(bar.status(), 200);
    assert_eq!(bar.bytes().await.unwrap().as_ref(), b"pinned-hosted");
}

#[tokio::test]
async fn a_metadata_sidecar_follows_its_distribution_to_the_named_host() {
    // PEP 658 sidecars have no entry of their own; they sit beside the
    // distribution, so they must resolve to the same host, not the pinned one.
    let cdn = MockServer::start().await;
    Mock::given(method("GET"))
        .and(match_path("/elsewhere/foo-1.0.0.tar.gz.metadata"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"Name: foo".to_vec()))
        .mount(&cdn)
        .await;
    let cdn_url = format!("{}/elsewhere/foo-1.0.0.tar.gz", cdn.uri());

    let proxy = TestProxy::builder()
        .start_proxy_with_hosts(&["127.0.0.1"])
        .await;
    proxy
        .mock_simple("foo", &doc_pointing_at(&cdn_url), "\"e1\"")
        .await;

    let resp = proxy
        .get("/files/foo/elsewhere/foo-1.0.0.tar.gz.metadata")
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"Name: foo");
}

#[tokio::test]
async fn the_gate_and_the_download_agree_on_which_file_they_mean() {
    // Age gate and URL resolution read the same index entry, so a file the gate
    // cleared is the file that gets fetched — from the host the index named.
    let cdn = other_cdn(b"old-enough").await;
    let cdn_url = format!("{}/elsewhere/foo-1.0.0.tar.gz", cdn.uri());
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy_with_hosts(&["127.0.0.1"])
        .await;
    proxy
        .mock_simple("foo", &doc_pointing_at(&cdn_url), "\"e1\"")
        .await;

    let resp = proxy.get("/files/foo/elsewhere/foo-1.0.0.tar.gz").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"old-enough");
}
