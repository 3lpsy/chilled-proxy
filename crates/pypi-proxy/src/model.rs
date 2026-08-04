//! Simple-index metadata model (HTTP validators) and its on-disk cache paths.

use std::fmt::{Display, Formatter, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use chilled_core::cache::fs::{fetch_file, file_mtime, store_file};
use chilled_core::http::{fmt_http_date, parse_http_date};

/// Cached simple-index entry metadata for one (normalized) project.
#[derive(Clone, Debug)]
pub(crate) struct PypiEntry {
    /// Normalized project name.
    name: String,
    /// HTTP entity tag header.
    etag: Option<String>,
    /// Index file modification time.
    mtime: Option<SystemTime>,
    /// Last upstream update check time.
    atime: Option<Instant>,
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
            etag: None,
            mtime: None,
            atime: None,
        }
    }

    /// Gets the project name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Whether this entry describes the same content as `other`.
    pub(crate) fn is_equivalent(&self, other: &PypiEntry) -> bool {
        (self.etag().is_some() && (self.etag() == other.etag()))
            || (self.last_modified().is_some() && (self.last_modified() == other.last_modified()))
    }

    /// Whether this entry is expired for the given TTL.
    pub(crate) fn is_expired_with_ttl(&self, ttl: &Duration) -> bool {
        self.atime.is_some_and(|atime| atime.elapsed() > *ttl)
    }

    /// Gets the HTTP entity tag metadata.
    pub(crate) fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// Gets the HTTP `Last-Modified` metadata.
    pub(crate) fn last_modified(&self) -> Option<String> {
        self.mtime.map(fmt_http_date)
    }

    /// Gets the file modification time metadata.
    pub(crate) fn mtime(&self) -> Option<SystemTime> {
        self.mtime
    }

    /// Sets the HTTP entity tag metadata.
    pub(crate) fn set_etag(&mut self, etag: &str) {
        self.etag = Some(etag.to_owned());
    }

    /// Sets the HTTP `Last-Modified` metadata.
    pub(crate) fn set_last_modified(&mut self, last_modified: &str) {
        self.mtime = parse_http_date(last_modified);
    }

    /// Sets the file modification time metadata.
    pub(crate) fn set_mtime(&mut self, mtime: SystemTime) {
        self.mtime = Some(mtime);
    }

    /// Updates the last upstream access time metadata.
    pub(crate) fn set_last_updated(&mut self) {
        self.atime = Some(Instant::now());
    }

    /// Relative cache file path: `{name}.json` (under `<cache_dir>/simple/`).
    pub(crate) fn to_file_path(&self) -> PathBuf {
        PathBuf::from(format!("{}.json", self.name))
    }
}

/// Caches a pristine simple-index body, pinning its mtime to `Last-Modified`.
pub(crate) fn cache_store_simple(dir: &Path, entry: &PypiEntry, data: &[u8]) {
    store_file(&dir.join(entry.to_file_path()), data, entry.mtime());
}

/// Fetches the cached pristine simple-index body, if present.
pub(crate) fn cache_fetch_simple(dir: &Path, name: &str) -> Option<Vec<u8>> {
    fetch_file(&dir.join(PypiEntry::new(name).to_file_path()))
}

/// Recreates missing entry metadata from the cache file's mtime.
pub(crate) fn cache_try_find_simple(dir: &Path, name: &str) -> Option<PypiEntry> {
    let mut entry = PypiEntry::new(name);
    let mtime = file_mtime(&dir.join(entry.to_file_path()))?;
    entry.set_mtime(mtime);
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn equivalence_prefers_etag_then_last_modified() {
        let mut a = PypiEntry::new("requests");
        let mut b = PypiEntry::new("requests");
        assert!(!a.is_equivalent(&b));

        a.set_etag("\"e1\"");
        b.set_etag("\"e1\"");
        assert!(a.is_equivalent(&b));

        b.set_etag("\"e2\"");
        assert!(!a.is_equivalent(&b));

        let mut c = PypiEntry::new("requests");
        let mut d = PypiEntry::new("requests");
        c.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
        d.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
        assert!(c.is_equivalent(&d));
    }

    #[test]
    fn last_modified_round_trips() {
        let mut e = PypiEntry::new("requests");
        e.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(
            e.last_modified().as_deref(),
            Some("Sun, 06 Nov 1994 08:49:37 GMT")
        );
    }

    #[test]
    fn ttl_expiry() {
        let mut e = PypiEntry::new("requests");
        // No atime -> never expired.
        assert!(!e.is_expired_with_ttl(&Duration::ZERO));
        e.set_last_updated();
        assert!(!e.is_expired_with_ttl(&Duration::from_secs(3600)));
        std::thread::sleep(Duration::from_millis(2));
        assert!(e.is_expired_with_ttl(&Duration::ZERO));
    }

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
        entry.set_mtime(UNIX_EPOCH);
        cache_store_simple(tmp.path(), &entry, b"{}");

        assert_eq!(
            cache_fetch_simple(tmp.path(), "requests"),
            Some(b"{}".to_vec())
        );
        // Metadata recreated from the pinned mtime.
        let found = cache_try_find_simple(tmp.path(), "requests").unwrap();
        assert_eq!(found.mtime(), Some(UNIX_EPOCH));
        assert!(cache_try_find_simple(tmp.path(), "missing").is_none());
    }
}
