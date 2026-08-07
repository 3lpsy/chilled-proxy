//! The resolved per-mount upstream auth: headers plus a basic-auth flag.

use reqwest::header::HeaderMap;

/// Headers attached to every upstream request a mount makes.
#[derive(Debug, Clone, Default)]
pub struct UpstreamAuth {
    /// Default headers for the mount's HTTP client. `Authorization` is marked
    /// sensitive so it cannot leak through `Debug`.
    pub(super) headers: HeaderMap,
    /// Whether credentials (as opposed to plain headers) are configured.
    pub(super) basic: bool,
}

impl UpstreamAuth {
    /// Whether anything at all is configured.
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }

    /// The default headers for this mount's HTTP client.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// A short, value-free description for the startup log.
    pub fn describe(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let extra = self.headers.len() - usize::from(self.basic);
        let mut parts = Vec::new();
        if self.basic {
            parts.push("basic auth".to_owned());
        }
        if extra > 0 {
            parts.push(format!("{extra} custom header(s)"));
        }
        Some(parts.join(", "))
    }

    /// Value-free presence report for the management API: custom header names
    /// only, with the synthetic `Authorization` reported as the `basic` flag.
    pub fn summary(&self) -> chilled_wire::AuthSummary {
        let header_names = self
            .headers
            .keys()
            .filter(|name| !(self.basic && *name == axum::http::header::AUTHORIZATION))
            .map(|name| name.as_str().to_owned())
            .collect();
        chilled_wire::AuthSummary {
            basic: self.basic,
            header_names,
        }
    }
}
