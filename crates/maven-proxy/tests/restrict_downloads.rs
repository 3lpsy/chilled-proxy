//! `--restrict-downloads`: the fail-closed artifact gate with on-demand
//! probing (Maven fetches pinned artifacts without reading metadata first).

mod common;

use common::StartProxy;
use common::{TestProxy, JAR_BYTES, OLD, TOO_NEW};

const GROUP: &str = "com/example";
const ARTIFACT: &str = "thing";

#[tokio::test]
async fn old_version_is_served() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    let path = proxy.file_path(GROUP, ARTIFACT, "1.0.0", "thing-1.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;

    let resp = proxy.get(&path).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), JAR_BYTES);
    // The gate probed the POM on demand.
    assert_eq!(
        proxy
            .upstream_hits(&proxy.pom_path(GROUP, ARTIFACT, "1.0.0"))
            .await,
        1
    );
}

#[tokio::test]
async fn too_new_version_is_403() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    let path = proxy.file_path(GROUP, ARTIFACT, "2.0.0", "thing-2.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "2.0.0", TOO_NEW).await;

    assert_eq!(proxy.get(&path).await.status(), 403);
    // The artifact itself was never fetched.
    assert_eq!(proxy.upstream_hits(&path).await, 0);
}

#[tokio::test]
async fn absent_version_is_404_not_403() {
    // A mount serves one repository, and a multi-repository build asks each of
    // them for artifacts only another one carries. Upstream answering 404 is a
    // definite "not here", not an undatable version: reporting it as gated
    // sends the user hunting a cooldown problem that does not exist, and
    // records a first-seen stamp for a version that is not there.
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    let path = proxy.file_path(GROUP, ARTIFACT, "4.0.0", "thing-4.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;
    proxy
        .mock_pom_head_status(GROUP, ARTIFACT, "4.0.0", 404)
        .await;

    assert_eq!(proxy.get(&path).await.status(), 404);
    // No sidecar was written for a version upstream does not have.
    assert!(!proxy.sidecar_path(GROUP, ARTIFACT).exists());
    // And the artifact itself was never fetched.
    assert_eq!(proxy.upstream_hits(&path).await, 0);
}

#[tokio::test]
async fn probe_failure_is_403_fail_closed() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    let path = proxy.file_path(GROUP, ARTIFACT, "3.0.0", "thing-3.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;
    // 500, not 404: an upstream that failed to answer, which leaves the version
    // undatable and so gated.
    proxy
        .mock_pom_head_status(GROUP, ARTIFACT, "3.0.0", 500)
        .await;

    assert_eq!(proxy.get(&path).await.status(), 403);
    // A first-seen guess was recorded, and the version stays gated...
    assert_eq!(proxy.read_sidecar(GROUP, ARTIFACT)["3.0.0"]["src"], "fs");
    assert_eq!(proxy.get(&path).await.status(), 403);
    // ...but the guess is retried while it gates, so a transient probe failure
    // stops hiding an old artifact as soon as upstream answers again.
    assert_eq!(
        proxy
            .upstream_hits(&proxy.pom_path(GROUP, ARTIFACT, "3.0.0"))
            .await,
        2
    );
}

#[tokio::test]
async fn recovered_probe_replaces_the_first_seen_guess() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    let path = proxy.file_path(GROUP, ARTIFACT, "1.0.0", "thing-1.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;
    // The proxy saw this version while the probe was failing.
    proxy.seed_sidecar(
        GROUP,
        ARTIFACT,
        r#"{"1.0.0":{"ts":32472144000,"src":"fs"}}"#,
    );
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;

    // Upstream answers now: the real age wins and the download is allowed.
    assert_eq!(proxy.get(&path).await.status(), 200);
    let sidecar = proxy.read_sidecar(GROUP, ARTIFACT);
    assert_eq!(sidecar["1.0.0"]["src"], "lm");
}

#[tokio::test]
async fn known_sidecar_version_skips_the_probe() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    let path = proxy.file_path(GROUP, ARTIFACT, "1.0.0", "thing-1.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;
    proxy.seed_sidecar(GROUP, ARTIFACT, r#"{"1.0.0":{"ts":946684800,"src":"lm"}}"#);

    assert_eq!(proxy.get(&path).await.status(), 200);
    // Only the artifact fetch reached upstream — no HEAD probe.
    assert_eq!(proxy.upstream_total().await, 1);
}

#[tokio::test]
async fn gate_is_off_without_cooldown_or_flag() {
    // restrict flag without a cooldown window: nothing to enforce.
    let proxy = TestProxy::builder()
        .restrict_downloads()
        .start_proxy()
        .await;
    let path = proxy.file_path(GROUP, ARTIFACT, "2.0.0", "thing-2.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;
    assert_eq!(proxy.get(&path).await.status(), 200);

    // Cooldown without the restrict flag: downloads are ungated.
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    let path = proxy.file_path(GROUP, ARTIFACT, "2.0.0", "thing-2.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;
    assert_eq!(proxy.get(&path).await.status(), 200);
    assert_eq!(proxy.upstream_total().await, 1);
}

#[tokio::test]
async fn override_exempts_artifact_from_the_gate() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .override_package("com.example:thing")
        .start_proxy()
        .await;
    let path = proxy.file_path(GROUP, ARTIFACT, "2.0.0", "thing-2.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;

    assert_eq!(proxy.get(&path).await.status(), 200);
    // Exempt artifacts are never probed.
    assert_eq!(proxy.upstream_total().await, 1);
}
