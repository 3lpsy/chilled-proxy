//! Crate name and version validation.
//!
//! Attacker-controlled path segments feed `Url::join` and cache paths;
//! restricting them to the crates.io charset closes SSRF and traversal.

/// Maximum accepted crate name / version length (crates.io caps names at 64).
const MAX_LEN: usize = 64;

/// Returns `true` for a syntactically valid crate name: non-empty ASCII
/// alphanumerics plus `-` and `_` (notably no `.`, `/`, `:`, `@`).
#[must_use]
pub(crate) fn is_crate_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Returns `true` for a syntactically plausible crate version (semver charset;
/// no `/`, `:`, or `@`, so it cannot inject a host or path separator).
#[must_use]
pub(crate) fn is_crate_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= MAX_LEN
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(is_crate_name("serde"));
        assert!(is_crate_name("serde_json"));
        assert!(is_crate_name("x11-dl"));
        assert!(!is_crate_name(""));
        assert!(!is_crate_name("http:")); // SSRF scheme vector
        assert!(!is_crate_name("..")); // traversal
        assert!(!is_crate_name("a/b"));
        assert!(!is_crate_name("a.b"));
        assert!(!is_crate_name("@host"));
        assert!(!is_crate_name(&"a".repeat(65)));
    }

    #[test]
    fn versions() {
        assert!(is_crate_version("1.0.0"));
        assert!(is_crate_version("1.0.0-alpha.1+build.2"));
        assert!(!is_crate_version(""));
        assert!(!is_crate_version("127.0.0.1:9999")); // SSRF host vector
        assert!(!is_crate_version("a/b"));
        assert!(!is_crate_version("../x"));
    }
}
