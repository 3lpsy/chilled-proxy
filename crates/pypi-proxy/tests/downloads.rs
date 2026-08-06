//! File download path: proxy + cache, upstream errors, the size cap, and the
//! `--restrict-downloads` age-gate (fail-closed against the pristine index).

mod common;

use std::time::SystemTime;

use common::StartProxy;
use common::{simple_json, TestProxy, FILE_BYTES, OLD, SHA, TOO_NEW};

#[tokio::test]
async fn download_proxies_then_caches() {
    let proxy = TestProxy::builder().start_proxy().await;
    proxy.mock_file("foo-1.0.0.tar.gz", FILE_BYTES).await;

    let resp = proxy.download("foo", "foo-1.0.0.tar.gz").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/octet-stream");
    assert_eq!(resp.bytes().await.unwrap().as_ref(), FILE_BYTES);

    // Bytes cached on disk byte-for-byte.
    let path = proxy.file_cache_path("foo", "foo-1.0.0.tar.gz");
    assert_eq!(std::fs::read(&path).unwrap(), FILE_BYTES);

    // Second download served from disk — upstream hit only once.
    assert_eq!(
        proxy.download("foo", "foo-1.0.0.tar.gz").await.status(),
        200
    );
    assert_eq!(
        proxy
            .upstream_hits(&proxy.file_upstream_path("foo-1.0.0.tar.gz"))
            .await,
        1
    );
}

#[tokio::test]
async fn restrict_gates_against_the_pristine_index() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    // Pristine index on disk carries the upload-times the gate reads.
    let index = simple_json(
        "foo",
        &[
            ("foo-1.0.0.tar.gz", OLD, SHA),
            ("foo-2.0.0.tar.gz", TOO_NEW, SHA),
        ],
    );
    proxy.seed_simple_file("foo", &index, SystemTime::now());
    proxy.mock_file("foo-1.0.0.tar.gz", FILE_BYTES).await;

    // Too-new -> refused before any upstream download.
    assert_eq!(
        proxy.download("foo", "foo-2.0.0.tar.gz").await.status(),
        403
    );
    // Unknown filename -> refused (fail-closed).
    assert_eq!(
        proxy.download("foo", "foo-9.9.9.tar.gz").await.status(),
        403
    );
    assert_eq!(proxy.upstream_total().await, 0);

    // Old enough -> allowed and proxied.
    assert_eq!(
        proxy.download("foo", "foo-1.0.0.tar.gz").await.status(),
        200
    );
}

#[tokio::test]
async fn restrict_without_cached_index_fails_closed() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    proxy.mock_file("foo-1.0.0.tar.gz", FILE_BYTES).await;

    // Nothing cached: the gate fetches the index, upstream has no such
    // project, so the upload time stays unverifiable -> refused.
    assert_eq!(
        proxy.download("foo", "foo-1.0.0.tar.gz").await.status(),
        403
    );
    assert_eq!(proxy.upstream_hits("/foo/").await, 1);
    // The distribution itself was never fetched.
    assert_eq!(
        proxy
            .upstream_hits(&proxy.file_upstream_path("foo-1.0.0.tar.gz"))
            .await,
        0
    );
}

#[tokio::test]
async fn restrict_gate_fetches_the_index_on_demand() {
    // A pinned lockfile install may never request the index; a cold cache must
    // not turn every distribution into a 403.
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    let index = simple_json(
        "foo",
        &[
            ("foo-1.0.0.tar.gz", OLD, SHA),
            ("foo-2.0.0.tar.gz", TOO_NEW, SHA),
        ],
    );
    proxy.mock_simple("foo", &index, "\"e1\"").await;
    proxy.mock_file("foo-1.0.0.tar.gz", FILE_BYTES).await;

    assert_eq!(
        proxy.download("foo", "foo-1.0.0.tar.gz").await.status(),
        200
    );
    assert_eq!(proxy.upstream_hits("/foo/").await, 1);
    // Still fail-closed for a distribution inside the window.
    assert_eq!(
        proxy.download("foo", "foo-2.0.0.tar.gz").await.status(),
        403
    );
}

#[tokio::test]
async fn too_new_is_downloadable_without_restrict() {
    // Cooldown hides new files from the index, but a direct download still
    // works when --restrict-downloads is off.
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    proxy.mock_file("foo-2.0.0.tar.gz", FILE_BYTES).await;

    assert_eq!(
        proxy.download("foo", "foo-2.0.0.tar.gz").await.status(),
        200
    );
}

#[tokio::test]
async fn oversized_content_length_is_507() {
    use chilled_core::registry::RegistryProxy;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // A raw upstream declaring a Content-Length one byte over the default cap
    // (hyper-based mocks normalize the header away, so speak plain TCP). The
    // up-front cap check must refuse with 507 before reading any body.
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    // One byte over whatever this registry's default artifact cap is.
    let over_cap_head = format!(
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n",
        pypi_proxy::DEFAULT_MAX_ARTIFACT_SIZE + 1
    );
    tokio::spawn(async move {
        let mut open = Vec::new();
        loop {
            let Ok((mut sock, _)) = upstream.accept().await else {
                break;
            };
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await;
            let _ = sock.write_all(over_cap_head.as_bytes()).await;
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
        max_metadata_size: pypi_proxy::DEFAULT_MAX_METADATA_SIZE,
        max_artifact_size: pypi_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
        proxy_url: url::Url::parse("http://localhost:3080/pypi/").unwrap(),
    };
    let upstream_url = url::Url::parse(&format!("http://{upstream_addr}/")).unwrap();
    let config = pypi_proxy::Config::new(
        pypi_proxy::Upstreams {
            simple: upstream_url.clone(),
            files: upstream_url,
        },
        settings,
        &[],
    );
    let router = pypi_proxy::PypiProxy::new(config, reqwest::Client::new()).router();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(chilled_core::serve::serve_listener(listener, router));

    let resp = reqwest::get(format!(
        "http://{addr}/files/foo/packages/aa/bb/cc/foo-1.0.0.tar.gz"
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 507);
    // Nothing partial was cached.
    assert!(!tmp
        .path()
        .join("files/foo/packages/aa/bb/cc/foo-1.0.0.tar.gz")
        .exists());
}

#[tokio::test]
async fn download_forwards_upstream_404() {
    let proxy = TestProxy::builder().start_proxy().await;
    proxy.mock_file_status("foo-9.9.9.tar.gz", 404).await;

    assert_eq!(
        proxy.download("foo", "foo-9.9.9.tar.gz").await.status(),
        404
    );
}

#[tokio::test]
async fn download_transport_failure_is_502() {
    let proxy = TestProxy::builder().dead_upstream().start_proxy().await;

    assert_eq!(
        proxy.download("foo", "foo-1.0.0.tar.gz").await.status(),
        502
    );
}

#[tokio::test]
async fn pep658_metadata_sidecar_is_proxied() {
    // pip/uv fetch `<wheel>.metadata` when the index advertises core-metadata;
    // the files route must serve it rather than 404.
    let proxy = TestProxy::builder().start_proxy().await;
    proxy
        .mock_file(
            "foo-1.0.0-py3-none-any.whl.metadata",
            b"Metadata-Version: 2.1",
        )
        .await;

    let resp = proxy
        .download("foo", "foo-1.0.0-py3-none-any.whl.metadata")
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.bytes().await.unwrap().as_ref(),
        b"Metadata-Version: 2.1"
    );
}

#[tokio::test]
async fn metadata_sidecar_ages_with_its_distribution() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start_proxy()
        .await;
    let index = simple_json(
        "foo",
        &[
            ("foo-1.0.0-py3-none-any.whl", OLD, SHA),
            ("foo-2.0.0-py3-none-any.whl", TOO_NEW, SHA),
        ],
    );
    proxy.seed_simple_file("foo", &index, SystemTime::now());
    proxy
        .mock_file("foo-1.0.0-py3-none-any.whl.metadata", b"ok")
        .await;

    // The sidecar of a too-new wheel is refused; the old one is served.
    assert_eq!(
        proxy
            .download("foo", "foo-2.0.0-py3-none-any.whl.metadata")
            .await
            .status(),
        403
    );
    assert_eq!(
        proxy
            .download("foo", "foo-1.0.0-py3-none-any.whl.metadata")
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn same_filename_at_different_paths_does_not_collide() {
    // A multi-host index can carry same-named files at different paths
    // (PyTorch: `whl/cpu/…` vs `whl/cu118/…`); the cache must keep both.
    let proxy = TestProxy::builder().start_proxy().await;
    proxy
        .server
        .mock_get("/whl/cpu/torch-2.0.0.whl", b"cpu-build", &[])
        .await;
    proxy
        .server
        .mock_get("/whl/cu118/torch-2.0.0.whl", b"cuda-build", &[])
        .await;

    let cpu = proxy.get("/files/torch/whl/cpu/torch-2.0.0.whl").await;
    assert_eq!(cpu.bytes().await.unwrap().as_ref(), b"cpu-build");
    let cuda = proxy.get("/files/torch/whl/cu118/torch-2.0.0.whl").await;
    assert_eq!(cuda.bytes().await.unwrap().as_ref(), b"cuda-build");

    // Both served again from cache, each from its own path.
    let cpu = proxy.get("/files/torch/whl/cpu/torch-2.0.0.whl").await;
    assert_eq!(cpu.bytes().await.unwrap().as_ref(), b"cpu-build");
    let cuda = proxy.get("/files/torch/whl/cu118/torch-2.0.0.whl").await;
    assert_eq!(cuda.bytes().await.unwrap().as_ref(), b"cuda-build");
    assert_eq!(proxy.upstream_hits("/whl/cpu/torch-2.0.0.whl").await, 1);
    assert_eq!(proxy.upstream_hits("/whl/cu118/torch-2.0.0.whl").await, 1);
}
