//! Checksum coherence: generated hashes must match the *served* (filtered)
//! metadata bytes; artifact checksums pass through verbatim.

mod common;

use common::StartProxy;
use common::{metadata_xml, TestProxy, JAR_BYTES, OLD, TOO_NEW};
use sha1::Digest;

const GROUP: &str = "com/example";
const ARTIFACT: &str = "thing";

#[tokio::test]
async fn generated_checksums_match_served_filtered_body() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0", "2.0.0"], "2.0.0", "2.0.0");
    proxy
        .mock_metadata(GROUP, ARTIFACT, &body, "\"etag123\"", OLD)
        .await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "2.0.0", TOO_NEW).await;

    let served = proxy
        .get_metadata(GROUP, ARTIFACT)
        .await
        .bytes()
        .await
        .unwrap();
    assert!(!served.windows(5).any(|w| w == b"2.0.0"), "filtered body");

    // sha1 / md5 / sha256 of exactly the bytes the .xml route served.
    let sha1_resp = proxy
        .get(&format!("/{GROUP}/{ARTIFACT}/maven-metadata.xml.sha1"))
        .await;
    assert_eq!(
        sha1_resp.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        sha1_resp.text().await.unwrap(),
        format!("{:x}", sha1::Sha1::digest(&served))
    );

    let md5_resp = proxy
        .get(&format!("/{GROUP}/{ARTIFACT}/maven-metadata.xml.md5"))
        .await;
    assert_eq!(
        md5_resp.text().await.unwrap(),
        format!("{:x}", md5::Md5::digest(&served))
    );

    let sha256_resp = proxy
        .get(&format!("/{GROUP}/{ARTIFACT}/maven-metadata.xml.sha256"))
        .await;
    assert_eq!(
        sha256_resp.text().await.unwrap(),
        format!("{:x}", sha2::Sha256::digest(&served))
    );
}

#[tokio::test]
async fn metadata_checksum_passes_through_when_cooldown_off() {
    let proxy = TestProxy::builder().start_proxy().await; // cooldown = 0
    proxy
        .mock_file(
            &format!("/{GROUP}/{ARTIFACT}/maven-metadata.xml.sha1"),
            b"cafebabe-upstream-hash",
            &[],
        )
        .await;

    let resp = proxy
        .get(&format!("/{GROUP}/{ARTIFACT}/maven-metadata.xml.sha1"))
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.bytes().await.unwrap().as_ref(),
        b"cafebabe-upstream-hash"
    );
}

#[tokio::test]
async fn artifact_checksum_passes_through_verbatim() {
    // Artifact bytes are never modified, so their upstream checksums stay valid
    // and are proxied as-is — even with cooldown active.
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    let path = proxy.file_path(GROUP, ARTIFACT, "1.0.0", "thing-1.0.0.jar.sha1");
    proxy.mock_file(&path, b"upstream-jar-sha1", &[]).await;
    proxy.mock_pom_head(GROUP, ARTIFACT, "1.0.0", OLD).await;

    let resp = proxy.get(&path).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "text/plain; charset=utf-8");
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"upstream-jar-sha1");

    // Sanity: the jar itself also round-trips.
    let jar = proxy.file_path(GROUP, ARTIFACT, "1.0.0", "thing-1.0.0.jar");
    proxy.mock_file(&jar, JAR_BYTES, &[]).await;
    assert_eq!(
        proxy.get(&jar).await.bytes().await.unwrap().as_ref(),
        JAR_BYTES
    );
}
