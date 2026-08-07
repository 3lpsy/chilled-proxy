//! On-disk cache file helpers. Registry crates own their path derivation and
//! call these with already-validated relative paths.

use std::fs::{create_dir_all, metadata, read, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use log::error;

/// Distinguishes concurrent temp files written by this process.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Writes a cache file, creating parent directories and optionally pinning its
/// mtime (persists upstream `Last-Modified`). Errors are logged, not fatal.
/// The write lands on a unique temp file renamed into place, so a crash or a
/// concurrent writer can never leave a truncated body to be served.
pub fn store_file(path: &Path, data: &[u8], mtime: Option<SystemTime>) {
    let Some(parent) = path.parent() else {
        error!("cache: refusing to write to a parentless path");
        return;
    };
    if let Err(e) = create_dir_all(parent) {
        error!("cache: failed to create cache directory: {e}");
        return;
    }

    let tmp = parent.join(format!(
        ".chilled-tmp.{}.{}",
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));

    let mut file = match File::create(&tmp) {
        Ok(f) => f,
        Err(e) => {
            error!("cache: failed to create cache file: {e}");
            return;
        }
    };
    if let Err(e) = file.write_all(data) {
        error!("cache: failed to write cache file: {e}");
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if let Some(mtime) = mtime {
        file.set_modified(mtime)
            .unwrap_or_else(|e| error!("cache: failed to set cache file mtime: {e}"));
    }
    drop(file);

    if let Err(e) = std::fs::rename(&tmp, path) {
        error!("cache: failed to move cache file into place: {e}");
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Reads a cache file, if present.
pub fn fetch_file(path: &Path) -> Option<Vec<u8>> {
    read(path).ok()
}

/// Returns a cache file's mtime, if the file exists.
pub fn file_mtime(path: &Path) -> Option<SystemTime> {
    metadata(path).ok()?.modified().ok()
}

/// [`store_file`] off the blocking thread pool.
pub async fn store_file_async(path: PathBuf, data: bytes::Bytes, mtime: Option<SystemTime>) {
    let _ = tokio::task::spawn_blocking(move || store_file(&path, &data, mtime)).await;
}

/// [`fetch_file`] off the blocking thread pool.
pub async fn fetch_file_async(path: PathBuf) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(move || fetch_file(&path))
        .await
        .ok()
        .flatten()
}

/// [`file_mtime`] off the blocking thread pool.
pub async fn file_mtime_async(path: PathBuf) -> Option<SystemTime> {
    tokio::task::spawn_blocking(move || file_mtime(&path))
        .await
        .ok()
        .flatten()
}
