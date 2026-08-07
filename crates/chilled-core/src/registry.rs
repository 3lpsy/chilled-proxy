//! The plug-in seam between the top-level server and each registry proxy.

use axum::Router;

/// One cached artifact reported by a registry's cache scan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CachedArtifact {
    /// Package name (registry-native form).
    pub name: String,
    /// Version, or the cached file name where versions don't apply.
    pub version: String,
    /// Cache file mtime as unix seconds.
    pub cached_at: u64,
    /// Total size on disk in bytes (all files belonging to this version).
    pub size_bytes: u64,
}

/// A registry's cache statistics, reported at `/metrics`.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Cached artifacts, sorted.
    pub artifacts: Vec<CachedArtifact>,
    /// True when the scan could not read the cache (I/O error, not merely a
    /// missing directory) — consumers must not treat the result as "empty".
    pub incomplete: bool,
}

/// A mountable registry proxy. No identifier lives here: a registry can be
/// mounted more than once, so the mount name belongs to the mount (the
/// binary's `MountedRegistry`), not the proxy.
pub trait RegistryProxy: Send + Sync {
    /// The registry's router, with routes relative to its mount prefix.
    fn router(&self) -> Router;
    /// Scans this registry's artifact cache (blocking; call off the runtime).
    fn cache_stats(&self) -> CacheStats;
    /// Deletes one artifact's cached files (blocking). Returns the
    /// mount-relative request paths that would re-fetch what was deleted.
    fn purge_artifact(&self, name: &str, version: &str) -> Vec<String>;
    /// Deletes every cached artifact file, keeping metadata and index caches
    /// warm (blocking; call off the runtime).
    fn purge_all(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_artifacts_sort_by_name_then_version() {
        let mut v = [
            CachedArtifact {
                name: "b".into(),
                version: "1.0.0".into(),
                cached_at: 5,
                size_bytes: 1,
            },
            CachedArtifact {
                name: "a".into(),
                version: "2.0.0".into(),
                cached_at: 9,
                size_bytes: 2,
            },
            CachedArtifact {
                name: "a".into(),
                version: "1.0.0".into(),
                cached_at: 7,
                size_bytes: 3,
            },
        ];
        v.sort();
        let order: Vec<_> = v
            .iter()
            .map(|a| format!("{}-{}", a.name, a.version))
            .collect();
        assert_eq!(order, ["a-1.0.0", "a-2.0.0", "b-1.0.0"]);
    }
}
