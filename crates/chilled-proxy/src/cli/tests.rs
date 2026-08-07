use std::path::Path;
use std::time::Duration;

use clap::Parser;
use url::Url;

use super::*;
use crate::kind::RegistryKind;

fn parse(args: &[&str]) -> Cli {
    let mut argv = vec!["chilled-proxy"];
    argv.extend(args);
    Cli::try_parse_from(argv).expect("cli parses")
}

#[test]
fn defaults_are_general() {
    let cli = parse(&[]);
    for kind in RegistryKind::ALL {
        let s = cli.registry_settings(kind);
        assert_eq!(s.cooldown, Duration::ZERO);
        assert_eq!(s.cache_ttl, Duration::from_secs(3600));
        assert!(!s.restrict_downloads);
        assert!(s.overrides.is_empty());
        assert_eq!(s.cache_dir, Path::new("/var/cache/chilled").join(kind.id()));
        assert_eq!(
            s.proxy_url.as_str(),
            format!("http://127.0.0.1:3080/{kind}/")
        );
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
    for kind in RegistryKind::ALL {
        let s = cli.registry_settings(kind);
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
        cli.registry_settings(RegistryKind::Crates).cooldown,
        Duration::from_secs(604_800)
    );
    assert_eq!(
        cli.registry_settings(RegistryKind::Npm).cooldown,
        Duration::from_secs(86_400)
    );
    assert_eq!(
        cli.registry_settings(RegistryKind::Pypi).cache_ttl,
        Duration::from_secs(90)
    );
    assert!(
        cli.registry_settings(RegistryKind::Crates)
            .restrict_downloads
    );
    assert!(
        !cli.registry_settings(RegistryKind::Maven)
            .restrict_downloads
    );
}

#[test]
fn restrict_downloads_per_registry_without_value_means_true() {
    let cli = parse(&["--restrict-downloads-npm"]);
    assert!(cli.registry_settings(RegistryKind::Npm).restrict_downloads);
    assert!(
        !cli.registry_settings(RegistryKind::Crates)
            .restrict_downloads
    );
}

#[test]
fn per_registry_override_list_replaces_general() {
    let cli = parse(&[
        "--cooldown-overrides",
        "serde,tokio",
        "--cooldown-overrides-npm",
        "lodash",
    ]);
    let crates = cli.registry_settings(RegistryKind::Crates);
    assert!(crates.overrides.contains("serde") && crates.overrides.contains("tokio"));
    let npm = cli.registry_settings(RegistryKind::Npm);
    assert!(npm.overrides.contains("lodash"));
    assert!(!npm.overrides.contains("serde"));
}

#[test]
fn proxy_url_gets_trailing_slash_and_derived_default() {
    let cli = parse(&["--npm-proxy-url", "https://proxy.example.com/npm"]);
    assert_eq!(
        cli.registry_settings(RegistryKind::Npm).proxy_url.as_str(),
        "https://proxy.example.com/npm/"
    );

    // Derived default uses the listen port; 0.0.0.0 maps to localhost.
    let cli = parse(&["--listen", "0.0.0.0:9999"]);
    assert_eq!(
        cli.registry_settings(RegistryKind::Crates)
            .proxy_url
            .as_str(),
        "http://127.0.0.1:9999/crates/"
    );
    let cli = parse(&["--listen", "proxy.lan:8080"]);
    assert_eq!(
        cli.registry_settings(RegistryKind::Maven)
            .proxy_url
            .as_str(),
        "http://proxy.lan:8080/maven/"
    );
}

#[test]
fn reverse_proxy_url_derives_every_mount_default() {
    let cli = parse(&["--reverse-proxy-url", "https://proxy.corp/mirrors"]);
    assert_eq!(
        cli.registry_settings(RegistryKind::Npm).proxy_url.as_str(),
        "https://proxy.corp/mirrors/npm/"
    );

    // Extra mounts derive from the same base, keyed by their own path.
    let cli = parse(&[
        "--reverse-proxy-url",
        "https://proxy.corp/",
        "--maven-mount",
        "name=plugins",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();
    let plugins = instances.iter().find(|i| i.name == "plugins").unwrap();
    assert_eq!(
        plugins.settings.proxy_url.as_str(),
        "https://proxy.corp/plugins/"
    );

    // The per-registry flag still wins for its default mount.
    let cli = parse(&[
        "--reverse-proxy-url",
        "https://proxy.corp/",
        "--npm-proxy-url",
        "https://npm.corp/",
    ]);
    assert_eq!(
        cli.registry_settings(RegistryKind::Npm).proxy_url.as_str(),
        "https://npm.corp/"
    );
    assert_eq!(
        cli.registry_settings(RegistryKind::Pypi).proxy_url.as_str(),
        "https://proxy.corp/pypi/"
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
            cli.registry_settings(RegistryKind::Npm).restrict_downloads,
            "value {truthy} should enable"
        );
    }
    for falsy in ["0", "false", "no", "off"] {
        let cli = parse(&[
            "--restrict-downloads",
            &format!("--restrict-downloads-npm={falsy}"),
        ]);
        assert!(
            !cli.registry_settings(RegistryKind::Npm).restrict_downloads,
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
    for kind in RegistryKind::ALL {
        assert_eq!(cli.mount_path(kind), format!("/{}", kind.id()));
    }
}

#[test]
fn mount_paths_are_configurable_and_normalized() {
    let cli = parse(&["--npm-path", "/registry/npm/", "--maven-path", "/m2"]);
    assert_eq!(cli.mount_path(RegistryKind::Npm), "/registry/npm");
    assert_eq!(cli.mount_path(RegistryKind::Maven), "/m2");
    // Untouched registries keep their defaults.
    assert_eq!(cli.mount_path(RegistryKind::Crates), "/crates");
}

#[test]
fn derived_proxy_url_follows_the_mount() {
    let cli = parse(&["--npm-path", "/registry/npm", "--listen", "proxy.lan:8080"]);
    assert_eq!(
        cli.registry_settings(RegistryKind::Npm).proxy_url.as_str(),
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
        cli.registry_settings(RegistryKind::Npm).proxy_url.as_str(),
        "http://127.0.0.1:3080/"
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

// ---------------------------------------------------------------------------
// Extra mounts (`--<registry>-mount`).
// ---------------------------------------------------------------------------

/// The instance named `name`, or a panic naming what was actually built.
fn instance<'a>(instances: &'a [RegistryInstance], name: &str) -> &'a RegistryInstance {
    instances
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| {
            let built: Vec<&str> = instances.iter().map(|i| i.name.as_str()).collect();
            panic!("no mount named '{name}' among {built:?}")
        })
}

#[test]
fn default_instances_are_named_after_their_registry() {
    let instances = parse(&[]).instances(&crate::auth::ProcessEnv).unwrap();
    let names: Vec<&str> = instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "crates",
            "npm",
            "pypi",
            "maven",
            "gradle-plugins",
            "google-maven"
        ]
    );

    let maven = instance(&instances, "maven");
    assert_eq!(maven.kind, RegistryKind::Maven);
    assert_eq!(maven.path, "/maven");
    assert_eq!(maven.secondary, None);
    // crates.io and PyPI carry a second URL; npm and Maven do not.
    assert!(instance(&instances, "crates").secondary.is_some());
    assert!(instance(&instances, "pypi").secondary.is_some());
    assert_eq!(instance(&instances, "npm").secondary, None);
}

#[test]
fn an_extra_mount_gets_its_own_upstream_path_and_cache() {
    let cli = parse(&[
        "--maven-mount",
        "name=plugins,upstream=https://plugins.gradle.org/m2/",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();
    let plugins = instance(&instances, "plugins");

    assert_eq!(plugins.kind, RegistryKind::Maven);
    assert_eq!(plugins.upstream.as_str(), "https://plugins.gradle.org/m2/");
    // The path defaults to the name, and the cache directory follows it so two
    // mounts of one registry never share cached artifacts.
    assert_eq!(plugins.path, "/plugins");
    assert_eq!(
        plugins.settings.cache_dir,
        Path::new("/var/cache/chilled").join("plugins")
    );
    assert_eq!(
        plugins.settings.proxy_url.as_str(),
        "http://127.0.0.1:3080/plugins/"
    );
    // The registry's own mount is untouched.
    assert_eq!(instance(&instances, "maven").path, "/maven");
}

#[test]
fn an_upstream_without_a_trailing_slash_is_normalized() {
    // Upstreams are joined against, so a missing slash would silently drop the
    // last path segment.
    let cli = parse(&[
        "--maven-mount",
        "name=plugins,upstream=https://plugins.gradle.org/m2",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();
    assert_eq!(
        instance(&instances, "plugins").upstream.as_str(),
        "https://plugins.gradle.org/m2/"
    );
}

#[test]
fn a_mount_spec_overrides_registry_and_general_flags() {
    let cli = parse(&[
        "--cooldown",
        "7d",
        "--cooldown-maven",
        "3d",
        "--restrict-downloads",
        "--maven-mount",
        "name=plugins,cooldown=1d,cache-ttl=90,restrict-downloads=false",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();

    let plugins = instance(&instances, "plugins");
    assert_eq!(plugins.settings.cooldown, Duration::from_secs(86_400));
    assert_eq!(plugins.settings.cache_ttl, Duration::from_secs(90));
    assert!(!plugins.settings.restrict_downloads);

    // Unset spec keys fall back to the registry flag, then the general one.
    let maven = instance(&instances, "maven");
    assert_eq!(maven.settings.cooldown, Duration::from_secs(3 * 86_400));
    assert!(maven.settings.restrict_downloads);
}

#[test]
fn an_extra_mount_inherits_the_registry_upstream_when_it_names_none() {
    // Two mounts of one upstream, differing only in cooldown, is a valid setup.
    let cli = parse(&[
        "--maven-upstream-url",
        "https://repo.example.com/maven2/",
        "--maven-mount",
        "name=fresh,cooldown=0",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();
    assert_eq!(
        instance(&instances, "fresh").upstream.as_str(),
        "https://repo.example.com/maven2/"
    );
}

#[test]
fn extra_mounts_survive_disabling_the_default_one() {
    let cli = parse(&[
        "--disable-maven",
        "--maven-mount",
        "name=plugins,upstream=https://plugins.gradle.org/m2/",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();
    assert!(instances.iter().all(|i| i.name != "maven"));
    assert_eq!(instance(&instances, "plugins").kind, RegistryKind::Maven);
    assert!(cli.check_mounts().is_ok());
}

#[test]
fn every_registry_takes_extra_mounts() {
    let cli = parse(&[
        "--crates-mount",
        "name=c2,index=https://index.example.com/,upstream=https://dl.example.com/",
        "--npm-mount",
        "name=n2,upstream=https://npm.example.com/",
        "--pypi-mount",
        "name=p2,upstream=https://pypi.example.com/simple/,files=https://files.example.com/",
        "--maven-mount",
        "name=m2,upstream=https://maven.example.com/",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();

    let c2 = instance(&instances, "c2");
    assert_eq!(c2.upstream.as_str(), "https://dl.example.com/");
    assert_eq!(
        c2.secondary.as_ref().map(Url::as_str),
        Some("https://index.example.com/")
    );
    let p2 = instance(&instances, "p2");
    assert_eq!(p2.upstream.as_str(), "https://pypi.example.com/simple/");
    assert_eq!(
        p2.secondary.as_ref().map(Url::as_str),
        Some("https://files.example.com/")
    );
    assert_eq!(instance(&instances, "n2").kind, RegistryKind::Npm);
    assert_eq!(instance(&instances, "m2").kind, RegistryKind::Maven);
    assert!(cli.check_mounts().is_ok());
}

#[test]
fn a_registry_takes_more_than_one_extra_mount() {
    let cli = parse(&[
        "--maven-mount",
        "name=plugins,upstream=https://plugins.gradle.org/m2/",
        "--maven-mount",
        "name=google,upstream=https://dl.google.com/dl/android/maven2/",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();
    assert_eq!(instance(&instances, "plugins").path, "/plugins");
    assert_eq!(instance(&instances, "google").path, "/google");
    assert!(cli.check_mounts().is_ok());
}

// ---------------------------------------------------------------------------
// Built-in extra mounts.
// ---------------------------------------------------------------------------

#[test]
fn gradles_other_upstreams_are_mounted_out_of_the_box() {
    // Gating only Central would leave `plugins { }` and AndroidX ungated.
    let cli = parse(&["--cooldown", "7d"]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();

    let portal = instance(&instances, "gradle-plugins");
    assert_eq!(portal.kind, RegistryKind::Maven);
    assert_eq!(portal.path, "/gradle-plugins");
    assert_eq!(portal.upstream.as_str(), "https://plugins.gradle.org/m2/");

    let google = instance(&instances, "google-maven");
    assert_eq!(google.path, "/google-maven");
    assert_eq!(
        google.upstream.as_str(),
        "https://dl.google.com/dl/android/maven2/"
    );

    // They inherit the cooldown, so the default deployment gates all three.
    for name in ["maven", "gradle-plugins", "google-maven"] {
        assert_eq!(
            instance(&instances, name).settings.cooldown,
            Duration::from_secs(604_800),
            "{name} should inherit the general cooldown"
        );
    }
    // Each keeps its own cache and its own derived proxy URL.
    assert_eq!(
        portal.settings.cache_dir,
        Path::new("/var/cache/chilled").join("gradle-plugins")
    );
    assert_eq!(
        portal.settings.proxy_url.as_str(),
        "http://127.0.0.1:3080/gradle-plugins/"
    );
    assert!(cli.check_mounts().is_ok());
}

#[test]
fn built_in_mounts_can_be_turned_off() {
    let instances = parse(&["--no-default-mounts"])
        .instances(&crate::auth::ProcessEnv)
        .unwrap();
    let names: Vec<&str> = instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["crates", "npm", "pypi", "maven"]);
}

#[test]
fn disabling_maven_takes_its_built_in_mounts_with_it() {
    let instances = parse(&["--disable-maven"])
        .instances(&crate::auth::ProcessEnv)
        .unwrap();
    let names: Vec<&str> = instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["crates", "npm", "pypi"]);
}

#[test]
fn an_explicit_mount_replaces_the_built_in_of_the_same_name() {
    // Rather than colliding on the name, which would be a startup error.
    let cli = parse(&[
        "--maven-mount",
        "name=gradle-plugins,path=/portal,upstream=https://mirror.example.com/m2/,cooldown=1d",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();

    let portal = instance(&instances, "gradle-plugins");
    assert_eq!(portal.path, "/portal");
    assert_eq!(portal.upstream.as_str(), "https://mirror.example.com/m2/");
    assert_eq!(portal.settings.cooldown, Duration::from_secs(86_400));
    // Exactly one mount claims the name.
    assert_eq!(
        instances
            .iter()
            .filter(|i| i.name == "gradle-plugins")
            .count(),
        1
    );
    // The other built-in is untouched.
    assert_eq!(instance(&instances, "google-maven").path, "/google-maven");
    assert!(cli.check_mounts().is_ok());
}

#[test]
fn a_root_mount_suppresses_its_built_ins() {
    // A registry at `/` owns the listener, so its built-ins have nowhere to go.
    let cli = parse(&[
        "--maven-path",
        "/",
        "--disable-crates",
        "--disable-npm",
        "--disable-pypi",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();
    let names: Vec<&str> = instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["maven"]);
    assert!(cli.check_mounts().is_ok());
}

#[test]
fn built_in_mounts_do_not_disturb_a_root_deployment() {
    // The documented single-ecosystem layout still starts cleanly.
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
fn one_env_var_can_hold_several_specs() {
    // `;` separates specs so a single CHILLED_MAVEN_MOUNTS can carry a fleet.
    let cli = parse(&["--maven-mount", "name=plugins;name=google"]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();
    assert_eq!(instance(&instances, "plugins").kind, RegistryKind::Maven);
    assert_eq!(instance(&instances, "google").kind, RegistryKind::Maven);
}

#[test]
fn duplicate_mount_names_are_rejected() {
    // Names key the cache directory, so a collision would cross-contaminate it.
    let cli = parse(&[
        "--maven-mount",
        "name=plugins",
        "--npm-mount",
        "name=plugins",
    ]);
    let err = cli.check_mounts().unwrap_err();
    assert!(err.contains("used twice"), "unexpected: {err}");

    // Including a collision with a registry's own default instance.
    let cli = parse(&["--npm-mount", "name=maven"]);
    let err = cli.check_mounts().unwrap_err();
    assert!(err.contains("used twice"), "unexpected: {err}");
}

#[test]
fn nested_mounts_are_rejected() {
    // `/maven` and `/maven/plugins` have no unambiguous routing.
    let cli = parse(&["--maven-mount", "name=plugins,path=/maven/plugins"]);
    let err = cli.check_mounts().unwrap_err();
    assert!(err.contains("nested"), "unexpected: {err}");

    // A sibling path that merely shares a prefix is fine.
    let cli = parse(&["--maven-mount", "name=plugins,path=/maven-plugins"]);
    assert!(cli.check_mounts().is_ok());
}

#[test]
fn a_bad_spec_is_reported_by_check_mounts() {
    let cli = parse(&["--maven-mount", "path=/plugins"]);
    let err = cli.check_mounts().unwrap_err();
    assert!(
        err.contains("missing required key 'name'"),
        "unexpected: {err}"
    );
}

// ---------------------------------------------------------------------------
// Upstream authentication.
// ---------------------------------------------------------------------------

/// Resolves auth over a fixed environment instead of the process's own.
fn with_env(cli: &Cli, pairs: &[(&str, &str)]) -> Result<Vec<RegistryInstance>, String> {
    let env: std::collections::HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    cli.instances(&env)
}

#[test]
fn a_typoed_auth_env_var_is_refused() {
    // The key scan sees the injected environment, so a variable naming a mount
    // token nothing reads is caught before it silently does nothing.
    let cli = parse(&[]);
    let err = with_env(&cli, &[("CHILLED_MAVN_BASIC_AUTH_USERNAME", "alice")]).unwrap_err();
    assert!(err.contains("'MAVN'"), "unexpected error: {err}");

    // The global flag variables end in a matching suffix but name no mount.
    let ok = with_env(&cli, &[("CHILLED_UPSTREAM_HEADERS", "")]);
    assert!(ok.is_ok());
}

#[test]
fn mounts_have_no_auth_by_default() {
    let instances = parse(&[]).instances(&crate::auth::ProcessEnv).unwrap();
    assert!(instances.iter().all(|i| i.auth.is_empty()));
}

#[test]
fn cli_auth_lands_on_the_named_mount_only() {
    let cli = parse(&[
        "--upstream-basic-auth",
        "gradle-plugins=alice:s3cr3t",
        "--upstream-header",
        "maven=X-Build: ci",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();

    assert_eq!(
        instance(&instances, "gradle-plugins").auth.describe(),
        Some("basic auth".to_owned())
    );
    assert_eq!(
        instance(&instances, "maven").auth.describe(),
        Some("1 custom header(s)".to_owned())
    );
    // Nothing bleeds onto the other mounts.
    assert!(instance(&instances, "google-maven").auth.is_empty());
    assert!(instance(&instances, "npm").auth.is_empty());
}

#[test]
fn auth_for_an_unserved_mount_is_rejected() {
    // Otherwise the mount that was meant to be authenticated silently is not,
    // which shows up as an upstream 401 long after startup.
    let cli = parse(&["--upstream-basic-auth", "typo=alice:s3cr3t"]);
    let err = cli.check_mounts().unwrap_err();
    assert!(err.contains("is not served"), "unexpected: {err}");

    let cli = parse(&["--upstream-header", "typo=X-Build: ci"]);
    let err = cli.check_mounts().unwrap_err();
    assert!(err.contains("is not served"), "unexpected: {err}");
}

#[test]
fn auth_naming_a_mount_twice_is_rejected() {
    let cli = parse(&[
        "--upstream-basic-auth",
        "maven=alice:one",
        "--upstream-basic-auth",
        "maven=bob:two",
    ]);
    let err = cli.check_mounts().unwrap_err();
    assert!(err.contains("twice"), "unexpected: {err}");
}

#[test]
fn env_auth_reaches_the_mount_that_owns_the_token() {
    let cli = parse(&[]);
    let instances = with_env(
        &cli,
        &[
            ("CHILLED_GRADLE_PLUGINS_BASIC_AUTH_USERNAME", "alice"),
            ("CHILLED_GRADLE_PLUGINS_BASIC_AUTH_PASSWORD", "s3cr3t"),
            ("CHILLED_MAVEN_HEADERS", "X-Build: ci"),
        ],
    )
    .unwrap();

    assert_eq!(
        instance(&instances, "gradle-plugins").auth.describe(),
        Some("basic auth".to_owned())
    );
    assert_eq!(
        instance(&instances, "maven").auth.describe(),
        Some("1 custom header(s)".to_owned())
    );
    assert!(instance(&instances, "google-maven").auth.is_empty());
}

#[test]
fn a_custom_mount_reads_its_own_env_token() {
    let cli = parse(&["--maven-mount", "name=corp.internal"]);
    let instances = with_env(
        &cli,
        &[
            ("CHILLED_CORP_INTERNAL_BASIC_AUTH_USERNAME", "alice"),
            ("CHILLED_CORP_INTERNAL_BASIC_AUTH_PASSWORD", "s3cr3t"),
        ],
    )
    .unwrap();
    assert_eq!(
        instance(&instances, "corp.internal").auth.describe(),
        Some("basic auth".to_owned())
    );
}

#[test]
fn mounts_colliding_on_an_env_token_are_rejected() {
    // `a-b` and `a.b` both fold to CHILLED_A_B_*, so one would silently take
    // the other's credentials.
    let cli = parse(&["--maven-mount", "name=a-b", "--npm-mount", "name=a.b"]);
    let err = cli.check_mounts().unwrap_err();
    assert!(err.contains("both read CHILLED_A_B_"), "unexpected: {err}");
}

#[test]
fn size_caps_default_to_each_registry_own_limit() {
    // Unlike cooldown, these have no single general default: a 16 MiB crate and
    // a 512 MiB jar are both normal, so an unset flag must leave each registry
    // on its own constant rather than collapsing them onto one number.
    let cli = parse(&[]);
    let expected = [
        (
            RegistryKind::Crates,
            crates_proxy::DEFAULT_MAX_METADATA_SIZE,
            crates_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
        ),
        (
            RegistryKind::Npm,
            npm_proxy::DEFAULT_MAX_METADATA_SIZE,
            npm_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
        ),
        (
            RegistryKind::Pypi,
            pypi_proxy::DEFAULT_MAX_METADATA_SIZE,
            pypi_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
        ),
        (
            RegistryKind::Maven,
            maven_proxy::DEFAULT_MAX_METADATA_SIZE,
            maven_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
        ),
    ];
    for (id, meta, artifact) in expected {
        let s = cli.registry_settings(id);
        assert_eq!(s.max_metadata_size, meta, "{id} metadata");
        assert_eq!(s.max_artifact_size, artifact, "{id} artifact");
    }
    // The defaults are genuinely different, or this test proves nothing.
    assert_ne!(
        crates_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
        maven_proxy::DEFAULT_MAX_ARTIFACT_SIZE
    );
}

#[test]
fn general_size_cap_overrides_every_registry_default() {
    let cli = parse(&["--max-artifact-size", "1g", "--max-metadata-size", "2m"]);
    for kind in RegistryKind::ALL {
        let s = cli.registry_settings(kind);
        assert_eq!(s.max_artifact_size, 1024 * 1024 * 1024, "{kind}");
        assert_eq!(s.max_metadata_size, 2 * 1024 * 1024, "{kind}");
    }
}

#[test]
fn per_registry_size_cap_beats_the_general_one() {
    let cli = parse(&[
        "--max-artifact-size",
        "1g",
        "--max-artifact-size-pypi",
        "2g",
    ]);
    assert_eq!(
        cli.registry_settings(RegistryKind::Pypi).max_artifact_size,
        2 * 1024 * 1024 * 1024
    );
    // Everyone else still takes the general value.
    assert_eq!(
        cli.registry_settings(RegistryKind::Maven).max_artifact_size,
        1024 * 1024 * 1024
    );
}

#[test]
fn file_hosts_spec_beats_the_general_flag() {
    let cli = parse(&[
        "--pypi-file-hosts",
        "general.example",
        "--pypi-mount",
        "name=torch,upstream=https://dl.example/whl/,files=https://dl.example/,\
         file-hosts=cdn.example",
    ]);
    let instances = cli.instances(&crate::auth::ProcessEnv).unwrap();
    assert_eq!(instance(&instances, "torch").file_hosts, ["cdn.example"]);
    // Absent a spec key, a pypi mount inherits the general flag.
    assert_eq!(instance(&instances, "pypi").file_hosts, ["general.example"]);
    // Other registries resolve no file hosts at all.
    assert!(instance(&instances, "maven").file_hosts.is_empty());
}

#[test]
fn a_custom_upstream_must_state_the_secondary_url() {
    // Inheriting the default would silently pair a private mirror with the
    // public index / file host.
    let cli = parse(&[
        "--pypi-mount",
        "name=mirror,upstream=https://pypi.internal/simple/",
    ]);
    let err = cli.instances(&crate::auth::ProcessEnv).unwrap_err();
    assert!(err.contains("files="), "unexpected error: {err}");

    let cli = parse(&[
        "--crates-mount",
        "name=mirror,upstream=https://crates.internal/",
    ]);
    let err = cli.instances(&crate::auth::ProcessEnv).unwrap_err();
    assert!(err.contains("index="), "unexpected error: {err}");

    // Registries without a second URL are unaffected.
    let cli = parse(&[
        "--maven-mount",
        "name=mirror,upstream=https://maven.internal/",
    ]);
    assert!(cli.instances(&crate::auth::ProcessEnv).is_ok());
}

#[test]
fn a_mount_size_cap_beats_the_registry_and_general_ones() {
    // The pytorch case: one mount carrying large ML wheels, without raising the
    // ceiling for every other PyPI mount in the process.
    let cli = parse(&[
        "--max-artifact-size",
        "300m",
        "--max-artifact-size-pypi",
        "400m",
        "--pypi-mount",
        "name=pytorch,upstream=https://download.pytorch.org/whl/cpu/,\
         files=https://download.pytorch.org/,max-artifact-size=2g",
    ]);
    let instances = cli
        .instances(&crate::auth::ProcessEnv)
        .expect("instances resolve");
    let pytorch = instances
        .iter()
        .find(|i| i.name == "pytorch")
        .expect("pytorch mount served");
    assert_eq!(pytorch.settings.max_artifact_size, 2 * 1024 * 1024 * 1024);
    // The registry's own default mount keeps the per-registry value.
    let default = instances
        .iter()
        .find(|i| i.name == "pypi")
        .expect("default pypi mount served");
    assert_eq!(default.settings.max_artifact_size, 400 * 1024 * 1024);
    // And an unrelated registry keeps the general value.
    let npm = instances.iter().find(|i| i.name == "npm").unwrap();
    assert_eq!(npm.settings.max_artifact_size, 300 * 1024 * 1024);
}

// ---------------------------------------------------------------------------
// Web UI flag resolution (`--ui-*`).
// ---------------------------------------------------------------------------

mod ui {
    use std::path::PathBuf;

    use chilled_api::{AuthMode, UiConfig};
    use clap::Parser;

    use crate::cli::ui::{resolve_ui, DEFAULT_CACHE_UPDATE_INTERVAL, DEFAULT_SESSION_TTL};
    use crate::cli::Cli;
    use crate::constants::DEFAULT_UI_DB_PATH;

    fn resolve(args: &[&str]) -> Result<Option<UiConfig>, String> {
        let argv: Vec<&str> = std::iter::once("chilled-proxy")
            .chain(args.iter().copied())
            .collect();
        resolve_ui(&Cli::try_parse_from(argv).expect("argv parses"))
    }

    #[test]
    fn off_by_default_but_rejects_stray_ui_flags() {
        assert!(resolve(&[]).unwrap().is_none());
        let err = resolve(&["--ui-auth", "builtin"]).unwrap_err();
        assert!(err.contains("--ui-auth"), "{err}");
        let err = resolve(&["--ui-public-readonly-enabled"]).unwrap_err();
        assert!(err.contains("--ui is not enabled"), "{err}");
    }

    #[test]
    fn builtin_defaults() {
        let ui = resolve(&["--ui"]).unwrap().unwrap();
        assert_eq!(ui.auth_mode, AuthMode::Builtin);
        assert_eq!(ui.cache_update_interval, DEFAULT_CACHE_UPDATE_INTERVAL);
        assert_eq!(ui.session_ttl, DEFAULT_SESSION_TTL);
        assert_eq!(ui.db_path, PathBuf::from(DEFAULT_UI_DB_PATH));
        assert!(!ui.public_readonly && !ui.trust_first_user_signup);
    }

    #[test]
    fn oidc_requires_and_lowercases_the_user_header() {
        let err = resolve(&["--ui", "--ui-auth", "oidc"]).unwrap_err();
        assert!(err.contains("--ui-oidc-user-header"), "{err}");

        let ui = resolve(&[
            "--ui",
            "--ui-auth",
            "oidc",
            "--ui-oidc-user-header",
            "X-Auth-Request-Email",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(ui.oidc_user_header.as_deref(), Some("x-auth-request-email"));

        let err = resolve(&[
            "--ui",
            "--ui-auth",
            "oidc",
            "--ui-oidc-user-header",
            "bad header",
        ])
        .unwrap_err();
        assert!(err.contains("invalid"), "{err}");
    }

    #[test]
    fn oidc_only_flags_rejected_in_builtin_mode() {
        for args in [
            &["--ui", "--ui-oidc-user-header", "x-user"][..],
            &["--ui", "--ui-oidc-login-url", "/oauth2/sign_in"][..],
        ] {
            let err = resolve(args).unwrap_err();
            assert!(err.contains("--ui-auth oidc"), "{err}");
        }
    }

    #[test]
    fn oidc_mode_rejects_builtin_only_knobs() {
        let oidc = &["--ui", "--ui-auth", "oidc", "--ui-oidc-user-header", "x-u"];
        let err = resolve(&[oidc, &["--ui-trust-first-user-signup"][..]].concat()).unwrap_err();
        assert!(err.contains("incompatible"), "{err}");
        let err = resolve(
            &[
                oidc,
                &["--ui-admin-username", "a", "--ui-admin-password", "b"][..],
            ]
            .concat(),
        )
        .unwrap_err();
        assert!(err.contains("incompatible"), "{err}");
    }

    #[test]
    fn admin_credentials_must_come_together() {
        let err = resolve(&["--ui", "--ui-admin-username", "admin"]).unwrap_err();
        assert!(err.contains("together"), "{err}");
        let err = resolve(&["--ui", "--ui-admin-password", "pw"]).unwrap_err();
        assert!(err.contains("together"), "{err}");
        assert!(resolve(&[
            "--ui",
            "--ui-admin-username",
            "admin",
            "--ui-admin-password",
            "pw"
        ])
        .is_ok());
    }

    #[test]
    fn interval_floor_and_bad_mode() {
        let err = resolve(&["--ui", "--ui-cache-update-interval", "5s"]).unwrap_err();
        assert!(err.contains("at least"), "{err}");
        let err = resolve(&["--ui", "--ui-auth", "saml"]).unwrap_err();
        assert!(err.contains("builtin or oidc"), "{err}");
    }
}
