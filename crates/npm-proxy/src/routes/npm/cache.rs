//! On-disk cache helpers for packuments and tarballs.

use chilled_core::cache::fs::{fetch_file, file_mtime, store_file};

use crate::model::{NpmEntry, PackageRef};
use crate::state::AppState;

pub(super) async fn cache_read_packument(state: &AppState, pkg: &PackageRef) -> Option<Vec<u8>> {
    let path = state.config.packuments_dir.join(pkg.packument_rel());
    tokio::task::spawn_blocking(move || fetch_file(&path))
        .await
        .ok()
        .flatten()
}

/// Stores the pristine packument, pinning its mtime to `Last-Modified`.
pub(super) async fn cache_write_packument(
    state: &AppState,
    pkg: &PackageRef,
    entry: &NpmEntry,
    data: &[u8],
) {
    let path = state.config.packuments_dir.join(pkg.packument_rel());
    let mtime = entry.mtime();
    let data = data.to_vec();
    let _ = tokio::task::spawn_blocking(move || store_file(&path, &data, mtime)).await;
}

/// Recreates packument metadata from the cache file's mtime.
pub(super) async fn cache_find_packument(state: &AppState, pkg: &PackageRef) -> Option<NpmEntry> {
    let path = state.config.packuments_dir.join(pkg.packument_rel());
    let mtime = tokio::task::spawn_blocking(move || file_mtime(&path))
        .await
        .ok()
        .flatten()?;
    let mut entry = NpmEntry::new();
    entry.set_mtime(mtime);
    Some(entry)
}

/// Reads a cached tarball off the blocking thread pool.
pub(super) async fn cache_read_tarball(
    state: &AppState,
    pkg: &PackageRef,
    file: &str,
) -> Option<Vec<u8>> {
    let path = state.config.tarballs_dir.join(pkg.tarball_rel(file));
    tokio::task::spawn_blocking(move || fetch_file(&path))
        .await
        .ok()
        .flatten()
}
