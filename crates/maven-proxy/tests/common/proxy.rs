//! The Maven-flavored [`TestProxy`] handle and its builder, wrapping
//! [`chilled_testkit::TestServer`] with repository-path helpers.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use chilled_core::registry::RegistryProxy;
use chilled_testkit::{TestServer, TestServerBuilder};
use maven_proxy::MavenProxy;

/// Configures and starts a [`TestProxy`] (Maven registry mounted at `/maven`).
pub struct TestProxyBuilder {
    inner: TestServerBuilder,
}

impl TestProxyBuilder {
    pub fn new() -> Self {
        TestProxyBuilder {
            inner: TestServerBuilder::new("/maven"),
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

    /// Adds a `group:artifact` key to the cooldown-override set.
    pub fn override_artifact(mut self, key: &str) -> Self {
        self.inner = self.inner.override_package(key);
        self
    }

    pub fn restrict_downloads(mut self) -> Self {
        self.inner = self.inner.restrict_downloads();
        self
    }

    pub fn dead_upstream(mut self) -> Self {
        self.inner = self.inner.dead_upstream();
        self
    }

    pub async fn start(self) -> TestProxy {
        let server = self
            .inner
            .start(|ctx| {
                let config = maven_proxy::Config::new(ctx.upstream.clone(), ctx.settings.clone());
                MavenProxy::new(config, reqwest::Client::new()).router()
            })
            .await;
        TestProxy { server }
    }
}

/// A running Maven proxy + its mock upstream + temp cache dir.
///
/// Path convention: `group` is slash-form (e.g. `com/example`).
pub struct TestProxy {
    pub server: TestServer,
}

impl TestProxy {
    /// Entry point: configure a proxy via the builder.
    pub fn builder() -> TestProxyBuilder {
        TestProxyBuilder::new()
    }

    // Upstream path helpers.

    /// Upstream path of the artifact-level metadata file.
    pub fn metadata_path(&self, group: &str, artifact: &str) -> String {
        format!("/{group}/{artifact}/maven-metadata.xml")
    }

    /// Upstream path of a version's POM (the age-probe target).
    pub fn pom_path(&self, group: &str, artifact: &str, version: &str) -> String {
        format!("/{group}/{artifact}/{version}/{artifact}-{version}.pom")
    }

    /// Upstream path of an arbitrary artifact file.
    pub fn file_path(&self, group: &str, artifact: &str, version: &str, file: &str) -> String {
        format!("/{group}/{artifact}/{version}/{file}")
    }

    // Mock upstream mounting.

    /// Mounts a 200 metadata response with validators.
    pub async fn mock_metadata(
        &self,
        group: &str,
        artifact: &str,
        body: &str,
        etag: &str,
        last_modified: &str,
    ) {
        self.server
            .mock_get(
                &self.metadata_path(group, artifact),
                body.as_bytes(),
                &[("etag", etag), ("last-modified", last_modified)],
            )
            .await;
    }

    /// Mounts a higher-priority conditional `304` for the metadata path,
    /// matching requests whose `If-None-Match` equals `etag`.
    pub async fn mock_metadata_304(&self, group: &str, artifact: &str, etag: &str) {
        self.server
            .mock_get_304(&self.metadata_path(group, artifact), etag)
            .await;
    }

    /// Mounts an arbitrary upstream status for the metadata path.
    pub async fn mock_metadata_status(&self, group: &str, artifact: &str, status: u16) {
        self.server
            .mock_get_status(
                &self.metadata_path(group, artifact),
                status,
                b"upstream says no",
                &[],
            )
            .await;
    }

    /// Mounts a 200 POM HEAD probe response with a `Last-Modified`.
    pub async fn mock_pom_head(
        &self,
        group: &str,
        artifact: &str,
        version: &str,
        last_modified: &str,
    ) {
        self.server
            .mock_head(
                &self.pom_path(group, artifact, version),
                200,
                &[("last-modified", last_modified)],
            )
            .await;
    }

    /// Mounts a POM HEAD probe response with an arbitrary status (no headers).
    pub async fn mock_pom_head_status(
        &self,
        group: &str,
        artifact: &str,
        version: &str,
        status: u16,
    ) {
        self.server
            .mock_head(&self.pom_path(group, artifact, version), status, &[])
            .await;
    }

    /// Mounts a 200 file response with body bytes and extra headers.
    pub async fn mock_file(&self, path: &str, body: &[u8], headers: &[(&str, &str)]) {
        self.server.mock_get(path, body, headers).await;
    }

    /// Mounts an arbitrary upstream status for a file path.
    pub async fn mock_file_status(&self, path: &str, status: u16) {
        self.server
            .mock_get_status(path, status, b"nope", &[])
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

    // HTTP drivers (act as Maven/Gradle). Paths are relative to `/maven`.

    /// `GET /maven<path>` with optional extra request headers.
    pub async fn get_with(&self, path: &str, headers: &[(&str, &str)]) -> reqwest::Response {
        self.server.get(path, headers).await
    }

    /// `GET /maven<path>`.
    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.server.get(path, &[]).await
    }

    /// `GET` of the artifact-level metadata.
    pub async fn get_metadata(&self, group: &str, artifact: &str) -> reqwest::Response {
        self.get(&self.metadata_path(group, artifact)).await
    }

    // On-disk cache helpers.

    /// Absolute cache path of the pristine metadata file.
    pub fn metadata_cache_path(&self, group: &str, artifact: &str) -> PathBuf {
        self.server
            .cache_path(&format!("repo/{group}/{artifact}/maven-metadata.xml"))
    }

    /// Absolute cache path of the version-age sidecar file.
    pub fn sidecar_path(&self, group: &str, artifact: &str) -> PathBuf {
        self.server
            .cache_path(&format!("repo/{group}/{artifact}/.chilled-versions.json"))
    }

    /// Absolute cache path of an artifact file.
    pub fn file_cache_path(
        &self,
        group: &str,
        artifact: &str,
        version: &str,
        file: &str,
    ) -> PathBuf {
        self.server
            .cache_path(&format!("repo/{group}/{artifact}/{version}/{file}"))
    }

    /// Writes a pristine metadata file straight into the cache with an mtime.
    pub fn seed_metadata(&self, group: &str, artifact: &str, body: &str, mtime: SystemTime) {
        self.server.seed_file(
            &format!("repo/{group}/{artifact}/maven-metadata.xml"),
            body.as_bytes(),
            mtime,
        );
    }

    /// Writes a sidecar file straight into the cache.
    pub fn seed_sidecar(&self, group: &str, artifact: &str, json: &str) {
        self.server.seed_file(
            &format!("repo/{group}/{artifact}/.chilled-versions.json"),
            json.as_bytes(),
            SystemTime::now(),
        );
    }

    /// Parses the sidecar file as JSON (panics if missing/corrupt).
    pub fn read_sidecar(&self, group: &str, artifact: &str) -> serde_json::Value {
        let data = std::fs::read(self.sidecar_path(group, artifact)).expect("sidecar file");
        serde_json::from_slice(&data).expect("sidecar JSON")
    }
}
