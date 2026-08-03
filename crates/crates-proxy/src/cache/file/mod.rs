//! On-disk cache paths for index entries and `.crate` files, over the generic
//! store/fetch helpers in `chilled_core::cache::fs`.

#[cfg(test)]
mod tests;

use std::path::Path;

use chilled_core::cache::fs::{fetch_file, file_mtime, store_file};

use super::crate_info::CrateInfo;
use super::index_entry::IndexEntry;

/// Caches the crate package file on the local filesystem.
pub(crate) fn cache_store_crate(dir: &Path, crate_info: &CrateInfo, data: &[u8]) {
    store_file(&dir.join(crate_info.to_file_path()), data, None);
}

/// Fetches the cached crate package file, if present.
pub(crate) fn cache_fetch_crate(dir: &Path, crate_info: &CrateInfo) -> Option<Vec<u8>> {
    fetch_file(&dir.join(crate_info.to_file_path()))
}

/// Caches the index entry file, pinning its mtime to the `Last-Modified` metadata.
pub(crate) fn cache_store_index_entry(dir: &Path, entry: &IndexEntry, data: &[u8]) {
    store_file(&dir.join(entry.to_file_path()), data, entry.mtime());
}

/// Fetches the cached index entry file, if present.
pub(crate) fn cache_fetch_index_entry(dir: &Path, entry: &IndexEntry) -> Option<Vec<u8>> {
    fetch_file(&dir.join(entry.to_file_path()))
}

/// Recreates missing index entry metadata from the cache file's mtime.
pub(crate) fn cache_try_find_index_entry(dir: &Path, name: &str) -> Option<IndexEntry> {
    let mut entry = IndexEntry::new(name);
    let mtime = file_mtime(&dir.join(entry.to_file_path()))?;
    entry.set_mtime(mtime);
    Some(entry)
}
