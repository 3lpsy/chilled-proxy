//! The [`TestServerBuilder`]: proxy knobs and startup.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use chilled_core::config::RegistrySettings;
use tempfile::TempDir;
use url::Url;
use wiremock::MockServer;

use crate::builder::context::TestContext;
use crate::server::TestServer;

/// Configures and starts a [`TestServer`].
pub struct TestServerBuilder {
    cooldown: Duration,
    cache_ttl: Duration,
    overrides: HashSet<String>,
    restrict_downloads: bool,
    proxy_url: Option<String>,
    dead_upstream: bool,
    prefix: String,
    max_metadata_size: usize,
    max_artifact_size: usize,
}

impl TestServerBuilder {
    /// A builder for a registry mounted at `prefix` (e.g. `/crates`). An empty
    /// prefix serves the router unwrapped (for full-app tests).
    pub fn new(prefix: &str) -> Self {
        TestServerBuilder {
            cooldown: Duration::ZERO,
            cache_ttl: Duration::from_secs(3600),
            overrides: HashSet::new(),
            restrict_downloads: false,
            proxy_url: None,
            dead_upstream: false,
            prefix: prefix.to_string(),
            // Generous by default so size limits only matter to tests that ask
            // for them; the real per-registry defaults live in each crate.
            max_metadata_size: 0x400_0000,
            max_artifact_size: 0x2000_0000,
        }
    }

    pub fn cooldown(mut self, d: Duration) -> Self {
        self.cooldown = d;
        self
    }

    pub fn cooldown_days(self, days: u64) -> Self {
        self.cooldown(Duration::from_secs(days * 86_400))
    }

    pub fn cache_ttl(mut self, d: Duration) -> Self {
        self.cache_ttl = d;
        self
    }

    /// Adds a package to the cooldown-override set (stored lower-cased,
    /// matching the app's normalized lookup).
    pub fn override_package(mut self, name: &str) -> Self {
        self.overrides.insert(name.to_ascii_lowercase());
        self
    }

    pub fn restrict_downloads(mut self) -> Self {
        self.restrict_downloads = true;
        self
    }

    pub fn max_metadata_size(mut self, bytes: usize) -> Self {
        self.max_metadata_size = bytes;
        self
    }

    pub fn max_artifact_size(mut self, bytes: usize) -> Self {
        self.max_artifact_size = bytes;
        self
    }

    pub fn proxy_url(mut self, url: &str) -> Self {
        self.proxy_url = Some(url.to_string());
        self
    }

    /// Points the upstream at a refused port so fetches fail at the transport
    /// layer (for stale-cache / 502 tests).
    pub fn dead_upstream(mut self) -> Self {
        self.dead_upstream = true;
        self
    }

    /// Starts the mock upstream and the proxy, building the registry router
    /// with `make_router`. Returns a driving handle.
    pub async fn start(self, make_router: impl FnOnce(&TestContext) -> Router) -> TestServer {
        let mock_upstream = MockServer::start().await;

        let upstream = if self.dead_upstream {
            // Reserved-but-refused: nothing listens on TCP port 1.
            "http://127.0.0.1:1/".to_string()
        } else {
            format!("{}/", mock_upstream.uri().trim_end_matches('/'))
        };

        let tmp = TempDir::new().expect("create temp cache dir");
        let cache_dir = tmp.path().to_path_buf();

        let proxy_url = self
            .proxy_url
            .unwrap_or_else(|| format!("http://localhost:3080{}/", self.prefix));

        let ctx = TestContext {
            upstream: Url::parse(&upstream).unwrap(),
            cache_dir: cache_dir.clone(),
            settings: RegistrySettings {
                cache_dir: cache_dir.clone(),
                cache_ttl: self.cache_ttl,
                cooldown: self.cooldown,
                overrides: Arc::new(self.overrides),
                restrict_downloads: self.restrict_downloads,
                proxy_url: Url::parse(&proxy_url).unwrap(),
                max_metadata_size: self.max_metadata_size,
                max_artifact_size: self.max_artifact_size,
            },
        };

        let inner = make_router(&ctx);
        let app = if self.prefix.is_empty() {
            inner
        } else {
            Router::new().nest(&self.prefix, inner)
        };

        let (base_url, client) = crate::server::serve_app(app, "/").await;

        TestServer::new(mock_upstream, base_url, self.prefix, cache_dir, client, tmp)
    }
}
