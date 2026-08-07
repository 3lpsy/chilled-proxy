//! The parsed form of one `--<registry>-mount` spec.

use std::time::Duration;

use url::Url;

/// One parsed mount spec. `None` in a field means "inherit".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MountSpec {
    /// Instance name: the metrics key and cache subdirectory.
    pub(crate) name: String,
    /// Mount path; defaults to `/<name>`.
    pub(crate) path: Option<String>,
    /// Primary upstream URL.
    pub(crate) upstream: Option<Url>,
    /// The registry's second URL (crates.io `index`, PyPI `files`).
    pub(crate) secondary: Option<Url>,
    /// External URL of this mount.
    pub(crate) proxy_url: Option<Url>,
    /// Age-gating window.
    pub(crate) cooldown: Option<Duration>,
    /// Metadata cache TTL, in seconds.
    pub(crate) cache_ttl: Option<u64>,
    /// Whether to refuse downloads inside the cooldown window.
    pub(crate) restrict_downloads: Option<bool>,
    /// Cap on an upstream metadata document.
    pub(crate) max_metadata_size: Option<usize>,
    /// Cap on an upstream artifact download.
    pub(crate) max_artifact_size: Option<usize>,
    /// Extra hosts this mount's index may serve files from (PyPI only).
    pub(crate) file_hosts: Vec<String>,
}
