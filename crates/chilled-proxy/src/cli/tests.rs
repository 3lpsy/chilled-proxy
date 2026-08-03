use super::*;
use clap::Parser;

fn parse(args: &[&str]) -> Cli {
    let mut argv = vec!["chilled-proxy"];
    argv.extend(args);
    Cli::try_parse_from(argv).expect("cli parses")
}

#[test]
fn defaults_are_general() {
    let cli = parse(&[]);
    for id in ["crates", "npm", "pypi", "maven"] {
        let s = cli.registry_settings(id);
        assert_eq!(s.cooldown, Duration::ZERO);
        assert_eq!(s.cache_ttl, Duration::from_secs(3600));
        assert!(!s.restrict_downloads);
        assert!(s.overrides.is_empty());
        assert_eq!(s.cache_dir, Path::new("/var/cache/chilled").join(id));
        assert_eq!(s.proxy_url.as_str(), format!("http://localhost:3080/{id}/"));
    }
}

#[test]
fn general_flags_apply_to_every_registry() {
    let cli = parse(&[
        "--cooldown",
        "7d",
        "--cache-ttl",
        "60",
        "--restrict-downloads",
    ]);
    for id in ["crates", "npm", "pypi", "maven"] {
        let s = cli.registry_settings(id);
        assert_eq!(s.cooldown, Duration::from_secs(604_800));
        assert_eq!(s.cache_ttl, Duration::from_secs(60));
        assert!(s.restrict_downloads);
    }
}

#[test]
fn per_registry_flags_override_general() {
    let cli = parse(&[
        "--cooldown",
        "7d",
        "--cooldown-npm",
        "1d",
        "--cache-ttl-pypi",
        "90",
        "--restrict-downloads",
        "--restrict-downloads-maven=false",
    ]);
    assert_eq!(
        cli.registry_settings("crates").cooldown,
        Duration::from_secs(604_800)
    );
    assert_eq!(
        cli.registry_settings("npm").cooldown,
        Duration::from_secs(86_400)
    );
    assert_eq!(
        cli.registry_settings("pypi").cache_ttl,
        Duration::from_secs(90)
    );
    assert!(cli.registry_settings("crates").restrict_downloads);
    assert!(!cli.registry_settings("maven").restrict_downloads);
}

#[test]
fn restrict_downloads_per_registry_without_value_means_true() {
    let cli = parse(&["--restrict-downloads-npm"]);
    assert!(cli.registry_settings("npm").restrict_downloads);
    assert!(!cli.registry_settings("crates").restrict_downloads);
}

#[test]
fn per_registry_override_list_replaces_general() {
    let cli = parse(&[
        "--cooldown-overrides",
        "serde,tokio",
        "--cooldown-overrides-npm",
        "lodash",
    ]);
    let crates = cli.registry_settings("crates");
    assert!(crates.overrides.contains("serde") && crates.overrides.contains("tokio"));
    let npm = cli.registry_settings("npm");
    assert!(npm.overrides.contains("lodash"));
    assert!(!npm.overrides.contains("serde"));
}

#[test]
fn proxy_url_gets_trailing_slash_and_derived_default() {
    let cli = parse(&["--npm-proxy-url", "https://proxy.example.com/npm"]);
    assert_eq!(
        cli.registry_settings("npm").proxy_url.as_str(),
        "https://proxy.example.com/npm/"
    );

    // Derived default uses the listen port; 0.0.0.0 maps to localhost.
    let cli = parse(&["--listen", "0.0.0.0:9999"]);
    assert_eq!(
        cli.registry_settings("crates").proxy_url.as_str(),
        "http://localhost:9999/crates/"
    );
    let cli = parse(&["--listen", "proxy.lan:8080"]);
    assert_eq!(
        cli.registry_settings("maven").proxy_url.as_str(),
        "http://proxy.lan:8080/maven/"
    );
}

#[test]
fn log_level_resolution() {
    assert_eq!(parse(&[]).resolved_log_level(), "info");
    assert_eq!(parse(&["-v"]).resolved_log_level(), "debug");
    assert_eq!(parse(&["-vv"]).resolved_log_level(), "trace");
    assert_eq!(parse(&["--log-level", "WARN"]).resolved_log_level(), "warn");
    // -v beats --log-level.
    assert_eq!(
        parse(&["-v", "--log-level", "warn"]).resolved_log_level(),
        "debug"
    );
}

#[test]
fn boolean_flags_accept_common_spellings() {
    // Container operators write `=1`/`=yes`; a strict true/false parser would
    // make the process exit at startup instead of enabling the feature.
    for truthy in ["1", "true", "yes", "on"] {
        let cli = parse(&[&format!("--restrict-downloads-npm={truthy}")]);
        assert!(
            cli.registry_settings("npm").restrict_downloads,
            "value {truthy} should enable"
        );
    }
    for falsy in ["0", "false", "no", "off"] {
        let cli = parse(&[
            "--restrict-downloads",
            &format!("--restrict-downloads-npm={falsy}"),
        ]);
        assert!(
            !cli.registry_settings("npm").restrict_downloads,
            "value {falsy} should disable"
        );
    }
}

#[test]
fn disable_flags_parse() {
    let cli = parse(&["--disable-npm", "--disable-maven"]);
    assert!(!cli.disable_crates);
    assert!(cli.disable_npm);
    assert!(!cli.disable_pypi);
    assert!(cli.disable_maven);
}

#[test]
fn upstream_url_defaults() {
    let cli = parse(&[]);
    assert_eq!(cli.crates_index_url.as_str(), "https://index.crates.io/");
    assert_eq!(cli.crates_upstream_url.as_str(), "https://crates.io/");
    assert_eq!(cli.npm_upstream_url.as_str(), "https://registry.npmjs.org/");
    assert_eq!(cli.pypi_upstream_url.as_str(), "https://pypi.org/simple/");
    assert_eq!(
        cli.pypi_files_url.as_str(),
        "https://files.pythonhosted.org/"
    );
    assert_eq!(
        cli.maven_upstream_url.as_str(),
        "https://repo.maven.apache.org/maven2/"
    );
}

#[test]
fn mount_paths_default_to_the_registry_name() {
    let cli = parse(&[]);
    for id in ["crates", "npm", "pypi", "maven"] {
        assert_eq!(cli.mount_path(id), format!("/{id}"));
    }
}

#[test]
fn mount_paths_are_configurable_and_normalized() {
    let cli = parse(&["--npm-path", "/registry/npm/", "--maven-path", "/m2"]);
    assert_eq!(cli.mount_path("npm"), "/registry/npm");
    assert_eq!(cli.mount_path("maven"), "/m2");
    // Untouched registries keep their defaults.
    assert_eq!(cli.mount_path("crates"), "/crates");
}

#[test]
fn derived_proxy_url_follows_the_mount() {
    let cli = parse(&["--npm-path", "/registry/npm", "--listen", "proxy.lan:8080"]);
    assert_eq!(
        cli.registry_settings("npm").proxy_url.as_str(),
        "http://proxy.lan:8080/registry/npm/"
    );

    // Root mount yields the bare origin.
    let cli = parse(&[
        "--npm-path",
        "/",
        "--disable-crates",
        "--disable-pypi",
        "--disable-maven",
    ]);
    assert_eq!(
        cli.registry_settings("npm").proxy_url.as_str(),
        "http://localhost:3080/"
    );
}

#[test]
fn root_mount_is_rejected_alongside_other_registries() {
    let cli = parse(&["--npm-path", "/"]);
    let err = cli.check_mounts().unwrap_err();
    assert!(err.contains("only one enabled"), "unexpected: {err}");

    // Disabling the rest makes it legal.
    let cli = parse(&[
        "--npm-path",
        "/",
        "--disable-crates",
        "--disable-pypi",
        "--disable-maven",
    ]);
    assert!(cli.check_mounts().is_ok());
}

#[test]
fn colliding_mounts_are_rejected() {
    let cli = parse(&["--npm-path", "/pkgs", "--pypi-path", "/pkgs"]);
    assert!(cli.check_mounts().unwrap_err().contains("both mounted"));

    // A disabled registry cannot collide with anything.
    let cli = parse(&[
        "--npm-path",
        "/pkgs",
        "--pypi-path",
        "/pkgs",
        "--disable-pypi",
    ]);
    assert!(cli.check_mounts().is_ok());
}

#[test]
fn malformed_mount_fails_at_parse_time() {
    assert!(Cli::try_parse_from(["chilled-proxy", "--npm-path", "relative"]).is_err());
    assert!(Cli::try_parse_from(["chilled-proxy", "--npm-path", "/../etc"]).is_err());
}
