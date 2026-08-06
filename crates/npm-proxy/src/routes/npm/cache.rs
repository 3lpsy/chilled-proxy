//! On-disk cache helpers for packuments and tarballs.

use bytes::Bytes;
use chilled_core::cache::fs::{fetch_file_async, file_mtime_async, store_file_async};

use crate::model::{NpmEntry, PackageRef};
use crate::state::AppState;

pub(super) async fn cache_read_packument(state: &AppState, pkg: &PackageRef) -> Option<Vec<u8>> {
    fetch_file_async(state.config.packuments_dir.join(pkg.packument_rel())).await
}

/// Stores the pristine packument, pinning its mtime to `Last-Modified`.
pub(super) async fn cache_write_packument(
    state: &AppState,
    pkg: &PackageRef,
    entry: &NpmEntry,
    data: &[u8],
) {
    let path = state.config.packuments_dir.join(pkg.packument_rel());
    store_file_async(path, Bytes::copy_from_slice(data), entry.mtime()).await;
}

/// Recreates packument metadata from the cache file's mtime.
pub(super) async fn cache_find_packument(state: &AppState, pkg: &PackageRef) -> Option<NpmEntry> {
    let path = state.config.packuments_dir.join(pkg.packument_rel());
    let mtime = file_mtime_async(path).await?;
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
    fetch_file_async(state.config.tarballs_dir.join(pkg.tarball_rel(file))).await
}
