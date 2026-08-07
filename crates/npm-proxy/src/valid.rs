//! npm name, scope, and version validation.
//!
//! Attacker-controlled path segments feed `Url::join` and cache paths;
//! restricting them to the npm charset closes SSRF and traversal.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_parts() {
        assert!(is_name_part("lodash"));
        assert!(is_name_part("Legacy-Uppercase"));
        assert!(is_name_part("dot.mid_under~tilde"));
        assert!(is_name_part(&"a".repeat(214)));
        assert!(!is_name_part(""));
        assert!(!is_name_part(".hidden")); // no leading dot
        assert!(!is_name_part("_private")); // no leading underscore
        assert!(!is_name_part("http:")); // SSRF scheme vector
        assert!(!is_name_part("a/b")); // separator
        assert!(!is_name_part("a@b"));
        assert!(!is_name_part("..")); // traversal
        assert!(!is_name_part(&"a".repeat(215)));
    }

    #[test]
    fn versions() {
        assert!(is_version("1.0.0"));
        assert!(is_version("1.0.0-alpha.1+build.2"));
        assert!(!is_version(""));
        assert!(!is_version("1.0.0_x")); // underscore not in semver charset
        assert!(!is_version("127.0.0.1:9999")); // SSRF host vector
        assert!(!is_version("a/b"));
        assert!(!is_version("../x"));
        assert!(!is_version(&"1".repeat(129)));
    }

    #[test]
    fn tarball_files() {
        assert_eq!(
            tarball_version("lodash", "lodash-4.17.21.tgz"),
            Some("4.17.21".to_owned())
        );
        assert_eq!(
            tarball_version("a-b", "a-b-1.0.0-rc.1.tgz"),
            Some("1.0.0-rc.1".to_owned())
        );
        // File must match the package's own name — no cache poisoning via mismatch.
        assert_eq!(tarball_version("lodash", "other-1.0.0.tgz"), None);
        assert_eq!(tarball_version("lodash", "lodash-1.0.0.tar"), None);
        assert_eq!(tarball_version("lodash", "lodash.tgz"), None);
        assert_eq!(tarball_version("lodash", "lodash-.tgz"), None);
        assert_eq!(tarball_version("lodash", "lodash-../../etc.tgz"), None);
    }
}
