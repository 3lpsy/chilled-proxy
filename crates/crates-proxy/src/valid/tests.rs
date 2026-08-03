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
