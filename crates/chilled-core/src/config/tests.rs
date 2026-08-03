use super::*;

fn settings(cooldown_secs: u64, overrides: &[&str]) -> RegistrySettings {
    RegistrySettings {
        cache_dir: PathBuf::from("/tmp/x"),
        cache_ttl: Duration::from_secs(3600),
        cooldown: Duration::from_secs(cooldown_secs),
        overrides: Arc::new(overrides.iter().map(|s| s.to_string()).collect()),
        restrict_downloads: false,
        proxy_url: Url::parse("http://localhost:3080/crates/").unwrap(),
    }
}

#[test]
fn cutoff_none_when_cooldown_disabled() {
    assert_eq!(settings(0, &[]).cutoff_for("serde"), None);
    assert_eq!(settings(0, &[]).serve_window("serde"), None);
}

#[test]
fn cutoff_none_for_overridden_package() {
    let s = settings(86_400, &["serde"]);
    assert_eq!(s.cutoff_for("serde"), None);
    assert!(s.cutoff_for("tokio").is_some());
    assert_eq!(s.serve_window("tokio"), Some(86_400));
}

#[test]
fn cutoff_is_now_minus_cooldown() {
    let s = settings(1000, &[]);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let cutoff = s.cutoff_for("serde").unwrap();
    assert!(cutoff <= now - 1000 && cutoff >= now - 1002);
}

#[test]
fn log_level_normalizes() {
    assert_eq!(normalize_log_level(Some("debug".into())), "debug");
    assert_eq!(normalize_log_level(Some("  WARN ".into())), "warn");
    assert_eq!(normalize_log_level(Some("Off".into())), "off");
    assert_eq!(normalize_log_level(Some("verbose".into())), "info");
    assert_eq!(normalize_log_level(Some(String::new())), "info");
    assert_eq!(normalize_log_level(None), "info");
}

#[test]
fn overrides_parse_lowercased() {
    let set = parse_overrides("Serde, tokio ,,FOO\nbar");
    assert!(set.contains("serde"));
    assert!(set.contains("tokio"));
    assert!(set.contains("foo"));
    assert!(set.contains("bar"));
    assert_eq!(set.len(), 4);
    assert!(parse_overrides("").is_empty());
}
