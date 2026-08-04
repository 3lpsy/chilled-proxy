//! Shared request-path validation helpers. Registry crates own their name
//! charsets; these primitives close the encoding/traversal holes they share.

use percent_encoding::percent_decode_str;

/// Percent-decodes a raw URI path exactly once, rejecting anything that could
/// smuggle a second layer: residual `%`, backslashes, control bytes, or NUL.
///
/// SSRF/traversal-load-bearing: registries route on the decoded output and must
/// never decode again.
pub fn decode_path_once(raw: &str) -> Option<String> {
    let decoded = percent_decode_str(raw).decode_utf8().ok()?;
    if decoded.contains('%')
        || decoded.contains('\\')
        || decoded.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}')
    {
        return None;
    }
    Some(decoded.into_owned())
}

/// Returns `true` for a path segment that is non-empty and cannot traverse:
/// not `.`/`..`, no separators.
pub fn is_clean_segment(seg: &str) -> bool {
    !seg.is_empty() && seg != "." && seg != ".." && !seg.contains(['/', '\\'])
}

#[cfg(test)]
mod tests {
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
}
