//! The PyPI-flavored [`TestProxy`] handle and its builder, wrapping
//! [`chilled_testkit::TestServer`] with simple-index and files helpers.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use chilled_core::registry::RegistryProxy;
use chilled_testkit::{TestServer, TestServerBuilder};
use pypi_proxy::PypiProxy;

use super::fixtures::SIMPLE_CTYPE;

/// Configures and starts a [`TestProxy`] (PyPI registry mounted at `/pypi`).
pub struct TestProxyBuilder {
    inner: TestServerBuilder,
    file_hosts: Vec<String>,
}

impl TestProxyBuilder {
    pub fn new() -> Self {
        TestProxyBuilder {
            inner: TestServerBuilder::new("/pypi"),
            file_hosts: Vec::new(),
        }
    }

    pub fn cooldown(mut self, d: Duration) -> Self {
        self.inner = self.inner.cooldown(d);
        self
    }

    pub fn cooldown_days(mut self, days: u64) -> Self {
        self.inner = self.inner.cooldown_days(days);
        self
    }

    pub fn cache_ttl(mut self, d: Duration) -> Self {
        self.inner = self.inner.cache_ttl(d);
        self
    }

    pub fn override_package(mut self, name: &str) -> Self {
        self.inner = self.inner.override_package(name);
        self
    }

    pub fn restrict_downloads(mut self) -> Self {
        self.inner = self.inner.restrict_downloads();
        self
    }

    pub fn proxy_url(mut self, url: &str) -> Self {
        self.inner = self.inner.proxy_url(url);
        self
    }

    pub fn dead_upstream(mut self) -> Self {
        self.inner = self.inner.dead_upstream();
        self
    }

    pub fn max_metadata_size(mut self, bytes: usize) -> Self {
        self.inner = self.inner.max_metadata_size(bytes);
        self
    }

    pub fn max_artifact_size(mut self, bytes: usize) -> Self {
        self.inner = self.inner.max_artifact_size(bytes);
        self
    }

    /// Extra hosts the mount may fetch distribution files from.
    pub fn file_hosts(mut self, hosts: &[&str]) -> Self {
        self.file_hosts = hosts.iter().map(|h| (*h).to_string()).collect();
        self
    }

    pub async fn start(self) -> TestProxy {
        let file_hosts = self.file_hosts.clone();
        let server = self
            .inner
            .start(move |ctx| {
                // Both the simple index and the files host point at the mock.
                let config = pypi_proxy::Config::with_file_hosts(
                    ctx.upstream.clone(),
                    ctx.upstream.clone(),
                    ctx.settings.clone(),
                    &file_hosts,
                );
                PypiProxy::new(config, reqwest::Client::new()).router()
            })
            .await;
        let raw_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");
        TestProxy { server, raw_client }
    }
}

/// A running PyPI proxy + its mock upstream + temp cache dir.
pub struct TestProxy {
    pub server: TestServer,
    /// Client that does not follow redirects (for 301 assertions).
    raw_client: reqwest::Client,
}

impl TestProxy {
    /// Entry point: configure a proxy via the builder.
    pub fn builder() -> TestProxyBuilder {
        TestProxyBuilder::new()
    }

    // Mock upstream mounting.

    /// Mounts a 200 PEP 691 JSON simple-index response for `name`.
    pub async fn mock_simple(&self, name: &str, body: &str, etag: &str) {
        self.server
            .mock_get(
                &format!("/{name}/"),
                body.as_bytes(),
                &[("etag", etag), ("content-type", SIMPLE_CTYPE)],
            )
            .await;
    }

    /// Mounts a 200 simple-index response with an arbitrary content type.
    pub async fn mock_simple_ctype(&self, name: &str, body: &str, etag: &str, ctype: &str) {
        self.server
            .mock_get(
                &format!("/{name}/"),
                body.as_bytes(),
                &[("etag", etag), ("content-type", ctype)],
            )
            .await;
    }

    /// Mounts a higher-priority conditional `304` for `name`, matching requests
    /// whose `If-None-Match` equals `etag` (the unmarked upstream validator).
    pub async fn mock_simple_304(&self, name: &str, etag: &str) {
        self.server.mock_get_304(&format!("/{name}/"), etag).await;
    }

    /// Mounts an arbitrary upstream status (e.g. 404) for a simple-index path.
    pub async fn mock_simple_status(&self, name: &str, status: u16) {
        self.server
            .mock_get_status(&format!("/{name}/"), status, b"upstream says no", &[])
            .await;
    }

    /// Mounts a 200 file response at the default fixture path for `filename`.
    pub async fn mock_file(&self, filename: &str, bytes: &[u8]) {
        self.server
            .mock_get(&format!("/packages/aa/bb/cc/{filename}"), bytes, &[])
            .await;
    }

    /// Mounts an arbitrary upstream status for a file path.
    pub async fn mock_file_status(&self, filename: &str, status: u16) {
        self.server
            .mock_get_status(
                &format!("/packages/aa/bb/cc/{filename}"),
                status,
                b"nope",
                &[],
            )
            .await;
    }

    // Upstream introspection.

    /// Number of upstream requests received whose path equals `path`.
    pub async fn upstream_hits(&self, path: &str) -> usize {
        self.server.upstream_hits(path).await
    }

    /// Total number of upstream requests received.
    pub async fn upstream_total(&self) -> usize {
        self.server.upstream_total().await
    }

    /// The upstream simple-index path for a project (e.g. `/requests/`).
    pub fn simple_upstream_path(&self, name: &str) -> String {
        format!("/{name}/")
    }

    /// The upstream file path for a fixture filename.
    pub fn file_upstream_path(&self, filename: &str) -> String {
        format!("/packages/aa/bb/cc/{filename}")
    }

    // HTTP drivers (act as pip). Paths are relative to the `/pypi` mount.

    /// `GET /pypi/simple/{name}/` with optional extra request headers.
    pub async fn get_simple(&self, name: &str, headers: &[(&str, &str)]) -> reqwest::Response {
        self.server.get(&format!("/simple/{name}/"), headers).await
    }

    /// `GET /pypi/files/{project}/{tail}` (default fixture tail for `filename`).
    pub async fn download(&self, project: &str, filename: &str) -> reqwest::Response {
        self.get(&format!("/files/{project}/packages/aa/bb/cc/{filename}"))
            .await
    }

    /// `GET /pypi<path>` against the proxy (path begins with `/`).
    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.server.get(path, &[]).await
    }

    /// `GET /pypi<path>` without following redirects (for 301 assertions).
    pub async fn get_no_redirect(&self, path: &str) -> reqwest::Response {
        self.raw_client
            .get(format!("{}/pypi{}", self.server.base_url, path))
            .send()
            .await
            .expect("proxy request")
    }

    // On-disk cache helpers.

    /// Absolute path of the cached pristine simple index for `name`.
    pub fn simple_cache_path(&self, name: &str) -> PathBuf {
        self.server
            .cache_dir
            .join("simple")
            .join(format!("{name}.json"))
    }

    /// Absolute path of the cached file for `project`/`filename`.
    pub fn file_cache_path(&self, project: &str, filename: &str) -> PathBuf {
        self.server
            .cache_dir
            .join("files")
            .join(project)
            .join(filename)
    }

    /// Writes a pristine (unfiltered) simple index straight to the on-disk
    /// cache, with the given mtime — bypassing an upstream fetch.
    pub fn seed_simple_file(&self, name: &str, body: &str, mtime: SystemTime) {
        self.server
            .seed_file(&format!("simple/{name}.json"), body.as_bytes(), mtime);
    }
}
