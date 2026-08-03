//! The [`TestServer`] handle: drive the running proxy over HTTP, mount upstream
//! responses, and inspect the on-disk cache.

#[cfg(test)]
mod tests;

use std::fs::{self, File};
use std::path::PathBuf;
use std::time::SystemTime;

use reqwest::header::HeaderName;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path as match_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A running proxy + its mock upstream + temp cache dir.
pub struct TestServer {
    pub mock_upstream: MockServer,
    pub base_url: String,
    pub prefix: String,
    pub cache_dir: PathBuf,
    client: reqwest::Client,
    _tmp: TempDir,
}

impl TestServer {
    /// Assembles a handle from the started proxy's parts. Called by the builder.
    pub(crate) fn new(
        mock_upstream: MockServer,
        base_url: String,
        prefix: String,
        cache_dir: PathBuf,
        client: reqwest::Client,
        tmp: TempDir,
    ) -> Self {
        TestServer {
            mock_upstream,
            base_url,
            prefix,
            cache_dir,
            client,
            _tmp: tmp,
        }
    }

    // Mock upstream mounting.

    /// Mounts a 200 GET response with body bytes and extra headers.
    pub async fn mock_get(&self, path: &str, body: &[u8], headers: &[(&str, &str)]) {
        self.mock_get_status(path, 200, body, headers).await;
    }

    /// Mounts a GET response with an explicit status, body, and headers.
    pub async fn mock_get_status(
        &self,
        path: &str,
        status: u16,
        body: &[u8],
        headers: &[(&str, &str)],
    ) {
        let mut resp = ResponseTemplate::new(status).set_body_bytes(body.to_vec());
        for (k, v) in headers {
            resp = resp.insert_header(*k, *v);
        }
        Mock::given(method("GET"))
            .and(match_path(path.to_string()))
            .respond_with(resp)
            .mount(&self.mock_upstream)
            .await;
    }

    /// Mounts a higher-priority conditional `304` matching `If-None-Match: etag`.
    pub async fn mock_get_304(&self, path: &str, etag: &str) {
        Mock::given(method("GET"))
            .and(match_path(path.to_string()))
            .and(header("if-none-match", etag))
            .respond_with(ResponseTemplate::new(304).insert_header("etag", etag))
            .with_priority(1)
            .mount(&self.mock_upstream)
            .await;
    }

    /// Mounts a HEAD response with the given status and headers.
    pub async fn mock_head(&self, path: &str, status: u16, headers: &[(&str, &str)]) {
        let mut resp = ResponseTemplate::new(status);
        for (k, v) in headers {
            resp = resp.insert_header(*k, *v);
        }
        Mock::given(method("HEAD"))
            .and(match_path(path.to_string()))
            .respond_with(resp)
            .mount(&self.mock_upstream)
            .await;
    }

    // Upstream introspection.

    /// Number of upstream requests received whose path equals `path`.
    pub async fn upstream_hits(&self, path: &str) -> usize {
        self.mock_upstream
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| r.url.path() == path)
            .count()
    }

    /// Total number of upstream requests received.
    pub async fn upstream_total(&self) -> usize {
        self.mock_upstream
            .received_requests()
            .await
            .unwrap_or_default()
            .len()
    }

    // HTTP drivers (act as the package manager).

    /// `GET <prefix><path>` against the proxy, with optional request headers.
    pub async fn get(&self, path: &str, headers: &[(&str, &str)]) -> reqwest::Response {
        let mut req = self
            .client
            .get(format!("{}{}{}", self.base_url, self.prefix, path));
        for (k, v) in headers {
            req = req.header(HeaderName::from_bytes(k.as_bytes()).unwrap(), *v);
        }
        req.send().await.expect("proxy request")
    }

    /// `GET <raw>` against the proxy without the registry prefix.
    pub async fn get_raw(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
            .expect("proxy request")
    }

    // On-disk cache helpers.

    /// Absolute path under the temp cache dir.
    pub fn cache_path(&self, rel: &str) -> PathBuf {
        self.cache_dir.join(rel)
    }

    /// Writes a pristine file straight into the cache with a given mtime,
    /// bypassing an upstream fetch (stale-cache / restrict-downloads setups).
    pub fn seed_file(&self, rel: &str, body: &[u8], mtime: SystemTime) {
        let path = self.cache_path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = File::create(&path).unwrap();
        use std::io::Write;
        f.write_all(body).unwrap();
        f.set_modified(mtime).unwrap();
    }
}
