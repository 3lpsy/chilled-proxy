//! Version doc route: `GET /{name}/{version}` is derived locally from the
//! filtered packument, so hidden versions stay hidden.

mod common;

use common::{TestProxy, OLD, TOO_NEW};

#[tokio::test]
async fn version_doc_returns_the_version_object() {
    let proxy = TestProxy::builder().start().await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", OLD)])
        .await;

    let resp = proxy.get_version("lodash", "1.0.0").await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(doc["name"], "lodash");
    assert_eq!(doc["version"], "1.0.0");
    // The version doc carries the rewritten (proxied) tarball URL.
    assert_eq!(
        doc["dist"]["tarball"],
        "http://localhost:3080/npm/lodash/-/lodash-1.0.0.tgz"
    );
}

#[tokio::test]
async fn filtered_version_doc_is_404() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;

    let resp = proxy.get_version("lodash", "2.0.0").await;
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "Not found");
}

#[tokio::test]
async fn absent_version_doc_is_404() {
    let proxy = TestProxy::builder().start().await;
    proxy.mock_packument("lodash", &[("1.0.0", OLD)]).await;

    let resp = proxy.get_version("lodash", "9.9.9").await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn dist_tag_resolves_to_a_version_doc() {
    // npm resolves `GET /pkg/latest` through dist-tags, not just versions.
    let proxy = TestProxy::builder().start().await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", OLD)])
        .await;

    let resp = proxy.get("/lodash/latest").await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(doc["version"], "2.0.0");
}

#[tokio::test]
async fn dist_tag_follows_the_filtered_view() {
    // Under cooldown `latest` was repointed, so the tag resolves to the newest
    // version the proxy actually serves.
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;

    let doc: serde_json::Value = proxy.get("/lodash/latest").await.json().await.unwrap();
    assert_eq!(doc["version"], "1.0.0");
}

#[tokio::test]
async fn unknown_dist_tag_is_404() {
    let proxy = TestProxy::builder().start().await;
    proxy.mock_packument("lodash", &[("1.0.0", OLD)]).await;

    assert_eq!(proxy.get("/lodash/nope").await.status(), 404);
}
