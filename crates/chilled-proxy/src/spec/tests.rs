use super::*;

#[test]
fn name_is_the_only_required_key() {
    let spec = parse("maven", "name=plugins").unwrap();
    assert_eq!(spec.name, "plugins");
    // Everything else inherits.
    assert_eq!(spec.path, None);
    assert_eq!(spec.upstream, None);
    assert_eq!(spec.cooldown, None);

    let err = parse("maven", "path=/plugins").unwrap_err();
    assert!(
        err.contains("missing required key 'name'"),
        "unexpected: {err}"
    );
}

#[test]
fn parses_a_full_spec() {
    let spec = parse(
        "maven",
        "name=plugins,path=/gradle-plugins,upstream=https://plugins.gradle.org/m2/,\
         proxy-url=https://proxy.example.com/gradle-plugins,cooldown=3d,cache-ttl=60,\
         restrict-downloads=true",
    )
    .unwrap();

    assert_eq!(spec.name, "plugins");
    assert_eq!(spec.path.as_deref(), Some("/gradle-plugins"));
    assert_eq!(
        spec.upstream.as_ref().map(Url::as_str),
        Some("https://plugins.gradle.org/m2/")
    );
    assert_eq!(spec.cooldown, Some(Duration::from_secs(3 * 86_400)));
    assert_eq!(spec.cache_ttl, Some(60));
    assert_eq!(spec.restrict_downloads, Some(true));
}

#[test]
fn tolerates_whitespace_and_empty_pairs() {
    let spec = parse("npm", " name = mirror , , path = /mirror ,").unwrap();
    assert_eq!(spec.name, "mirror");
    assert_eq!(spec.path.as_deref(), Some("/mirror"));
}

#[test]
fn keys_accept_both_spellings() {
    let spec = parse(
        "maven",
        "name=m,cache_ttl=30,restrict_downloads=no,proxy_url=http://x/",
    )
    .unwrap();
    assert_eq!(spec.cache_ttl, Some(30));
    assert_eq!(spec.restrict_downloads, Some(false));
    assert!(spec.proxy_url.is_some());
}

#[test]
fn paths_are_normalized_like_mount_flags() {
    // The trailing slash is dropped, exactly as `--<registry>-path` does.
    assert_eq!(
        parse("npm", "name=m,path=/mirror/")
            .unwrap()
            .path
            .as_deref(),
        Some("/mirror")
    );
    let err = parse("npm", "name=m,path=mirror").unwrap_err();
    assert!(err.contains("must start with '/'"), "unexpected: {err}");
}

#[test]
fn only_registries_with_a_second_url_accept_one() {
    // crates.io takes `index`, PyPI takes `files`; npm and Maven take neither.
    assert!(parse("crates", "name=c,index=https://index.example.com/")
        .unwrap()
        .secondary
        .is_some());
    assert!(parse("pypi", "name=p,files=https://files.example.com/")
        .unwrap()
        .secondary
        .is_some());

    let err = parse("maven", "name=m,index=https://x/").unwrap_err();
    assert!(err.contains("unknown key 'index'"), "unexpected: {err}");
    // The message lists what the registry does accept.
    assert!(err.contains("upstream"), "lists accepted keys: {err}");

    let err = parse("crates", "name=c,files=https://x/").unwrap_err();
    assert!(err.contains("unknown key 'files'"), "unexpected: {err}");
    assert!(err.contains("index"), "lists the crates.io key: {err}");
}

#[test]
fn rejects_malformed_pairs() {
    for raw in ["name=m,bogus", "name=m,path", "name"] {
        let err = parse("npm", raw).unwrap_err();
        assert!(
            err.contains("expected key=value"),
            "{raw} should be refused: {err}"
        );
    }

    let err = parse("npm", "name=m,path=").unwrap_err();
    assert!(err.contains("has no value"), "unexpected: {err}");
}

#[test]
fn rejects_repeated_keys() {
    // Silently letting the last value win hides a config mistake.
    for raw in [
        "name=a,name=b",
        "name=m,path=/a,path=/b",
        "name=m,cooldown=1d,cooldown=2d",
    ] {
        let err = parse("npm", raw).unwrap_err();
        assert!(
            err.contains("given twice"),
            "{raw} should be refused: {err}"
        );
    }
}

#[test]
fn rejects_unusable_names() {
    // The name becomes a cache subdirectory.
    for name in ["../etc", "a/b", ".hidden", "with space", "quote\"x"] {
        let err = parse("npm", &format!("name={name}")).unwrap_err();
        assert!(
            err.contains("must be [A-Za-z0-9._-]"),
            "{name} should be refused: {err}"
        );
    }
}

#[test]
fn rejects_malformed_values() {
    let err = parse("npm", "name=m,upstream=not a url").unwrap_err();
    assert!(err.contains("is not a valid URL"), "unexpected: {err}");

    let err = parse("npm", "name=m,cooldown=7q").unwrap_err();
    assert!(err.contains("invalid duration unit"), "unexpected: {err}");

    let err = parse("npm", "name=m,cache-ttl=soon").unwrap_err();
    assert!(err.contains("expects seconds"), "unexpected: {err}");

    let err = parse("npm", "name=m,restrict-downloads=maybe").unwrap_err();
    assert!(err.contains("expects a boolean"), "unexpected: {err}");
}

#[test]
fn errors_name_the_flag_and_the_spec() {
    let err = parse("maven", "name=m,cooldown=7q").unwrap_err();
    assert!(
        err.starts_with("--maven-mount 'name=m,cooldown=7q':"),
        "unexpected: {err}"
    );
}

#[test]
fn size_caps_parse_per_mount_with_units() {
    let spec = parse(
        "pypi",
        "name=pytorch,max-artifact-size=2g,max-metadata-size=128m",
    )
    .expect("spec parses");
    assert_eq!(spec.max_artifact_size, Some(2 * 1024 * 1024 * 1024));
    assert_eq!(spec.max_metadata_size, Some(128 * 1024 * 1024));
    // Underscore spelling, as the other keys accept.
    let spec = parse("pypi", "name=p,max_artifact_size=1024").expect("spec parses");
    assert_eq!(spec.max_artifact_size, Some(1024));
}

#[test]
fn size_caps_reject_repeats_and_junk() {
    let err = parse("pypi", "name=p,max-artifact-size=1g,max-artifact-size=2g").unwrap_err();
    assert!(
        err.contains("twice") || err.contains("more than once"),
        "unexpected: {err}"
    );
    let err = parse("pypi", "name=p,max-artifact-size=1potato").unwrap_err();
    assert!(err.contains("invalid size unit"), "unexpected: {err}");
}

#[test]
fn size_cap_keys_are_listed_as_accepted() {
    // A typo'd key should point the operator at the real ones.
    let err = parse("pypi", "name=p,max-artifact=1g").unwrap_err();
    assert!(err.contains("max-artifact-size"), "unexpected: {err}");
    assert!(err.contains("max-metadata-size"), "unexpected: {err}");
}
