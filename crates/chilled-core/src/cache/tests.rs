use std::time::{Duration, UNIX_EPOCH};

use bytes::Bytes;

use super::fs::{fetch_file, file_mtime, store_file};
use super::{CacheEntry, FilteredMemo, MetadataCache};

// --- entry ---

#[test]
fn equivalence_prefers_etag_then_last_modified() {
    let mut a = CacheEntry::new();
    let mut b = CacheEntry::new();
    assert!(!a.is_equivalent(&b));

    a.set_etag("\"e1\"");
    b.set_etag("\"e1\"");
    assert!(a.is_equivalent(&b));
    b.set_etag("\"e2\"");
    assert!(!a.is_equivalent(&b));

    let mut c = CacheEntry::new();
    let mut d = CacheEntry::new();
    c.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
    d.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
    assert!(c.is_equivalent(&d));
    assert!(!c.is_equivalent(&CacheEntry::new()));
}

#[test]
fn last_modified_round_trips() {
    let mut e = CacheEntry::new();
    e.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
    assert_eq!(
        e.last_modified().as_deref(),
        Some("Sun, 06 Nov 1994 08:49:37 GMT")
    );

    let mut m = CacheEntry::new();
    m.set_mtime(UNIX_EPOCH);
    assert_eq!(
        m.last_modified().as_deref(),
        Some("Thu, 01 Jan 1970 00:00:00 GMT")
    );
}

#[test]
fn validator_prefers_etag_over_last_modified() {
    let mut e = CacheEntry::new();
    assert_eq!(e.validator(), "");
    e.set_last_modified("Sat, 01 Jan 2000 00:00:00 GMT");
    assert_eq!(e.validator(), "Sat, 01 Jan 2000 00:00:00 GMT");
    e.set_etag("\"x\"");
    assert_eq!(e.validator(), "\"x\"");
}

#[test]
fn expiry_requires_a_recorded_update() {
    let mut e = CacheEntry::new();
    assert!(!e.is_expired_with_ttl(&Duration::ZERO));
    e.set_last_updated();
    assert!(!e.is_expired_with_ttl(&Duration::from_secs(3600)));
    std::thread::sleep(Duration::from_millis(2));
    assert!(e.is_expired_with_ttl(&Duration::ZERO));
}

// --- fs ---

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

// --- memo ---

#[test]
fn memo_respects_validator_and_bucket() {
    let memo = FilteredMemo::new();
    memo.put("a".into(), "etag1".into(), 10, Bytes::from_static(b"x"));
    assert_eq!(memo.get("a", "etag1", 10), Some(Bytes::from_static(b"x")));
    // Different source content -> miss.
    assert_eq!(memo.get("a", "etag2", 10), None);
    // Different cutoff bucket -> miss.
    assert_eq!(memo.get("a", "etag1", 11), None);
    // Unknown key -> miss.
    assert_eq!(memo.get("b", "etag1", 10), None);
}

#[test]
fn unvalidatable_bodies_are_never_memoized() {
    // With no upstream ETag or Last-Modified there is nothing to invalidate
    // against, so a refreshed body must not be shadowed by the old one.
    let memo = FilteredMemo::new();
    memo.put("a".into(), String::new(), 10, Bytes::from_static(b"old"));
    assert_eq!(memo.get("a", "", 10), None);

    // A validated entry for the same key still behaves normally.
    memo.put("a".into(), "etag1".into(), 10, Bytes::from_static(b"new"));
    assert_eq!(memo.get("a", "etag1", 10), Some(Bytes::from_static(b"new")));
    assert_eq!(memo.get("a", "", 10), None);
}

// --- metadata ---

#[test]
fn store_fetch_invalidate_round_trip() {
    let cache: MetadataCache<String> = MetadataCache::new();

    assert_eq!(cache.fetch("serde"), None);
    cache.store("serde", "\"abc\"".to_string());
    assert_eq!(cache.fetch("serde"), Some("\"abc\"".to_string()));
    cache.invalidate("serde");
    assert_eq!(cache.fetch("serde"), None);
}

#[test]
fn store_overwrites() {
    let cache: MetadataCache<u32> = MetadataCache::new();
    cache.store("a", 1);
    cache.store("a", 2);
    assert_eq!(cache.fetch("a"), Some(2));
}
