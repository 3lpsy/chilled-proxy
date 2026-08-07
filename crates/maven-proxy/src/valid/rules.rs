//! Segment, version, and artifact-filename validators.

use crate::checksum::split_checksum;
use crate::constants::MAX_VERSION_LEN;

/// Base artifact extensions accepted after stripping checksum/`.asc` suffixes.
const BASE_EXTS: &[&str] = &[".jar", ".pom", ".war", ".aar", ".module", ".zip", ".tar.gz"];

/// A directory path segment: `[A-Za-z0-9_][A-Za-z0-9._-]*` (no leading dot).
pub(crate) fn is_dir_segment(seg: &str) -> bool {
    chilled_core::valid::is_clean_segment(seg)
        && (seg.as_bytes()[0].is_ascii_alphanumeric() || seg.as_bytes()[0] == b'_')
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// The final filename segment: like a dir segment but also allows `+`.
pub(crate) fn is_file_segment(seg: &str) -> bool {
    chilled_core::valid::is_clean_segment(seg)
        && (seg.as_bytes()[0].is_ascii_alphanumeric() || seg.as_bytes()[0] == b'_')
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
}

/// A version segment: `[A-Za-z0-9._+-]{1,128}`, no leading dot.
pub(crate) fn is_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= MAX_VERSION_LEN
        && !version.starts_with('.')
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
}

/// A classifier between the version and the extension: `[A-Za-z0-9._-]+`.
fn is_classifier(classifier: &str) -> bool {
    !classifier.is_empty()
        && classifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Whether `file` is an acceptable artifact filename for `{artifact}-{version}`:
/// optional checksum and `.asc` suffixes over a whitelisted base extension, with
/// the stem `{artifact}-{version}[-{classifier}]`.
pub(crate) fn is_artifact_file(artifact: &str, version: &str, file: &str) -> bool {
    let (rest, _algo) = split_checksum(file);
    let rest = rest.strip_suffix(".asc").unwrap_or(rest);

    let Some(stem) = BASE_EXTS.iter().find_map(|ext| rest.strip_suffix(ext)) else {
        return false;
    };

    let prefix = format!("{artifact}-{version}");
    if let Some(tail) = stem.strip_prefix(&prefix) {
        return tail.is_empty() || tail.strip_prefix('-').is_some_and(is_classifier);
    }

    // A resolved snapshot names the build instead of `-SNAPSHOT`, e.g.
    // `thing-1.0-20240101.120000-1.jar` under `thing/1.0-SNAPSHOT/`.
    let Some(base) = version.strip_suffix("-SNAPSHOT") else {
        return false;
    };
    let Some(tail) = stem.strip_prefix(&format!("{artifact}-{base}-")) else {
        return false;
    };
    // `<yyyymmdd>.<hhmmss>-<build>[-<classifier>]`.
    let mut parts = tail.splitn(3, '-');
    let (Some(stamp), Some(build)) = (parts.next(), parts.next()) else {
        return false;
    };
    let Some((date, time)) = stamp.split_once('.') else {
        return false;
    };
    let digits = |s: &str, len: usize| s.len() == len && s.bytes().all(|b| b.is_ascii_digit());
    digits(date, 8)
        && digits(time, 6)
        && !build.is_empty()
        && build.bytes().all(|b| b.is_ascii_digit())
        && parts.next().is_none_or(is_classifier)
}
