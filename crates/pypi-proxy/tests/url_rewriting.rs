//! File-URL rewriting: every served URL points back at the proxy's files
//! route, hashes stay intact, and the HTML carries fragments and attributes.

mod common;

use common::{simple_json, TestProxy, OLD, SHA, SIMPLE_CTYPE};

const JSON_ACCEPT: &[(&str, &str)] = &[("accept", SIMPLE_CTYPE)];

#[tokio::test]
async fn json_file_urls_point_at_the_proxy() {
    let proxy = TestProxy::builder().start().await;
    let body = simple_json(
        "foo",
        &[
            ("foo-1.0.0.tar.gz", OLD, SHA),
            ("foo-1.0.0-py3-none-any.whl", OLD, SHA),
        ],
    );
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    for file in doc["files"].as_array().unwrap() {
        let url = file["url"].as_str().unwrap();
        let filename = file["filename"].as_str().unwrap();
        assert_eq!(
            url,
            format!("http://localhost:3080/pypi/files/foo/packages/aa/bb/cc/{filename}")
        );
    }
}

#[tokio::test]
async fn html_anchors_carry_fragment_and_requires_python() {
    let proxy = TestProxy::builder().start().await;
    let body = simple_json("foo", &[("foo-1.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let html = proxy.get_simple("foo", &[]).await.text().await.unwrap();
    assert!(html.contains(&format!(
        "href=\"http://localhost:3080/pypi/files/foo/packages/aa/bb/cc/foo-1.0.0.tar.gz#sha256={SHA}\""
    )));
    // The fixture's `>=3.8` must arrive HTML-escaped.
    assert!(html.contains(" data-requires-python=\"&gt;=3.8\""));
    assert!(!html.contains("data-requires-python=\">=3.8\""));
}

#[tokio::test]
async fn hashes_object_is_untouched_in_json() {
    let proxy = TestProxy::builder().start().await;
    let body = simple_json("foo", &[("foo-1.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        doc["files"][0]["hashes"],
        serde_json::json!({ "sha256": SHA })
    );
}
