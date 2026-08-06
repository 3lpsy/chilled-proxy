use super::*;
use chilled_core::config::RegistrySettings;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

fn config(proxy: &str, upstream: &str) -> Config {
    Config::new(
        crate::config::Upstreams {
            index: Url::parse("https://index.crates.io/").unwrap(),
            download: Url::parse(upstream).unwrap(),
        },
        RegistrySettings {
            cache_dir: PathBuf::from("/tmp/x"),
            cache_ttl: Duration::from_secs(3600),
            cooldown: Duration::ZERO,
            overrides: Arc::new(HashSet::new()),
            restrict_downloads: false,
            proxy_url: Url::parse(proxy).unwrap(),
            max_metadata_size: 0x400_0000,
            max_artifact_size: 0x2000_0000,
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
