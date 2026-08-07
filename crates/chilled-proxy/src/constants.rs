//! chilled-proxy compile-time constants.

use crate::kind::RegistryKind;

/// Extra mounts served by default, as `(registry, name, path, upstream)`.
/// Gating only Maven Central leaves Gradle plugins and AndroidX ungated, so
/// those repositories are mounted out of the box. A `--<registry>-mount` of
/// the same name replaces an entry; `--no-default-mounts` drops them all.
pub(crate) const DEFAULT_MOUNTS: &[(RegistryKind, &str, &str, &str)] = &[
    (
        RegistryKind::Maven,
        "gradle-plugins",
        "/gradle-plugins",
        "https://plugins.gradle.org/m2/",
    ),
    (
        RegistryKind::Maven,
        "google-maven",
        "/google-maven",
        "https://dl.google.com/dl/android/maven2/",
    ),
];

/// Default listen address and port.
pub(crate) const LISTEN_ADDRESS: &str = "0.0.0.0:3080";

/// Default cache directory (each registry gets a subdirectory).
pub(crate) const DEFAULT_CACHE_DIR: &str = "/var/cache/chilled";

/// Default UI database path — outside the cache dir so cache wipes keep users.
pub(crate) const DEFAULT_UI_DB_PATH: &str = "/var/lib/chilled/chilled.db";

/// Floor for `--ui-cache-update-interval`: scans stat every cached file.
pub(crate) const MIN_UI_CACHE_UPDATE_INTERVAL_SECS: u64 = 30;

/// Default metadata cache entry Time-to-Live in seconds.
pub(crate) const DEFAULT_CACHE_TTL_SECS: u64 = 3600;

/// Program version tag: `"<major>.<minor>.<patch>"`.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// HTTP client User Agent string.
pub(crate) const HTTP_USER_AGENT: &str = concat!("chilled-proxy/", env!("CARGO_PKG_VERSION"));
