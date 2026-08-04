//! chilled-proxy compile-time constants.

/// Every registry this binary can serve, in mount order.
pub(crate) const REGISTRY_IDS: [&str; 4] = ["crates", "npm", "pypi", "maven"];

/// Extra mounts served by default, as `(registry, name, path, upstream)`.
///
/// A Gradle build resolves from three Maven-layout repositories, and gating
/// only Central leaves plugins and AndroidX ungated — the two easiest holes to
/// miss — so the other two are mounted out of the box. A `--<registry>-mount`
/// of the same name replaces the entry; `--no-default-mounts` drops them all.
pub(crate) const DEFAULT_MOUNTS: &[(&str, &str, &str, &str)] = &[
    (
        "maven",
        "gradle-plugins",
        "/gradle-plugins",
        "https://plugins.gradle.org/m2/",
    ),
    (
        "maven",
        "google-maven",
        "/google-maven",
        "https://dl.google.com/dl/android/maven2/",
    ),
];

/// Default listen address and port.
pub(crate) const LISTEN_ADDRESS: &str = "0.0.0.0:3080";

/// Default cache directory (each registry gets a subdirectory).
pub(crate) const DEFAULT_CACHE_DIR: &str = "/var/cache/chilled";

/// Default metadata cache entry Time-to-Live in seconds.
pub(crate) const DEFAULT_CACHE_TTL_SECS: u64 = 3600;

/// Program version tag: `"<major>.<minor>.<patch>"`.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// HTTP client User Agent string.
pub(crate) const HTTP_USER_AGENT: &str = concat!("chilled-proxy/", env!("CARGO_PKG_VERSION"));
