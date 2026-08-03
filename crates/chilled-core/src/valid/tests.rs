use super::*;

#[test]
fn decodes_exactly_once() {
    assert_eq!(
        decode_path_once("@scope%2fname"),
        Some("@scope/name".into())
    );
    assert_eq!(decode_path_once("plain/path"), Some("plain/path".into()));
    // Double-encoding leaves a residual `%` after one decode -> rejected.
    assert_eq!(decode_path_once("%252e%252e"), None);
    // A stray `%` not followed by hex stays a `%` -> rejected.
    assert_eq!(decode_path_once("50%"), None);
}

#[test]
fn rejects_smuggled_bytes() {
    assert_eq!(decode_path_once("a%5Cb"), None); // backslash
    assert_eq!(decode_path_once("a%00b"), None); // NUL
    assert_eq!(decode_path_once("a%0d%0ab"), None); // CRLF injection
    assert_eq!(decode_path_once("a%7fb"), None); // DEL
    assert_eq!(decode_path_once("%ff"), None); // invalid UTF-8
}

#[test]
fn clean_segments() {
    assert!(is_clean_segment("serde"));
    assert!(is_clean_segment("@scope"));
    assert!(!is_clean_segment(""));
    assert!(!is_clean_segment("."));
    assert!(!is_clean_segment(".."));
    assert!(!is_clean_segment("a/b"));
    assert!(!is_clean_segment("a\\b"));
}
