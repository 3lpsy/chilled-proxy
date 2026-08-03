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
