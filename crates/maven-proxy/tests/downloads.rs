//! Artifact download path: proxy + cache, error forwarding, the size cap,
//! stale metadata serving, and the snapshot pass-through.

mod common;

use std::time::SystemTime;

use common::{metadata_xml, TestProxy, JAR_BYTES};

const GROUP: &str = "com/example";
const ARTIFACT: &str = "thing";

#[tokio::test]
async fn artifact_proxies_then_caches_byte_exact() {
    let proxy = TestProxy::builder().start().await;
    let path = proxy.file_path(GROUP, ARTIFACT, "1.0.0", "thing-1.0.0.jar");
    proxy.mock_file(&path, JAR_BYTES, &[]).await;

    let resp = proxy.get(&path).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/java-archive");
    assert_eq!(resp.bytes().await.unwrap().as_ref(), JAR_BYTES);

    // Cached on disk; a second request stays local.
    let cache = proxy.file_cache_path(GROUP, ARTIFACT, "1.0.0", "thing-1.0.0.jar");
    assert_eq!(std::fs::read(&cache).unwrap(), JAR_BYTES);
    assert_eq!(proxy.get(&path).await.status(), 200);
    assert_eq!(proxy.upstream_hits(&path).await, 1);
}

#[tokio::test]
async fn pom_is_served_as_xml() {
    let proxy = TestProxy::builder().start().await;
    let path = proxy.file_path(GROUP, ARTIFACT, "1.0.0", "thing-1.0.0.pom");
    proxy.mock_file(&path, b"<project/>", &[]).await;

    let resp = proxy.get(&path).await;
    assert_eq!(resp.headers()["content-type"], "text/xml");
}

#[tokio::test]
async fn upstream_errors_are_forwarded() {
    let proxy = TestProxy::builder().start().await;
    let missing = proxy.file_path(GROUP, ARTIFACT, "9.9.9", "thing-9.9.9.jar");
    proxy.mock_file_status(&missing, 404).await;
    assert_eq!(proxy.get(&missing).await.status(), 404);

    let broken = proxy.file_path(GROUP, ARTIFACT, "8.8.8", "thing-8.8.8.jar");
    proxy.mock_file_status(&broken, 500).await;
    assert_eq!(proxy.get(&broken).await.status(), 500);
}

#[tokio::test]
async fn dead_upstream_artifact_is_502() {
    let proxy = TestProxy::builder().dead_upstream().start().await;
    let path = proxy.file_path(GROUP, ARTIFACT, "1.0.0", "thing-1.0.0.jar");
    assert_eq!(proxy.get(&path).await.status(), 502);
}

#[tokio::test]
async fn dead_upstream_serves_stale_filtered_metadata() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .dead_upstream()
        .start()
        .await;
    // Seed the pristine metadata and a sidecar that gates 2.0.0.
    let body = metadata_xml(GROUP, ARTIFACT, &["1.0.0", "2.0.0"], "2.0.0", "2.0.0");
    proxy.seed_metadata(GROUP, ARTIFACT, &body, SystemTime::now());
    proxy.seed_sidecar(
        GROUP,
        ARTIFACT,
        r#"{"1.0.0":{"ts":946684800,"src":"lm"},"2.0.0":{"ts":32472144000,"src":"lm"}}"#,
    );

    let resp = proxy.get_metadata(GROUP, ARTIFACT).await;
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(text.contains("1.0.0"), "stale cache served");
    assert!(!text.contains("2.0.0"), "stale serve is still filtered");
}

#[tokio::test]
async fn dead_upstream_metadata_without_cache_is_502() {
    let proxy = TestProxy::builder().dead_upstream().start().await;
    assert_eq!(proxy.get_metadata(GROUP, ARTIFACT).await.status(), 502);
}

#[tokio::test]
async fn snapshot_version_dir_metadata_passes_through_ungated() {
    let proxy = TestProxy::builder().cooldown_days(7).start().await;
    let path = format!("/{GROUP}/{ARTIFACT}/1.0-SNAPSHOT/maven-metadata.xml");
    let body = b"<metadata><versioning><snapshot/></versioning></metadata>";
    proxy.mock_file(&path, body, &[]).await;

    let resp = proxy.get(&path).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
    // No POM probes for snapshot metadata.
    assert_eq!(proxy.upstream_total().await, 1);
}

#[tokio::test]
async fn oversized_artifact_is_507() {
    use chilled_core::registry::RegistryProxy;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // A raw upstream declaring a Content-Length one byte over the 512 MiB cap
    // (hyper-based mocks normalize the header away, so speak plain TCP).
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    tokio::spawn(async move {
        let mut open = Vec::new();
        loop {
            let Ok((mut sock, _)) = upstream.accept().await else {
                break;
            };
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 536870913\r\n\r\n")
                .await;
            open.push(sock); // keep the connection open; the body never comes
        }
    });

    let tmp = tempfile::TempDir::new().unwrap();
    let settings = chilled_core::config::RegistrySettings {
        cache_dir: tmp.path().to_path_buf(),
        cache_ttl: std::time::Duration::from_secs(3600),
        cooldown: std::time::Duration::ZERO,
        overrides: std::sync::Arc::new(std::collections::HashSet::new()),
        restrict_downloads: false,
        proxy_url: url::Url::parse("http://localhost:3080/maven/").unwrap(),
    };
    let config = maven_proxy::Config::new(
        url::Url::parse(&format!("http://{upstream_addr}/")).unwrap(),
        settings,
    );
    let router = maven_proxy::MavenProxy::new(config, reqwest::Client::new()).router();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(chilled_core::serve::serve_listener(listener, router));

    let resp = reqwest::get(format!(
        "http://{addr}/com/example/thing/1.0.0/thing-1.0.0.jar"
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 507);
    // Nothing partial was cached.
    assert!(!tmp
        .path()
        .join("repo/com/example/thing/1.0.0/thing-1.0.0.jar")
        .exists());
}
