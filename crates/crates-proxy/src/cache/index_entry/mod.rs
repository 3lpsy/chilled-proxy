//! Registry index entry model: sparse-index paths and HTTP validators.

#[cfg(test)]
mod tests;

use std::fmt::{Display, Formatter, Result};
use std::path::PathBuf;

use chilled_core::cache::CacheEntry;

/// Registry index entry structure
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct IndexEntry {
    /// Crate name
    name: String,
    /// Cached response metadata (HTTP validators and freshness).
    pub(crate) meta: CacheEntry,
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
            meta: CacheEntry::new(),
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
