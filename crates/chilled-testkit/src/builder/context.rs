//! The resolved configuration handed to a registry crate's router factory.

use std::path::PathBuf;

use chilled_core::config::RegistrySettings;
use url::Url;

/// Everything a registry crate needs to construct its config for a test run.
pub struct TestContext {
    /// The mock upstream's base URL (or a refused port when `dead_upstream`).
    pub upstream: Url,
    /// Root of the temp cache directory.
    pub cache_dir: PathBuf,
    /// Resolved common settings (cooldown, TTL, overrides, proxy URL, ...).
    pub settings: RegistrySettings,
}
