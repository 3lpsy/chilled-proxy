//! Async wrappers moving pristine simple-index cache I/O off the async workers.

use std::path::Path;

use crate::model::{cache_fetch_simple, cache_store_simple, cache_try_find_simple, PypiEntry};

/// Reads the cached pristine simple index off the blocking thread pool.
pub(super) async fn cache_read_simple(dir: &Path, name: &str) -> Option<Vec<u8>> {
    let dir = dir.to_path_buf();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || cache_fetch_simple(&dir, &name))
        .await
        .ok()
        .flatten()
}

/// Stores a pristine simple index off the blocking thread pool.
pub(super) async fn cache_write_simple(dir: &Path, entry: &PypiEntry, data: &[u8]) {
    let dir = dir.to_path_buf();
    let entry = entry.clone();
    let data = data.to_vec();
    let _ = tokio::task::spawn_blocking(move || cache_store_simple(&dir, &entry, &data)).await;
}

/// Recreates entry metadata from the cache file's mtime off the blocking pool.
pub(super) async fn cache_find_simple(dir: &Path, name: &str) -> Option<PypiEntry> {
    let dir = dir.to_path_buf();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || cache_try_find_simple(&dir, &name))
        .await
        .ok()
        .flatten()
}
