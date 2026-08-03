//! Registry index entry model: sparse-index paths and HTTP validators.

#[cfg(test)]
mod tests;

use std::fmt::{Display, Formatter, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use chilled_core::http::{fmt_http_date, parse_http_date};

/// Registry index entry structure
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct IndexEntry {
    /// Crate name
    name: String,
    /// HTTP entity tag header
    etag: Option<String>,
    /// Index file modification time
    mtime: Option<SystemTime>,
    /// Last index entry update check time
    atime: Option<Instant>,
}

impl Display for IndexEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.write_str(&self.name)
    }
}

impl IndexEntry {
    /// Creates a registry index entry object for a crate.
    #[must_use]
    pub(crate) fn new(name: &str) -> Self {
        IndexEntry {
            name: name.to_owned(),
            etag: None,
            mtime: None,
            atime: None,
        }
    }

    /// Creates an entry from the sparse index URL path.
    ///
    /// Rejects crate names outside the crates.io character set, closing off
    /// SSRF and path-traversal via crafted index paths.
    #[must_use]
    pub(crate) fn try_from_index_url(url: &str) -> Option<Self> {
        let mut i = url.split('/');

        let name = match i.next() {
            Some("1" | "2") => match (i.next(), i.next()) {
                (Some(name), None) => name,
                _ => return None,
            },
            _ => match (i.next(), i.next(), i.next()) {
                (Some(_), Some(name), None) => name,
                _ => return None,
            },
        };

        crate::valid::is_crate_name(name).then(|| IndexEntry::new(name))
    }

    /// Gets the crate name.
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Checks if this index entry file contents is the same
    /// as `other` according to the associated metadata.
    #[must_use]
    pub(crate) fn is_equivalent(&self, other: &IndexEntry) -> bool {
        (self.etag().is_some() && (self.etag() == other.etag()))
            || (self.last_modified().is_some() && (self.last_modified() == other.last_modified()))
    }

    /// Checks if this index entry is expired according for the TTL given.
    #[must_use]
    pub(crate) fn is_expired_with_ttl(&self, ttl: &Duration) -> bool {
        self.atime.is_some_and(|atime| atime.elapsed() > *ttl)
    }

    /// Gets the HTTP entity tag metadata.
    #[must_use]
    pub(crate) fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// Gets the HTTP Last-Modified metadata.
    #[must_use]
    pub(crate) fn last_modified(&self) -> Option<String> {
        self.mtime.map(fmt_http_date)
    }

    /// Gets the file modification time metadata.
    #[must_use]
    pub(crate) fn mtime(&self) -> Option<SystemTime> {
        self.mtime
    }

    /// Sets the HTTP entity tag metadata.
    pub(crate) fn set_etag(&mut self, etag: &str) {
        self.etag = Some(etag.to_owned());
    }

    /// Sets the HTTP Last-Modified metadata.
    pub(crate) fn set_last_modified(&mut self, last_modified: &str) {
        self.mtime = parse_http_date(last_modified);
    }

    /// Sets the file modification time metadata.
    pub(crate) fn set_mtime(&mut self, mtime: SystemTime) {
        self.mtime = Some(mtime);
    }

    /// Updates the last upstream server access time metadata.
    pub(crate) fn set_last_updated(&mut self) {
        self.atime = Some(Instant::now());
    }

    /// Builds the index entry download URL (relative).
    ///
    /// The name is ASCII-lowercased to match the sparse-index path convention:
    /// crates.io serves entries at a lowercased path (e.g. `Inflector` lives at
    /// `in/fl/inflector`), and cargo requests the index that way. The download
    /// endpoint, however, carries the crate's canonical case, so without this
    /// normalization the `--restrict-downloads` gate would look up a cached entry
    /// at the wrong path and fail-closed (403) for any crate with uppercase in
    /// its name.
    #[must_use]
    pub(crate) fn to_index_url(&self) -> String {
        let name = self.name.to_ascii_lowercase();

        match name.len() {
            0 => String::new(),
            sz @ (1 | 2) => format!("{sz}/{name}"),
            3 => format!("3/{first}/{name}", first = &name[..1]),
            _ => format!(
                "{first}/{second}/{name}",
                first = &name[0..2],
                second = &name[2..4]
            ),
        }
    }

    /// Builds the relative index entry file path for cache storage.
    #[must_use]
    pub(crate) fn to_file_path(&self) -> PathBuf {
        PathBuf::from(self.to_index_url())
    }
}
