//! The crates-flavored [`TestProxy`] handle and its builder, wrapping
//! [`chilled_testkit::TestServer`] with sparse-index helpers.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use chilled_core::registry::RegistryProxy;
use chilled_testkit::{TestServer, TestServerBuilder};
use crates_proxy::CratesProxy;

use super::fixtures::index_rel;

/// Starts the crates router for a configured [`TestServerBuilder`]. The
/// generic knobs (`cooldown`, `override_package`, `dead_upstream`, ...) live on
/// the builder itself.
pub trait StartProxy {
    async fn start_proxy(self) -> TestProxy;
}

impl StartProxy for TestServerBuilder {
    async fn start_proxy(self) -> TestProxy {
        let server = self
            .start(|ctx| {
                let config = crates_proxy::Config::new(
                    crates_proxy::Upstreams {
                        index: ctx.upstream.clone(),
                        download: ctx.upstream.clone(),
                    },
                    ctx.settings.clone(),
                );
                CratesProxy::new(config, reqwest::Client::new()).router()
            })
            .await;
        TestProxy { server }
    }
}

/// A running crates proxy + its mock upstream + temp cache dir.
pub struct TestProxy {
    pub server: TestServer,
}

impl TestProxy {
    /// Entry point: a builder for the crates registry mounted at `/crates`.
    pub fn builder() -> TestServerBuilder {
        TestServerBuilder::new("/crates")
    }

    pub fn mock_upstream(&self) -> &wiremock::MockServer {
        &self.server.mock_upstream
    }

    // Mock upstream mounting.

    /// Mounts a 200 index response for `name` with the given body + validators.
    pub async fn mock_index(&self, name: &str, body: &str, etag: &str, last_modified: &str) {
        self.server
            .mock_get(
                &format!("/{}", index_rel(name)),
                body.as_bytes(),
                &[("etag", etag), ("last-modified", last_modified)],
            )
            .await;
    }

    /// Mounts a 200 index response carrying a non-UTF-8 body.
    pub async fn mock_index_bytes(&self, name: &str, body: Vec<u8>, etag: &str) {
        self.server
            .mock_get(&format!("/{}", index_rel(name)), &body, &[("etag", etag)])
            .await;
    }

    /// Mounts a higher-priority conditional `304` for `name`, matching requests
    /// whose `If-None-Match` equals `etag` (the unmarked upstream validator).
    pub async fn mock_index_304(&self, name: &str, etag: &str) {
        self.server
            .mock_get_304(&format!("/{}", index_rel(name)), etag)
            .await;
    }

    /// Mounts an arbitrary upstream status (e.g. 404) for an index path.
    pub async fn mock_index_status(&self, name: &str, status: u16) {
        self.server
            .mock_get_status(
                &format!("/{}", index_rel(name)),
                status,
                b"upstream says no",
                &[],
            )
            .await;
    }

    /// Mounts a 200 crate-download response with `bytes`.
    pub async fn mock_crate(&self, name: &str, version: &str, bytes: &[u8]) {
        self.server
            .mock_get(
                &format!("/api/v1/crates/{name}/{version}/download"),
                bytes,
                &[],
            )
            .await;
    }

    /// Mounts an arbitrary upstream status for a crate download.
    pub async fn mock_crate_status(&self, name: &str, version: &str, status: u16) {
        self.server
            .mock_get_status(
                &format!("/api/v1/crates/{name}/{version}/download"),
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

    /// The upstream index path for a crate (e.g. `/se/rd/serde`).
    pub fn index_upstream_path(&self, name: &str) -> String {
        format!("/{}", index_rel(name))
    }

    // HTTP drivers (act as cargo). Paths are relative to the `/crates` mount.

    /// `GET /crates/index/<sparse-path>` with optional extra request headers.
    pub async fn get_index(&self, name: &str, headers: &[(&str, &str)]) -> reqwest::Response {
        self.server
            .get(&format!("/index/{}", index_rel(name)), headers)
            .await
    }

    /// `GET /crates/index/config.json`.
    pub async fn get_config_json(&self) -> reqwest::Response {
        self.get("/index/config.json").await
    }

    /// `GET /crates/api/v1/crates/<name>/<version>/download`.
    pub async fn download(&self, name: &str, version: &str) -> reqwest::Response {
        self.get(&format!("/api/v1/crates/{name}/{version}/download"))
            .await
    }

    /// `GET /crates<path>` against the proxy (path begins with `/`).
    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.server.get(path, &[]).await
    }

    // On-disk cache helpers.

    /// Absolute path of the cached index entry file for `name`.
    pub fn index_cache_path(&self, name: &str) -> PathBuf {
        self.server.cache_dir.join("index").join(index_rel(name))
    }

    /// Absolute path of the cached `.crate` file for `name`/`version`.
    pub fn crate_cache_path(&self, name: &str, version: &str) -> PathBuf {
        self.server
            .cache_dir
            .join("crates")
            .join(name)
            .join(format!("{name}-{version}.crate"))
    }

    /// Writes a pristine (unfiltered) index entry straight to the on-disk
    /// cache, with the given mtime — bypassing an upstream fetch.
    pub fn seed_index_file(&self, name: &str, body: &str, mtime: SystemTime) {
        self.seed_index_bytes(name, body.as_bytes(), mtime);
    }

    /// Like [`Self::seed_index_file`] but writes raw bytes.
    pub fn seed_index_bytes(&self, name: &str, body: &[u8], mtime: SystemTime) {
        self.server
            .seed_file(&format!("index/{}", index_rel(name)), body, mtime);
    }
}
