use super::*;
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn crate_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let info = CrateInfo::new("serde", "1.0.0");

    assert_eq!(cache_fetch_crate(tmp.path(), &info), None);
    cache_store_crate(tmp.path(), &info, b"tarball");
    assert_eq!(
        cache_fetch_crate(tmp.path(), &info),
        Some(b"tarball".to_vec())
    );
    assert!(tmp.path().join("serde/serde-1.0.0.crate").is_file());
}

#[test]
fn index_entry_round_trip_pins_mtime() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut entry = IndexEntry::new("serde");
    entry.set_mtime(UNIX_EPOCH + Duration::from_secs(784_111_777));

    cache_store_index_entry(tmp.path(), &entry, b"{}\n");
    assert_eq!(
        cache_fetch_index_entry(tmp.path(), &entry),
        Some(b"{}\n".to_vec())
    );

    // Metadata is recreatable from the pinned file mtime.
    let found = cache_try_find_index_entry(tmp.path(), "serde").unwrap();
    assert_eq!(found.mtime(), entry.mtime());
    assert_eq!(cache_try_find_index_entry(tmp.path(), "missing"), None);
}
