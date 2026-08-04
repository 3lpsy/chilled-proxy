//! PEP 503 project-name validation/normalization and file-path shape checks.
//!
//! Project names and file paths are attacker-controlled and fed into
//! `Url::join` and cache paths; restricting them to the PyPI charset closes
//! SSRF (scheme/host segments) and traversal (`..`, `/`).

#[cfg(test)]
mod tests;

use chilled_core::valid::is_clean_segment;

use crate::constants::{FILE_EXTENSIONS, METADATA_SUFFIX};

/// Maximum accepted raw project-name length.
const MAX_NAME_LEN: usize = 128;

/// Maximum accepted distribution filename length.
const MAX_FILENAME_LEN: usize = 256;

/// Maximum accepted directory segments before the filename on the files route.
///
/// PyPI's own layout spends four (`packages/<a>/<b>/<hash>`), but a mount may
/// front an index with a different one — PyTorch serves `whl/cpu/<file>` — so
/// the shape is bounded rather than pinned. The host is never taken from the
/// path (it comes from the pinned files URL), so depth is a traversal and
/// cache-layout question, not an SSRF one.
const MAX_PATH_SEGMENTS: usize = 8;

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

/// Returns `true` for a distribution filename: PyPI filename charset with an
/// allowed extension, optionally with the PEP 658 `.metadata` suffix.
pub(crate) fn is_valid_filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FILENAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-' | b'!'))
        && FILE_EXTENSIONS
            .iter()
            .any(|ext| distribution_name(name).ends_with(ext))
}

/// Strips the PEP 658 `.metadata` suffix, yielding the distribution the file
/// describes (the age gate reads the distribution's upload time).
pub(crate) fn distribution_name(name: &str) -> &str {
    name.strip_suffix(METADATA_SUFFIX).unwrap_or(name)
}

/// Validates a files-route tail: one or more clean directory segments followed
/// by a distribution filename. Returns the filename on success.
///
/// Every segment is charset- and traversal-checked, so the tail can only ever
/// name a path *below* the pinned files URL.
pub(crate) fn validate_fhp_path(path: &str) -> Option<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    let filename = parts.pop()?;
    if parts.is_empty() || parts.len() > MAX_PATH_SEGMENTS {
        return None;
    }
    if !parts.iter().all(|seg| is_path_segment(seg)) || !is_valid_filename(filename) {
        return None;
    }
    Some(filename)
}

/// A directory segment on the files route: clean (no `.`/`..`/empty) and
/// restricted to the charset upstream layouts actually use.
fn is_path_segment(seg: &str) -> bool {
    is_clean_segment(seg)
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
}
