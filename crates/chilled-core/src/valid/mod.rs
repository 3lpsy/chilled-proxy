//! Shared request-path validation helpers. Registry crates own their name
//! charsets; these primitives close the encoding/traversal holes they share.

#[cfg(test)]
mod tests;

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
