use super::*;
use chilled_core::config::RegistrySettings;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

fn config(proxy: &str, upstream: &str) -> Config {
    Config::new(
        Url::parse("https://index.crates.io/").unwrap(),
        Url::parse(upstream).unwrap(),
        RegistrySettings {
            cache_dir: PathBuf::from("/tmp/x"),
            cache_ttl: Duration::from_secs(3600),
            cooldown: Duration::ZERO,
            overrides: Arc::new(HashSet::new()),
            restrict_downloads: false,
            proxy_url: Url::parse(proxy).unwrap(),
        },
    )
}

#[test]
fn config_json_points_downloads_at_the_mount() {
    let c = config("http://proxy:3080/crates/", "https://crates.io/");
    assert_eq!(
        gen_config_json_file(&c),
        r#"{"dl":"http://proxy:3080/crates/api/v1/crates","api":"https://crates.io"}"#
    );
}

#[test]
fn config_json_trims_trailing_slashes() {
    // Cargo cannot handle trailing slashes in config.json values.
    let c = config("http://localhost:3080/crates/", "https://crates.io/");
    let body = gen_config_json_file(&c);
    assert!(!body.contains("crates/\""));
    assert!(!body.contains("io/\""));
}

#[test]
fn entry_validator_prefers_etag() {
    let mut entry = IndexEntry::new("serde");
    assert_eq!(entry_validator(&entry), "");
    entry.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
    assert_eq!(entry_validator(&entry), "Sun, 06 Nov 1994 08:49:37 GMT");
    entry.set_etag("\"abc\"");
    assert_eq!(entry_validator(&entry), "\"abc\"");
}
