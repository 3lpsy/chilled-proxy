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
}

/// A registry's cache statistics, reported at `/metrics`.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Cached artifacts, sorted.
    pub artifacts: Vec<CachedArtifact>,
}

/// A mountable registry proxy.
pub trait RegistryProxy: Send + Sync {
    /// Stable mount identifier (`crates`, `npm`, `pypi`, `maven`).
    fn id(&self) -> &'static str;
    /// The registry's router, with routes relative to its mount prefix.
    fn router(&self) -> Router;
    /// Scans this registry's artifact cache (blocking; call off the runtime).
    fn cache_stats(&self) -> CacheStats;
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
            },
            CachedArtifact {
                name: "a".into(),
                version: "2.0.0".into(),
                cached_at: 9,
            },
            CachedArtifact {
                name: "a".into(),
                version: "1.0.0".into(),
                cached_at: 7,
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
