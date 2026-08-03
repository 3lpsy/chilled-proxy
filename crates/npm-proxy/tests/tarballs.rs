//! Tarball download path: proxy + cache, upstream errors, the size cap, and
//! the `--restrict-downloads` age-gate.

mod common;

use std::time::SystemTime;

use common::{TestProxy, OLD, TARBALL_BYTES, TOO_NEW};

fn tarball_path(name: &str, file: &str) -> String {
    format!("/{name}/-/{file}")
}

#[tokio::test]
async fn tarball_proxies_then_caches_byte_exact() {
    let proxy = TestProxy::builder().start().await;
    proxy
        .mock_tarball("lodash", "lodash-1.0.0.tgz", TARBALL_BYTES)
        .await;

    let resp = proxy.download_tarball("lodash", "lodash-1.0.0.tgz").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/octet-stream");
    assert_eq!(resp.bytes().await.unwrap().as_ref(), TARBALL_BYTES);

    // Bytes cached on disk byte-for-byte.
    let path = proxy.tarball_cache_path("lodash", "lodash-1.0.0.tgz");
    assert_eq!(std::fs::read(&path).unwrap(), TARBALL_BYTES);

    // Second download served from disk — upstream hit only once.
    let second = proxy.download_tarball("lodash", "lodash-1.0.0.tgz").await;
    assert_eq!(second.status(), 200);
    assert_eq!(second.bytes().await.unwrap().as_ref(), TARBALL_BYTES);
    assert_eq!(
        proxy
            .upstream_hits(&tarball_path("lodash", "lodash-1.0.0.tgz"))
            .await,
        1
    );
}

#[tokio::test]
async fn tarball_forwards_upstream_404() {
    let proxy = TestProxy::builder().start().await;
    proxy
        .mock_tarball_status("lodash", "lodash-9.9.9.tgz", 404)
        .await;

    let resp = proxy.download_tarball("lodash", "lodash-9.9.9.tgz").await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn restrict_blocks_too_new_allows_old() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start()
        .await;
    // Pristine packument on disk carries the publish times the gate reads.
    let body = proxy.packument_body("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)]);
    proxy.seed_packument("lodash", &body, SystemTime::now());
    proxy
        .mock_tarball("lodash", "lodash-1.0.0.tgz", TARBALL_BYTES)
        .await;

    // Too-new -> refused before any upstream download, npm error envelope.
    let refused = proxy.download_tarball("lodash", "lodash-2.0.0.tgz").await;
    assert_eq!(refused.status(), 403);
    let error: serde_json::Value = refused.json().await.unwrap();
    assert_eq!(
        error["error"],
        "cooldown: version not old enough or unverifiable"
    );
    assert_eq!(
        proxy
            .upstream_hits(&tarball_path("lodash", "lodash-2.0.0.tgz"))
            .await,
        0
    );

    // Old enough -> allowed and proxied.
    let allowed = proxy.download_tarball("lodash", "lodash-1.0.0.tgz").await;
    assert_eq!(allowed.status(), 200);

    // Unknown version -> also refused (fail-closed).
    let unknown = proxy.download_tarball("lodash", "lodash-3.0.0.tgz").await;
    assert_eq!(unknown.status(), 403);
}

#[tokio::test]
async fn restrict_is_fail_closed_without_cached_packument() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start()
        .await;

    // Nothing cached: the gate tries to fetch the packument, upstream has no
    // such package, so the publish time stays unverifiable -> refused.
    let resp = proxy.download_tarball("lodash", "lodash-1.0.0.tgz").await;
    assert_eq!(resp.status(), 403);
    assert_eq!(proxy.upstream_hits("/lodash").await, 1);
    // The tarball itself was never fetched.
    assert_eq!(
        proxy
            .upstream_hits(&tarball_path("lodash", "lodash-1.0.0.tgz"))
            .await,
        0
    );
}

#[tokio::test]
async fn restrict_is_noop_when_cooldown_disabled() {
    let proxy = TestProxy::builder().restrict_downloads().start().await; // cooldown = 0
    proxy
        .mock_tarball("lodash", "lodash-2.0.0.tgz", TARBALL_BYTES)
        .await;

    let resp = proxy.download_tarball("lodash", "lodash-2.0.0.tgz").await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn oversized_tarball_is_507() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // A raw upstream declaring a Content-Length one byte over the 256 MiB cap
    // (hyper-based mocks normalize the header away, so speak plain TCP). The
    // up-front cap check must refuse with 507 before reading any body.
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
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 268435457\r\n\r\n")
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
        proxy_url: url::Url::parse("http://localhost:3080/npm/").unwrap(),
    };
    let config = npm_proxy::Config::new(
        url::Url::parse(&format!("http://{upstream_addr}/")).unwrap(),
        settings,
    );
    let router = {
        use chilled_core::registry::RegistryProxy;
        npm_proxy::NpmProxy::new(config, reqwest::Client::new()).router()
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(chilled_core::serve::serve_listener(listener, router));

    let resp = reqwest::get(format!("http://{addr}/big/-/big-1.0.0.tgz"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 507);
    // Nothing partial was cached.
    assert!(!tmp.path().join("tarballs/big/big-1.0.0.tgz").exists());
}

#[tokio::test]
async fn restrict_gate_fetches_the_packument_on_demand() {
    // `npm ci` installs from a lockfile without fetching packuments, so a cold
    // cache must not turn every tarball into a 403.
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .start()
        .await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;
    proxy
        .mock_tarball("lodash", "lodash-1.0.0.tgz", TARBALL_BYTES)
        .await;

    // No packument request has been made yet; the gate makes one itself.
    let resp = proxy.download_tarball("lodash", "lodash-1.0.0.tgz").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(proxy.upstream_hits("/lodash").await, 1);

    // Still fail-closed for a version inside the window.
    proxy
        .mock_tarball("lodash", "lodash-2.0.0.tgz", TARBALL_BYTES)
        .await;
    assert_eq!(
        proxy
            .download_tarball("lodash", "lodash-2.0.0.tgz")
            .await
            .status(),
        403
    );
}

#[tokio::test]
async fn restrict_gate_stays_closed_when_the_packument_is_unavailable() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .restrict_downloads()
        .dead_upstream()
        .start()
        .await;

    // Nothing cached and upstream unreachable -> refuse, never serve.
    assert_eq!(
        proxy
            .download_tarball("lodash", "lodash-1.0.0.tgz")
            .await
            .status(),
        403
    );
}
