//! Simple-index metadata model (HTTP validators) and its on-disk cache paths.

use std::fmt::{Display, Formatter, Result};
use std::path::{Path, PathBuf};

use chilled_core::cache::fs::{fetch_file, file_mtime, store_file};
use chilled_core::cache::CacheEntry;

/// Cached simple-index entry metadata for one (normalized) project.
#[derive(Clone, Debug)]
pub(crate) struct PypiEntry {
    /// Normalized project name.
    name: String,
    /// Cached response metadata (HTTP validators and freshness).
    pub(crate) meta: CacheEntry,
}

impl Display for PypiEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.write_str(&self.name)
    }
}

impl PypiEntry {
    /// Creates an entry for a normalized project name.
    pub(crate) fn new(name: &str) -> Self {
        PypiEntry {
            name: name.to_owned(),
            meta: CacheEntry::new(),
        }
    }

    /// Gets the project name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Relative cache file path: `{name}.json` (under `<cache_dir>/simple/`).
    pub(crate) fn to_file_path(&self) -> PathBuf {
        PathBuf::from(format!("{}.json", self.name))
    }
}

/// Caches a pristine simple-index body, pinning its mtime to `Last-Modified`.
pub(crate) fn cache_store_simple(dir: &Path, entry: &PypiEntry, data: &[u8]) {
    store_file(&dir.join(entry.to_file_path()), data, entry.meta.mtime());
}

/// Fetches the cached pristine simple-index body, if present.
pub(crate) fn cache_fetch_simple(dir: &Path, name: &str) -> Option<Vec<u8>> {
    fetch_file(&dir.join(PypiEntry::new(name).to_file_path()))
}

/// Recreates missing entry metadata from the cache file's mtime.
pub(crate) fn cache_try_find_simple(dir: &Path, name: &str) -> Option<PypiEntry> {
    let mut entry = PypiEntry::new(name);
    let mtime = file_mtime(&dir.join(entry.to_file_path()))?;
    entry.meta.set_mtime(mtime);
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn cache_path_is_flat_json() {
        assert_eq!(
            PypiEntry::new("foo-bar").to_file_path(),
            PathBuf::from("foo-bar.json")
        );
    }

    #[test]
    fn cache_store_fetch_round_trip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut entry = PypiEntry::new("requests");
        entry.meta.set_mtime(UNIX_EPOCH);
        cache_store_simple(tmp.path(), &entry, b"{}");

        assert_eq!(
            cache_fetch_simple(tmp.path(), "requests"),
            Some(b"{}".to_vec())
        );
        // Metadata recreated from the pinned mtime.
        let found = cache_try_find_simple(tmp.path(), "requests").unwrap();
        assert_eq!(found.meta.mtime(), Some(UNIX_EPOCH));
        assert!(cache_try_find_simple(tmp.path(), "missing").is_none());
    }
}
