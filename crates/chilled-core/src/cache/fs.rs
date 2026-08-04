//! On-disk cache file helpers. Registry crates own their path derivation and
//! call these with already-validated relative paths.

use std::fs::{create_dir_all, metadata, read, File};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use log::error;

/// Distinguishes concurrent temp files written by this process.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Writes a cache file, creating parent directories and optionally pinning its
/// mtime (used to persist upstream `Last-Modified`). Errors are logged, not fatal.
///
/// The write lands on a unique temp file and is renamed into place, so a crash
/// or a concurrent writer can never leave a truncated body to be served (and
/// cached) as if it were complete.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn store_fetch_round_trip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("a/b/entry");

        assert_eq!(fetch_file(&path), None);
        store_file(&path, b"hello", None);
        assert_eq!(fetch_file(&path), Some(b"hello".to_vec()));
    }

    #[test]
    fn store_pins_mtime() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("entry");
        let mtime = UNIX_EPOCH + Duration::from_secs(784_111_777);

        store_file(&path, b"x", Some(mtime));
        assert_eq!(file_mtime(&path), Some(mtime));
    }

    #[test]
    fn mtime_of_missing_file_is_none() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(file_mtime(&tmp.path().join("missing")), None);
    }

    #[test]
    fn writes_are_atomic_and_leave_no_temp_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("sub/entry");

        store_file(&path, b"first", None);
        store_file(&path, b"second", None);
        assert_eq!(fetch_file(&path), Some(b"second".to_vec()));

        // The rename target is the only file left behind.
        let left: Vec<_> = std::fs::read_dir(tmp.path().join("sub"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["entry"]);
    }

    #[test]
    fn concurrent_writers_never_leave_a_partial_body() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = std::sync::Arc::new(tmp.path().join("entry"));
        let big_a = vec![b'a'; 512 * 1024];
        let big_b = vec![b'b'; 512 * 1024];

        let handles: Vec<_> = [big_a.clone(), big_b.clone()]
            .into_iter()
            .map(|body| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for _ in 0..8 {
                        store_file(&path, &body, None);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        // Whichever writer won, the body is one of them in full — never a mix.
        let got = fetch_file(&path).unwrap();
        assert!(got == big_a || got == big_b, "torn write observed");
    }
}
