use super::*;

#[test]
fn test_from_url() {
    assert_eq!(IndexEntry::try_from_index_url(""), None);
    assert_eq!(IndexEntry::try_from_index_url("abc"), None);
    assert_eq!(IndexEntry::try_from_index_url("a/bc"), None);
    assert_eq!(IndexEntry::try_from_index_url("a/b/c/d"), None);

    assert_eq!(
        IndexEntry::try_from_index_url("1/a"),
        Some(IndexEntry::new("a"))
    );
    assert_eq!(
        IndexEntry::try_from_index_url("2/ab"),
        Some(IndexEntry::new("ab"))
    );
    assert_eq!(
        IndexEntry::try_from_index_url("3/a/abc"),
        Some(IndexEntry::new("abc"))
    );
    assert_eq!(
        IndexEntry::try_from_index_url("ab/cd/abcd"),
        Some(IndexEntry::new("abcd"))
    );
}

#[test]
fn test_to_url() {
    assert_eq!(IndexEntry::new("").to_index_url(), "");
    assert_eq!(IndexEntry::new("a").to_index_url(), "1/a");
    assert_eq!(IndexEntry::new("ab").to_index_url(), "2/ab");
    assert_eq!(IndexEntry::new("abc").to_index_url(), "3/a/abc");
    assert_eq!(IndexEntry::new("abcd").to_index_url(), "ab/cd/abcd");
}

#[test]
fn to_url_lowercases_name() {
    // crates.io serves entries at a lowercased path; the download endpoint
    // carries canonical case, so the path must normalize or the
    // --restrict-downloads gate looks in the wrong place. (Regression: a
    // 403 on every uppercase-named crate, e.g. `Inflector`.)
    assert_eq!(
        IndexEntry::new("Inflector").to_index_url(),
        "in/fl/inflector"
    );
    assert_eq!(IndexEntry::new("UUID").to_index_url(), "uu/id/uuid");
    assert_eq!(
        IndexEntry::new("Inflector").to_index_url(),
        IndexEntry::new("inflector").to_index_url()
    );
}

#[test]
fn equivalent_matches_on_etag() {
    let mut a = IndexEntry::new("serde");
    let mut b = IndexEntry::new("serde");
    a.set_etag("\"x\"");
    b.set_etag("\"x\"");
    assert!(a.is_equivalent(&b));
    b.set_etag("\"y\"");
    assert!(!a.is_equivalent(&b));
}

#[test]
fn equivalent_matches_on_last_modified_when_no_etag() {
    let when = "Sun, 06 Nov 1994 08:49:37 GMT";
    let mut a = IndexEntry::new("serde");
    let mut b = IndexEntry::new("serde");
    a.set_last_modified(when);
    b.set_last_modified(when);
    assert!(a.is_equivalent(&b));
    b.set_last_modified("Mon, 07 Nov 1994 08:49:37 GMT");
    assert!(!a.is_equivalent(&b));
}

#[test]
fn equivalent_false_when_no_metadata() {
    // Two bare entries carry no validators, so equivalence cannot be proven.
    let a = IndexEntry::new("serde");
    let b = IndexEntry::new("serde");
    assert!(!a.is_equivalent(&b));
}

#[test]
fn expiry_tracks_atime_and_ttl() {
    // No access time recorded yet -> never considered expired.
    let mut entry = IndexEntry::new("serde");
    assert!(!entry.is_expired_with_ttl(&Duration::from_secs(3600)));

    entry.set_last_updated();
    // A generous TTL is not yet expired; a zero TTL is, once any time passes.
    assert!(!entry.is_expired_with_ttl(&Duration::from_secs(3600)));
    std::thread::sleep(Duration::from_millis(2));
    assert!(entry.is_expired_with_ttl(&Duration::from_secs(0)));
}
