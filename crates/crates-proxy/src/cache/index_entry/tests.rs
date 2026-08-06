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
