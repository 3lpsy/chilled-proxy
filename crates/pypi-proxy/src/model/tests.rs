use super::*;
use std::time::UNIX_EPOCH;

#[test]
fn equivalence_prefers_etag_then_last_modified() {
    let mut a = PypiEntry::new("requests");
    let mut b = PypiEntry::new("requests");
    assert!(!a.is_equivalent(&b));

    a.set_etag("\"e1\"");
    b.set_etag("\"e1\"");
    assert!(a.is_equivalent(&b));

    b.set_etag("\"e2\"");
    assert!(!a.is_equivalent(&b));

    let mut c = PypiEntry::new("requests");
    let mut d = PypiEntry::new("requests");
    c.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
    d.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
    assert!(c.is_equivalent(&d));
}

#[test]
fn last_modified_round_trips() {
    let mut e = PypiEntry::new("requests");
    e.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
    assert_eq!(
        e.last_modified().as_deref(),
        Some("Sun, 06 Nov 1994 08:49:37 GMT")
    );
}

#[test]
fn ttl_expiry() {
    let mut e = PypiEntry::new("requests");
    // No atime -> never expired.
    assert!(!e.is_expired_with_ttl(&Duration::ZERO));
    e.set_last_updated();
    assert!(!e.is_expired_with_ttl(&Duration::from_secs(3600)));
    std::thread::sleep(Duration::from_millis(2));
    assert!(e.is_expired_with_ttl(&Duration::ZERO));
}

#[test]
fn cache_path_is_flat_json() {
    assert_eq!(
        PypiEntry::new("foo-bar").to_file_path(),
        PathBuf::from("foo-bar.json")
    );
}

#[test]
fn cache_store_fetch_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut entry = PypiEntry::new("requests");
    entry.set_mtime(UNIX_EPOCH);
    cache_store_simple(tmp.path(), &entry, b"{}");

    assert_eq!(
        cache_fetch_simple(tmp.path(), "requests"),
        Some(b"{}".to_vec())
    );
    // Metadata recreated from the pinned mtime.
    let found = cache_try_find_simple(tmp.path(), "requests").unwrap();
    assert_eq!(found.mtime(), Some(UNIX_EPOCH));
    assert!(cache_try_find_simple(tmp.path(), "missing").is_none());
}
