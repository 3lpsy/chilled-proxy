//! The pre-redacted per-mount configuration projection, built by the binary;
//! secrets are masked there, so credential values never reach the API.

use chilled_wire::AuthSummary;

/// Top-level server facts the config view reports.
#[derive(Debug, Clone)]
pub struct ServerView {
    pub listen: String,
    pub log_level: String,
    pub metrics_enabled: bool,
    /// Registry kinds disabled at their default mounts.
    pub disabled: Vec<String>,
}

/// One mount's API-safe configuration.
#[derive(Debug, Clone)]
pub struct MountView {
    pub name: String,
    /// Registry kind id: `crates`, `npm`, `pypi`, or `maven`.
    pub kind: String,
    pub path: String,
    /// Upstream URL, already redacted.
    pub upstream: String,
    /// Secondary upstream (sparse index / file host), already redacted.
    pub secondary: Option<String>,
    pub proxy_url: String,
    pub cooldown_secs: u64,
    pub cache_ttl_secs: u64,
    pub restrict_downloads: bool,
    pub auth: AuthSummary,
}
