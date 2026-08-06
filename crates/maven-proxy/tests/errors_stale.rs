//! Upstream failure handling for metadata: stale-serve from the pristine
//! cache on 5xx rather than failing the build.

mod common;

use common::StartProxy;
use common::{metadata_xml, TestProxy};

#[tokio::test]
async fn upstream_5xx_serves_cached_copy() {
    let proxy = TestProxy::builder()
        .cache_ttl(std::time::Duration::ZERO)
        .start_proxy()
        .await;
    let body = metadata_xml("com.example", "thing", &["1.0.0"], "1.0.0", "1.0.0");
    proxy
        .mock_metadata(
            "com/example",
            "thing",
            &body,
            "\"e1\"",
            "Sun, 06 Nov 1994 08:49:37 GMT",
        )
        .await;
    assert_eq!(
        proxy.get_metadata("com/example", "thing").await.status(),
        200
    );

    // Upstream degrades to 503; the zero TTL forces a refetch, which must
    // fall back to the cached copy instead of forwarding the outage.
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(
            "/com/example/thing/maven-metadata.xml",
        ))
        .respond_with(wiremock::ResponseTemplate::new(503))
        .with_priority(1)
        .mount(&proxy.server.mock_upstream)
        .await;

    let resp = proxy.get_metadata("com/example", "thing").await;
    assert_eq!(resp.status(), 200);
    assert!(resp
        .text()
        .await
        .unwrap()
        .contains("<version>1.0.0</version>"));
}
