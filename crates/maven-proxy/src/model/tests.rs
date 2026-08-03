use std::time::{Duration, SystemTime};

use super::MavenEntry;

#[test]
fn equivalence_prefers_etag() {
    let mut a = MavenEntry::new();
    let mut b = MavenEntry::new();
    a.set_etag("\"x\"");
    b.set_etag("\"x\"");
    assert!(a.is_equivalent(&b));
    b.set_etag("\"y\"");
    assert!(!a.is_equivalent(&b));
}

#[test]
fn equivalence_falls_back_to_last_modified() {
    let mut a = MavenEntry::new();
    let mut b = MavenEntry::new();
    a.set_last_modified("Sat, 01 Jan 2000 00:00:00 GMT");
    b.set_last_modified("Sat, 01 Jan 2000 00:00:00 GMT");
    assert!(a.is_equivalent(&b));
    assert!(!a.is_equivalent(&MavenEntry::new()));
}

#[test]
fn empty_entries_are_never_equivalent() {
    assert!(!MavenEntry::new().is_equivalent(&MavenEntry::new()));
}

#[test]
fn validator_prefers_etag_over_last_modified() {
    let mut e = MavenEntry::new();
    assert_eq!(e.validator(), "");
    e.set_last_modified("Sat, 01 Jan 2000 00:00:00 GMT");
    assert_eq!(e.validator(), "Sat, 01 Jan 2000 00:00:00 GMT");
    e.set_etag("\"x\"");
    assert_eq!(e.validator(), "\"x\"");
}

#[test]
fn expiry_requires_a_recorded_update() {
    let mut e = MavenEntry::new();
    assert!(!e.is_expired_with_ttl(&Duration::ZERO));
    e.set_last_updated();
    assert!(e.is_expired_with_ttl(&Duration::ZERO));
    assert!(!e.is_expired_with_ttl(&Duration::from_secs(3600)));
}

#[test]
fn last_modified_round_trips_mtime() {
    let mut e = MavenEntry::new();
    e.set_mtime(SystemTime::UNIX_EPOCH);
    assert_eq!(e.last_modified().unwrap(), "Thu, 01 Jan 1970 00:00:00 GMT");
}
