//! Simple-index serve path: content negotiation, caching, conditional GET /
//! format-tagged ETags, and TTL revalidation. Cooldown is left off here
//! (covered separately), so bodies are rewritten but unfiltered.

mod common;

use std::fs;
use std::time::Duration;

use common::{simple_json, TestProxy, OLD, SHA, SIMPLE_CTYPE};

const JSON_ACCEPT: &[(&str, &str)] = &[("accept", SIMPLE_CTYPE)];

#[tokio::test]
async fn json_accept_serves_json() {
    let proxy = TestProxy::builder().start().await;
    let body = simple_json("requests", &[("requests-2.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("requests", &body, "\"e1\"").await;

    let resp = proxy.get_simple("requests", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], SIMPLE_CTYPE);
    assert_eq!(resp.headers()["vary"], "Accept");

    let doc: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(doc["files"][0]["filename"], "requests-2.0.0.tar.gz");
    assert_eq!(doc["versions"], serde_json::json!(["2.0.0"]));
}

#[tokio::test]
async fn html_or_absent_accept_serves_html() {
    let proxy = TestProxy::builder().start().await;
    let body = simple_json("requests", &[("requests-2.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("requests", &body, "\"e1\"").await;

    for headers in [&[][..], &[("accept", "text/html")][..]] {
        let resp = proxy.get_simple("requests", headers).await;
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.headers()["content-type"], "text/html; charset=utf-8");
        assert_eq!(resp.headers()["vary"], "Accept");
        let html = resp.text().await.unwrap();
        assert!(html.contains("<h1>Links for requests</h1>"));
        assert!(html.contains(">requests-2.0.0.tar.gz</a>"));
    }
}

#[tokio::test]
async fn missing_trailing_slash_redirects() {
    let proxy = TestProxy::builder().start().await;

    let resp = proxy.get_no_redirect("/simple/requests").await;
    assert_eq!(resp.status(), 301);
    assert_eq!(resp.headers()["location"], "/pypi/simple/requests/");
    // Pure redirect: upstream untouched.
    assert_eq!(proxy.upstream_total().await, 0);
}

#[tokio::test]
async fn project_list_is_served_in_both_formats() {
    let proxy = TestProxy::builder().start().await;

    let resp = proxy.get("/simple/").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "text/html; charset=utf-8");

    let resp = proxy.server.get("/simple/", JSON_ACCEPT).await;
    assert_eq!(resp.headers()["content-type"], SIMPLE_CTYPE);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(doc["projects"], serde_json::json!([]));
    assert_eq!(proxy.upstream_total().await, 0);
}

#[tokio::test]
async fn cold_fetch_caches_pristine_json_on_disk() {
    let proxy = TestProxy::builder().start().await;
    let body = simple_json("requests", &[("requests-2.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("requests", &body, "\"e1\"").await;

    assert_eq!(
        proxy.get_simple("requests", JSON_ACCEPT).await.status(),
        200
    );

    // The disk copy is the pristine upstream body (unrewritten URLs).
    let cached = fs::read_to_string(proxy.simple_cache_path("requests")).unwrap();
    assert_eq!(cached, body);
    assert!(cached.contains("files.pythonhosted.org"));
}

#[tokio::test]
async fn cached_within_ttl_serves_without_upstream() {
    let proxy = TestProxy::builder()
        .cache_ttl(Duration::from_secs(3600))
        .start()
        .await;
    let body = simple_json("requests", &[("requests-2.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("requests", &body, "\"e1\"").await;

    let first = proxy
        .get_simple("requests", JSON_ACCEPT)
        .await
        .text()
        .await
        .unwrap();
    let second = proxy.get_simple("requests", JSON_ACCEPT).await;
    assert_eq!(second.status(), 200);
    assert_eq!(second.text().await.unwrap(), first);
    assert_eq!(
        proxy
            .upstream_hits(&proxy.simple_upstream_path("requests"))
            .await,
        1
    );
}

#[tokio::test]
async fn expired_ttl_revalidates_with_upstream_304() {
    let proxy = TestProxy::builder().cache_ttl(Duration::ZERO).start().await;
    let body = simple_json("requests", &[("requests-2.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("requests", &body, "\"e1\"").await;
    proxy.mock_simple_304("requests", "\"e1\"").await;

    assert_eq!(
        proxy.get_simple("requests", JSON_ACCEPT).await.status(),
        200
    );
    // Expired immediately -> conditional revalidation; upstream 304, body still served.
    let second = proxy.get_simple("requests", JSON_ACCEPT).await;
    assert_eq!(second.status(), 200);
    let doc: serde_json::Value = second.json().await.unwrap();
    assert_eq!(doc["files"][0]["filename"], "requests-2.0.0.tar.gz");
    assert_eq!(
        proxy
            .upstream_hits(&proxy.simple_upstream_path("requests"))
            .await,
        2
    );
}

#[tokio::test]
async fn client_304_only_for_matching_format_tag() {
    let proxy = TestProxy::builder().start().await;
    let body = simple_json("requests", &[("requests-2.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("requests", &body, "\"e1\"").await;

    let first = proxy.get_simple("requests", JSON_ACCEPT).await;
    let etag = first.headers()["etag"].to_str().unwrap().to_owned();
    // Rewrite marker + JSON representation tag on the served validator.
    assert_eq!(etag, "W/\"e1.rw.j\"");

    // Same etag, same format -> 304 without touching upstream again.
    let revalidate = proxy
        .get_simple(
            "requests",
            &[("accept", SIMPLE_CTYPE), ("if-none-match", &etag)],
        )
        .await;
    assert_eq!(revalidate.status(), 304);
    assert!(revalidate.text().await.unwrap().is_empty());

    // Same etag but asking for HTML -> full 200 with the HTML-tagged etag.
    let html = proxy
        .get_simple("requests", &[("if-none-match", &etag)])
        .await;
    assert_eq!(html.status(), 200);
    assert_eq!(html.headers()["etag"], "W/\"e1.rw.h\"");
    assert!(html
        .text()
        .await
        .unwrap()
        .contains("<h1>Links for requests</h1>"));

    assert_eq!(
        proxy
            .upstream_hits(&proxy.simple_upstream_path("requests"))
            .await,
        1
    );
}
