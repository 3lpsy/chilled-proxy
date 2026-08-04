//! The npm-flavored [`TestProxy`] handle and its builder, wrapping
//! [`chilled_testkit::TestServer`] with packument/tarball helpers.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use chilled_core::registry::RegistryProxy;
use chilled_testkit::{TestServer, TestServerBuilder};
use npm_proxy::NpmProxy;

use super::fixtures::{packument, ETAG};

/// Configures and starts a [`TestProxy`] (npm registry mounted at `/npm`).
pub struct TestProxyBuilder {
    inner: TestServerBuilder,
}

impl TestProxyBuilder {
    pub fn new() -> Self {
        TestProxyBuilder {
            inner: TestServerBuilder::new("/npm"),
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

    pub async fn start(self) -> TestProxy {
        let server = self
            .inner
            .start(|ctx| {
                let config = npm_proxy::Config::new(ctx.upstream.clone(), ctx.settings.clone());
                NpmProxy::new(config, reqwest::Client::new()).router()
            })
            .await;
        TestProxy { server }
    }
}

/// A running npm proxy + its mock upstream + temp cache dir.
pub struct TestProxy {
    pub server: TestServer,
}

impl TestProxy {
    /// Entry point: configure a proxy via the builder.
    pub fn builder() -> TestProxyBuilder {
        TestProxyBuilder::new()
    }

    /// The mock upstream base URL, with a trailing slash.
    pub fn upstream_url(&self) -> String {
        format!("{}/", self.server.mock_upstream.uri().trim_end_matches('/'))
    }

    /// Builds a packument body whose tarball URLs point at the mock upstream.
    pub fn packument_body(&self, name: &str, versions: &[(&str, &str)]) -> String {
        packument(name, versions, &self.upstream_url())
    }

    // Mock upstream mounting.

    /// Mounts a 200 packument for `name` (default ETag), returning the body.
    pub async fn mock_packument(&self, name: &str, versions: &[(&str, &str)]) -> String {
        let body = self.packument_body(name, versions);
        self.mock_packument_body(name, &body).await;
        body
    }

    /// Mounts a 200 packument response with an explicit body.
    pub async fn mock_packument_body(&self, name: &str, body: &str) {
        self.server
            .mock_get(&format!("/{name}"), body.as_bytes(), &[("etag", ETAG)])
            .await;
    }

    /// Mounts a higher-priority conditional `304` for `name`, matching requests
    /// whose `If-None-Match` equals `etag` (the unmarked upstream validator).
    pub async fn mock_packument_304(&self, name: &str, etag: &str) {
        self.server.mock_get_304(&format!("/{name}"), etag).await;
    }

    /// Mounts an arbitrary upstream status (e.g. 404) for a packument path.
    pub async fn mock_packument_status(&self, name: &str, status: u16) {
        self.server
            .mock_get_status(&format!("/{name}"), status, b"upstream says no", &[])
            .await;
    }

    /// Mounts a 200 tarball response with `bytes`.
    pub async fn mock_tarball(&self, name: &str, file: &str, bytes: &[u8]) {
        self.server
            .mock_get(&format!("/{name}/-/{file}"), bytes, &[])
            .await;
    }

    /// Mounts an arbitrary upstream status for a tarball download.
    pub async fn mock_tarball_status(&self, name: &str, file: &str, status: u16) {
        self.server
            .mock_get_status(&format!("/{name}/-/{file}"), status, b"nope", &[])
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

    /// The upstream packument path for a package (e.g. `/lodash`).
    pub fn packument_upstream_path(&self, name: &str) -> String {
        format!("/{name}")
    }

    // HTTP drivers (act as the npm client). Paths are relative to `/npm`.

    /// `GET /npm/{name}` with optional extra request headers.
    pub async fn get_packument(&self, name: &str, headers: &[(&str, &str)]) -> reqwest::Response {
        self.server.get(&format!("/{name}"), headers).await
    }

    /// `GET /npm/{name}/{version}`.
    pub async fn get_version(&self, name: &str, version: &str) -> reqwest::Response {
        self.server.get(&format!("/{name}/{version}"), &[]).await
    }

    /// `GET /npm/{name}/-/{file}`.
    pub async fn download_tarball(&self, name: &str, file: &str) -> reqwest::Response {
        self.server.get(&format!("/{name}/-/{file}"), &[]).await
    }

    /// `GET /npm{path}` against the proxy (path begins with `/`).
    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.server.get(path, &[]).await
    }

    // On-disk cache helpers.

    /// Absolute path of the cached pristine packument for `name`.
    pub fn packument_cache_path(&self, name: &str) -> PathBuf {
        self.server.cache_dir.join("packuments").join(name)
    }

    /// Absolute path of the cached tarball for `name`/`file`.
    pub fn tarball_cache_path(&self, name: &str, file: &str) -> PathBuf {
        self.server.cache_dir.join("tarballs").join(name).join(file)
    }

    /// Writes a pristine (unfiltered) packument straight to the on-disk
    /// cache, with the given mtime — bypassing an upstream fetch.
    pub fn seed_packument(&self, name: &str, body: &str, mtime: SystemTime) {
        self.server
            .seed_file(&format!("packuments/{name}"), body.as_bytes(), mtime);
    }
}
