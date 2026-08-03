//! Sidecar persistence and the first-seen fail-closed fallback.

mod common;

use common::{metadata_xml, TestProxy, OLD};

const GROUP: &str = "com/example";
const ARTIFACT: &str = "thing";

#[tokio::test]
async fn probe_failure_records_first_seen_and_gates() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0", "2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;
    // The 2.0.0 probe fails (404, no Last-Modified) -> first-seen = now.
    proxy
        .mock_pom_head_status(GROUP, ARTIFACT, "2.0.0", 404)
        .await;

    let text = proxy
        .get_metadata(GROUP, ARTIFACT)
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains("1.0.0"), "probed-old version kept");
    assert!(!text.contains("2.0.0"), "unverifiable version gated");

    // The sidecar recorded both sources.
    let sidecar = proxy.read_sidecar(GROUP, ARTIFACT);
    assert_eq!(sidecar["1.0.0"]["src"], "lm");
    assert_eq!(sidecar["2.0.0"]["src"], "fs");
    assert!(sidecar["2.0.0"]["ts"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn sidecar_is_persisted_and_not_reprobed() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0"], "1.0.0", "1.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;

    assert_eq!(proxy.get_metadata(GROUP, ARTIFACT).await.status(), 200);
    assert!(proxy.sidecar_path(GROUP, ARTIFACT).exists());
    assert_eq!(
        proxy
            .upstream_hits(&proxy.pom_path(GROUP, ARTIFACT, "1.0.0"))
            .await,
        1
    );

    // Second request: version already known — no additional HEAD probes.
    assert_eq!(proxy.get_metadata(GROUP, ARTIFACT).await.status(), 200);
    assert_eq!(
        proxy
            .upstream_hits(&proxy.pom_path(GROUP, ARTIFACT, "1.0.0"))
            .await,
        1
    );
}

#[tokio::test]
async fn seeded_sidecar_skips_probes_entirely() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0", "2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    // Pre-recorded ages: 1.0.0 ancient, 2.0.0 far future (still gated).
    proxy.seed_sidecar(
        GROUP,
        ARTIFACT,
        r#"{"1.0.0":{"ts":946684800,"src":"lm"},"2.0.0":{"ts":32472144000,"src":"lm"}}"#,
    );

    let text = proxy
        .get_metadata(GROUP, ARTIFACT)
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains("1.0.0") && !text.contains("2.0.0"));
    // Only the metadata fetch hit upstream — zero HEAD probes.
    assert_eq!(proxy.upstream_total().await, 1);
}

#[tokio::test]
async fn corrupt_sidecar_is_tolerated_and_reprobed() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0"], "1.0.0", "1.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;
    proxy.seed_sidecar(GROUP, ARTIFACT, "{not json");

    let text = proxy
        .get_metadata(GROUP, ARTIFACT)
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains("1.0.0"));
    // The corrupt file was replaced by a valid probed record.
    assert_eq!(proxy.read_sidecar(GROUP, ARTIFACT)["1.0.0"]["src"], "lm");
}
