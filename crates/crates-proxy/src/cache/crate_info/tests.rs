use super::*;

#[test]
fn from_download_url_accepts_valid() {
    assert_eq!(
        CrateInfo::try_from_download_url("serde/1.0.0/download"),
        Some(CrateInfo::new("serde", "1.0.0"))
    );
    assert_eq!(
        CrateInfo::try_from_download_url("x11-dl/2.21.0-alpha.1+build.2/download"),
        Some(CrateInfo::new("x11-dl", "2.21.0-alpha.1+build.2"))
    );
}

#[test]
fn from_download_url_rejects_malformed() {
    // Missing the `/download` suffix.
    assert_eq!(CrateInfo::try_from_download_url("serde/1.0.0"), None);
    // Wrong segment count.
    assert_eq!(CrateInfo::try_from_download_url("serde/download"), None);
    assert_eq!(CrateInfo::try_from_download_url("a/b/c/download"), None);
}

#[test]
fn from_download_url_rejects_injection_vectors() {
    // SSRF scheme / host and path-traversal segments must not survive.
    assert_eq!(
        CrateInfo::try_from_download_url("http:/1.0.0/download"),
        None
    );
    assert_eq!(
        CrateInfo::try_from_download_url("serde/127.0.0.1:9/download"),
        None
    );
    assert_eq!(CrateInfo::try_from_download_url("../etc/download"), None);
    assert_eq!(
        CrateInfo::try_from_download_url("serde/../../x/download"),
        None
    );
}

#[test]
fn url_and_path_builders() {
    let info = CrateInfo::new("serde", "1.0.0");
    assert_eq!(info.to_download_url(), "serde/1.0.0/download");
    assert_eq!(info.to_file_name(), "serde-1.0.0.crate");
    assert_eq!(
        info.to_file_path(),
        PathBuf::from("serde").join("serde-1.0.0.crate")
    );
}
