//! Bounded in-memory metadata cache (per-registry entry type).

use std::collections::HashMap;
use std::sync::RwLock;

use log::debug;

/// Maximum entries held before the cache is cleared. On overflow the whole map
/// is dropped (cheap, rare) and entries repopulate from upstream.
const METADATA_MAX_ENTRIES: usize = 8192;

/// Bounded, concurrent cache of response metadata (etag / mtime) keyed by
/// package name. Instance state, so a server is cleanly instantiable per test.
pub struct MetadataCache<E: Clone> {
    inner: RwLock<HashMap<String, E>>,
}

impl<E: Clone> Default for MetadataCache<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Clone> MetadataCache<E> {
    /// Creates an empty metadata cache.
    pub fn new() -> Self {
        MetadataCache {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Caches metadata for `name`.
    pub fn store(&self, name: &str, entry: E) {
        let mut map = self.inner.write().unwrap();
        if map.len() >= METADATA_MAX_ENTRIES && !map.contains_key(name) {
            debug!("metadata: cleared metadata cache at capacity");
            map.clear();
        }
        map.insert(name.to_owned(), entry);
    }

    /// Fetches the cached metadata for `name`.
    pub fn fetch(&self, name: &str) -> Option<E> {
        self.inner.read().unwrap().get(name).cloned()
    }

    /// Erases the cached metadata for `name`.
    pub fn invalidate(&self, name: &str) {
        self.inner.write().unwrap().remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_fetch_invalidate_round_trip() {
        let cache: MetadataCache<String> = MetadataCache::new();

        assert_eq!(cache.fetch("serde"), None);
        cache.store("serde", "\"abc\"".to_string());
        assert_eq!(cache.fetch("serde"), Some("\"abc\"".to_string()));
        cache.invalidate("serde");
        assert_eq!(cache.fetch("serde"), None);
    }

    #[test]
    fn store_overwrites() {
        let cache: MetadataCache<u32> = MetadataCache::new();
        cache.store("a", 1);
        cache.store("a", 2);
        assert_eq!(cache.fetch("a"), Some(2));
    }
}
