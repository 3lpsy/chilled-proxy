use super::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

fn config(cooldown_secs: u64, overrides: &[&str]) -> Config {
    Config::new(
        Url::parse("https://index.crates.io/").unwrap(),
        Url::parse("https://crates.io/").unwrap(),
        RegistrySettings {
            cache_dir: PathBuf::from("/var/cache/chilled/crates"),
            cache_ttl: Duration::from_secs(3600),
            cooldown: Duration::from_secs(cooldown_secs),
            overrides: Arc::new(
                overrides
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<HashSet<_>>(),
            ),
            restrict_downloads: false,
            proxy_url: Url::parse("http://localhost:3080/crates/").unwrap(),
        },
    )
}

#[test]
fn derives_cache_subdirs() {
    let c = config(0, &[]);
    assert_eq!(
        c.index_dir,
        PathBuf::from("/var/cache/chilled/crates/index")
    );
    assert_eq!(
        c.crates_dir,
        PathBuf::from("/var/cache/chilled/crates/crates")
    );
}

#[test]
fn override_lookup_is_case_insensitive() {
    let c = config(86_400, &["serde"]);
    assert_eq!(c.cutoff_for("Serde"), None);
    assert_eq!(c.serve_marker("SERDE"), None);
    assert!(c.cutoff_for("tokio").is_some());
    assert_eq!(c.serve_marker("tokio").unwrap().window, 86_400);
}
