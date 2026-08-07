//! PEP 503 project-name validation and normalization.

/// Maximum accepted raw project-name length.
const MAX_NAME_LEN: usize = 128;

/// Returns `true` for a syntactically valid raw project name: ASCII
/// alphanumerics plus `.`, `_`, `-`, starting and ending alphanumeric
/// (which kills `..`, `.hidden`, and trailing dots).
pub(crate) fn is_valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
}

/// PEP 503 normalization: lowercase, collapsing every run of `-`, `_`, `.`
/// into a single `-`. Safe on arbitrary strings (used for override entries).
pub(crate) fn normalize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending_sep = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            pending_sep = true;
        } else {
            if pending_sep {
                out.push('-');
                pending_sep = false;
            }
            out.push(c.to_ascii_lowercase());
        }
    }
    if pending_sep {
        out.push('-');
    }
    out
}
