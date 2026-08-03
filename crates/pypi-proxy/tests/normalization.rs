//! PEP 503 name normalization: redirects to the canonical path, normalized
//! upstream/cache identity, and normalized override lookup.

mod common;

use common::{simple_json, TestProxy, SHA, SIMPLE_CTYPE, TOO_NEW};

const JSON_ACCEPT: &[(&str, &str)] = &[("accept", SIMPLE_CTYPE)];

#[tokio::test]
async fn non_normalized_name_redirects_to_normalized_path() {
    let proxy = TestProxy::builder().start().await;

    let resp = proxy.get_no_redirect("/simple/Foo.Bar_baz/").await;
    assert_eq!(resp.status(), 301);
    assert_eq!(resp.headers()["location"], "/pypi/simple/foo-bar-baz/");
    assert_eq!(proxy.upstream_total().await, 0);
}

#[tokio::test]
async fn upstream_fetch_and_cache_use_the_normalized_name() {
    let proxy = TestProxy::builder().start().await;
    let body = simple_json(
        "foo-bar-baz",
        &[("foo_bar_baz-1.0.0.tar.gz", common::OLD, SHA)],
    );
    proxy.mock_simple("foo-bar-baz", &body, "\"e1\"").await;

    // The redirect-following client lands on the canonical page.
    let resp = proxy.get_simple("Foo.Bar_baz", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(doc["files"][0]["filename"], "foo_bar_baz-1.0.0.tar.gz");

    // Upstream was fetched at the normalized path only.
    assert_eq!(
        proxy
            .upstream_hits(&proxy.simple_upstream_path("foo-bar-baz"))
            .await,
        1
    );
    assert_eq!(proxy.upstream_total().await, 1);

    // And the cache file lives under the normalized name.
    assert!(proxy.simple_cache_path("foo-bar-baz").exists());
}

#[tokio::test]
async fn override_exempts_requests_under_any_spelling() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .override_package("foo-bar-baz")
        .start()
        .await;
    let body = simple_json("foo-bar-baz", &[("foo_bar_baz-9.0.0.tar.gz", TOO_NEW, SHA)]);
    proxy.mock_simple("foo-bar-baz", &body, "\"e1\"").await;

    // Requested under a non-normalized spelling; the override still applies,
    // so the too-new file is served (rewrite-only etag, no cooldown marker).
    let resp = proxy.get_simple("Foo.Bar_baz", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["etag"], "W/\"e1.rw.j\"");
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(doc["files"].as_array().unwrap().len(), 1);
    assert_eq!(doc["versions"], serde_json::json!(["9.0.0"]));
}
