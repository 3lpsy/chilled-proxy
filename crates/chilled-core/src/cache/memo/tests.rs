use super::*;

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
