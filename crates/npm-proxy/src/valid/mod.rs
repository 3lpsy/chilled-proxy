//! npm name, scope, and version validation.
//!
//! Path segments are attacker-controlled and fed into `Url::join` and cache
//! paths; restricting them to the npm charset closes SSRF (`http:` scheme
//! segments) and traversal (`..`, `/`).

#[cfg(test)]
mod tests;

/// Maximum accepted package name length (npm caps full names at 214).
pub(crate) const MAX_NAME_LEN: usize = 214;

/// Maximum accepted version length.
const MAX_VERSION_LEN: usize = 128;

/// Returns `true` for a valid npm name segment (a bare name, or a scope
/// without its `@`): URL-safe charset, no leading `.` or `_`.
pub(crate) fn is_name_part(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME_LEN
        && !s.starts_with('.')
        && !s.starts_with('_')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'))
}

/// Returns `true` for a syntactically plausible version (semver charset;
/// no `/`, `:`, or `@`, so it cannot inject a host or path separator).
pub(crate) fn is_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= MAX_VERSION_LEN
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
}

/// Parses a tarball file name, returning its version. The file must equal
/// `{unscoped}-{version}.tgz` exactly.
pub(crate) fn tarball_version(unscoped: &str, file: &str) -> Option<String> {
    let version = file
        .strip_suffix(".tgz")?
        .strip_prefix(unscoped)?
        .strip_prefix('-')?;
    is_version(version).then(|| version.to_owned())
}
