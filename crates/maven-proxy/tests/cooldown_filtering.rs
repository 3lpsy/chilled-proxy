//! The core feature: metadata age-gating via POM `Last-Modified` probes, its
//! ETag rewriting, overrides, and memoization.

mod common;

use common::{http_date_from_now, metadata_xml, TestProxy, OLD, OLD_NEWER, TOO_NEW};

const GROUP: &str = "com/example";
const ARTIFACT: &str = "thing";
const WEEK_SECS: u64 = 7 * 86_400;

/// The marker prefix a 7-day-filtered `etag123` body carries (the trailing
/// cutoff bucket moves with the clock).
fn week_prefix() -> String {
    chilled_testkit::marker_prefix("\"etag123\"", WEEK_SECS)
}

#[tokio::test]
async fn filtering_hides_too_new_version() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0", "2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "2.0.0", TOO_NEW).await;

    let resp = proxy.get_metadata(GROUP, ARTIFACT).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "text/xml");
    // Filtered body: weak, cooldown-tagged ETag and NO Last-Modified.
    assert!(resp.headers()["etag"]
        .to_str()
        .unwrap()
        .starts_with(&week_prefix()));
    assert!(resp.headers().get("last-modified").is_none());

    let text = resp.text().await.unwrap();
    assert!(text.contains("<version>1.0.0</version>"), "old kept");
    assert!(!text.contains("2.0.0"), "too-new hidden everywhere");
}

#[tokio::test]
async fn boundary_keeps_at_cutoff_drops_newer() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    // `kept` sits exactly at the cutoff when crafted (and only ages past it);
    // `dropped` sits two hours inside the window.
    let kept = http_date_from_now(-(WEEK_SECS as i64));
    let dropped = http_date_from_now(-(WEEK_SECS as i64) + 7200);
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0", "2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", &kept).await;
    proxy
        .mock_pom_head(GROUP, ARTIFACT, "2.0.0", &dropped)
        .await;

    let text = proxy
        .get_metadata(GROUP, ARTIFACT)
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains("<version>1.0.0</version>"), "<= cutoff kept");
    assert!(!text.contains("2.0.0"), "> cutoff dropped");
}

#[tokio::test]
async fn latest_and_release_repoint_to_survivors() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(
        GROUP,
        ARTIFACT,
        &["1.0.0", "1.1.0", "2.0.0"],
        "2.0.0",
        "2.0.0",
    );
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;
    proxy
        .mock_pom_head(GROUP, ARTIFACT, "1.1.0", OLD_NEWER)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "2.0.0", TOO_NEW).await;

    let text = proxy
        .get_metadata(GROUP, ARTIFACT)
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains("<latest>1.1.0</latest>"));
    assert!(text.contains("<release>1.1.0</release>"));
    assert!(!text.contains("2.0.0"));
}

#[tokio::test]
async fn all_versions_filtered_yields_404() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(GROUP, ARTIFACT, &["2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "2.0.0", TOO_NEW).await;

    let resp = proxy.get_metadata(GROUP, ARTIFACT).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn cooldown_disabled_serves_verbatim() {
    let proxy = TestProxy::builder().start().await; // cooldown = 0
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0", "2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;

    let resp = proxy.get_metadata(GROUP, ARTIFACT).await;
    assert_eq!(resp.status(), 200);
    // Unfiltered: the strong upstream ETag and Last-Modified pass through.
    assert_eq!(resp.headers()["etag"], "\"etag123\"");
    assert_eq!(resp.headers()["last-modified"], OLD);
    assert_eq!(resp.text().await.unwrap(), body);
    // No POM probes were made.
    assert_eq!(proxy.upstream_total().await, 1);
}

#[tokio::test]
async fn second_request_reuses_memo_and_sidecar() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0", "2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "2.0.0", TOO_NEW).await;

    let first = proxy
        .get_metadata(GROUP, ARTIFACT)
        .await
        .text()
        .await
        .unwrap();
    let second = proxy
        .get_metadata(GROUP, ARTIFACT)
        .await
        .text()
        .await
        .unwrap();
    assert_eq!(first, second);

    // One metadata fetch, one probe per version — nothing re-fetched.
    assert_eq!(
        proxy
            .upstream_hits(&proxy.metadata_path(GROUP, ARTIFACT))
            .await,
        1
    );
    assert_eq!(
        proxy
            .upstream_hits(&proxy.pom_path(GROUP, ARTIFACT, "1.0.0"))
            .await,
        1
    );
    assert_eq!(
        proxy
            .upstream_hits(&proxy.pom_path(GROUP, ARTIFACT, "2.0.0"))
            .await,
        1
    );
}

#[tokio::test]
async fn override_exempts_artifact_from_cooldown() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .override_artifact("com.example:thing")
        .start()
        .await;
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0", "2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;

    let resp = proxy.get_metadata(GROUP, ARTIFACT).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["etag"], "\"etag123\"");
    assert_eq!(resp.text().await.unwrap(), body);
    // Exempt artifacts are never probed.
    assert_eq!(proxy.upstream_total().await, 1);
}

#[tokio::test]
async fn stale_bucket_marker_is_reserved_not_304() {
    // Regression: the marker carries the cutoff bucket, so a client holding a
    // copy filtered at an earlier bucket is re-served — otherwise versions
    // that aged past the cooldown would stay invisible to it indefinitely.
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0"], "1.0.0", "1.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;
    proxy
        .mock_metadata_304(GROUP, ARTIFACT, "\"etag123\"")
        .await;

    let first = proxy.get_metadata(GROUP, ARTIFACT).await;
    let marker = first.headers()["etag"].to_str().unwrap().to_owned();

    let stale = chilled_testkit::shift_marker_bucket(&marker, -1);
    let resp = proxy
        .get_with(
            &proxy.metadata_path(GROUP, ARTIFACT),
            &[("if-none-match", &stale)],
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert!(resp
        .text()
        .await
        .unwrap()
        .contains("<version>1.0.0</version>"));
}

#[tokio::test]
async fn group_level_plugin_metadata_passes_through() {
    // Plugin-prefix metadata has <plugins> but no <versions>; gating it to a
    // 404 would break `mvn <prefix>:<goal>` resolution.
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<metadata><plugins><plugin><name>Thing</name>",
        "<prefix>thing</prefix><artifactId>thing-maven-plugin</artifactId>",
        "</plugin></plugins></metadata>\n"
    );
    proxy
        .mock_file(
            "/org/apache/maven/plugins/maven-metadata.xml",
            body.as_bytes(),
            &[("etag", "\"etagP\""), ("last-modified", OLD)],
        )
        .await;

    let resp = proxy
        .get("/org/apache/maven/plugins/maven-metadata.xml")
        .await;
    assert_eq!(resp.status(), 200);
    assert!(resp
        .text()
        .await
        .unwrap()
        .contains("<prefix>thing</prefix>"));
}

#[tokio::test]
async fn artifact_named_snapshot_is_still_gated() {
    // An artifactId ending in `-SNAPSHOT` must not be mistaken for a snapshot
    // version directory and slip past the cooldown ungated.
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(GROUP, "thing-SNAPSHOT", &["2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, "thing-SNAPSHOT", &body, "\"etagS\"", OLD)
        .await;
    proxy
        .mock_pom_head(GROUP, "thing-SNAPSHOT", "2.0.0", TOO_NEW)
        .await;

    // Gated: every version is within the window, so nothing is served.
    let resp = proxy.get_metadata(GROUP, "thing-SNAPSHOT").await;
    assert_eq!(resp.status(), 404);
}
