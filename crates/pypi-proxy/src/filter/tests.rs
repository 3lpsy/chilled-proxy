use super::*;
use serde_json::json;

fn proxy_url() -> Url {
    Url::parse("http://localhost:3080/pypi/").unwrap()
}

#[test]
fn upload_time_forms_all_parse() {
    let want = parse_rfc3339z("2026-03-20T03:13:45Z");
    assert_eq!(parse_upload_time("2026-03-20T03:13:45Z"), want);
    assert_eq!(parse_upload_time("2026-03-20T03:13:45+00:00"), want);
    assert_eq!(parse_upload_time("2026-03-20T03:13:45"), want);
    assert_eq!(parse_upload_time("2026-03-20T03:13:45.123456Z"), want);
    assert_eq!(parse_upload_time("2026-03-20T03:13:45.123456+00:00"), want);
    assert_eq!(parse_upload_time("garbage"), None);
    assert_eq!(parse_upload_time(""), None);
}

#[test]
fn wheel_filename_maps_to_second_field() {
    assert_eq!(
        filename_version("requests-2.31.0-py3-none-any.whl", "requests"),
        Some("2.31.0")
    );
    assert_eq!(
        filename_version("foo-1.0.0+local-py3-none-any.whl", "foo"),
        Some("1.0.0+local")
    );
    // No dash before the version field -> unparseable.
    assert_eq!(filename_version("foo.whl", "foo"), None);
}

#[test]
fn sdist_filename_splits_at_the_project_name() {
    assert_eq!(
        filename_version("requests-2.31.0.tar.gz", "requests"),
        Some("2.31.0")
    );
    assert_eq!(
        filename_version("zope-interface-6.0.zip", "zope-interface"),
        Some("6.0")
    );
    assert_eq!(
        filename_version("foo-bar-1.2.3.tar.bz2", "foo-bar"),
        Some("1.2.3")
    );
    assert_eq!(filename_version("nodash.tar.gz", "foo"), None);
    // A version containing `-` survives: the split follows the project name,
    // not the last dash.
    assert_eq!(
        filename_version("foo-1.0.0-beta.1.tar.gz", "foo"),
        Some("1.0.0-beta.1")
    );
    // The filename's dist name is matched after PEP 503 normalization.
    assert_eq!(
        filename_version("Zope_Interface-6.0.tar.gz", "zope-interface"),
        Some("6.0")
    );
    // Eggs are downloadable, so they contribute versions too.
    assert_eq!(
        filename_version("foo-1.0.0-py2.7.egg", "foo"),
        Some("1.0.0")
    );
    // PEP 440 epochs appear in sdist names.
    assert_eq!(filename_version("foo-1!2.0.tar.gz", "foo"), Some("1!2.0"));
}

fn doc(files: serde_json::Value, versions: serde_json::Value) -> Value {
    json!({
        "meta": {"api-version": "1.0"},
        "name": "foo",
        "versions": versions,
        "files": files,
    })
}

fn file(filename: &str, upload_time: &str) -> Value {
    json!({
        "filename": filename,
        "url": format!("https://files.pythonhosted.org/packages/aa/bb/cc/{filename}"),
        "hashes": {"sha256": "00"},
        "upload-time": upload_time,
    })
}

const OLD: &str = "2000-01-01T00:00:00Z";
const NEW: &str = "2999-01-01T00:00:00Z";

fn cutoff() -> u64 {
    parse_rfc3339z("2026-01-01T00:00:00Z").unwrap()
}

#[test]
fn cutoff_drops_too_new_files_and_versions() {
    let mut d = doc(
        json!([file("foo-1.0.0.tar.gz", OLD), file("foo-2.0.0.tar.gz", NEW)]),
        json!(["1.0.0", "2.0.0"]),
    );
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());
    let files = d["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["filename"], "foo-1.0.0.tar.gz");
    assert_eq!(d["versions"], json!(["1.0.0"]));
}

#[test]
fn boundary_upload_time_is_kept() {
    let at_cutoff = "2026-01-01T00:00:00Z";
    let mut d = doc(
        json!([file("foo-1.0.0.tar.gz", at_cutoff)]),
        json!(["1.0.0"]),
    );
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());
    assert_eq!(d["files"].as_array().unwrap().len(), 1);
    // One second past the cutoff and it is dropped.
    let mut d = doc(
        json!([file("foo-1.0.0.tar.gz", "2026-01-01T00:00:01Z")]),
        json!(["1.0.0"]),
    );
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());
    assert!(d["files"].as_array().unwrap().is_empty());
    assert_eq!(d["versions"], json!([]));
}

#[test]
fn missing_upload_time_dropped_only_under_cooldown() {
    let bare = json!({
        "filename": "foo-1.0.0.tar.gz",
        "url": "https://files.pythonhosted.org/packages/aa/bb/cc/foo-1.0.0.tar.gz",
    });
    let mut d = doc(json!([bare.clone()]), json!(["1.0.0"]));
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());
    assert!(d["files"].as_array().unwrap().is_empty());

    let mut d = doc(json!([bare]), json!(["1.0.0"]));
    filter_simple_json(&mut d, None, "foo", &proxy_url());
    assert_eq!(d["files"].as_array().unwrap().len(), 1);
    assert_eq!(d["versions"], json!(["1.0.0"]));
}

#[test]
fn unparseable_upload_time_dropped_under_cooldown() {
    let mut d = doc(
        json!([file("foo-1.0.0.tar.gz", "not-a-date")]),
        json!(["1.0.0"]),
    );
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());
    assert!(d["files"].as_array().unwrap().is_empty());
}

#[test]
fn plus_zero_offset_upload_time_is_gated_not_dropped() {
    let mut d = doc(
        json!([file("foo-1.0.0.tar.gz", "2000-01-01T00:00:00+00:00")]),
        json!(["1.0.0"]),
    );
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());
    assert_eq!(d["files"].as_array().unwrap().len(), 1);
}

#[test]
fn version_survives_via_wheel_when_sdist_dropped() {
    let mut d = doc(
        json!([
            file("foo-1.0.0-py3-none-any.whl", OLD),
            file("foo-1.0.0.tar.gz", NEW),
            file("foo-2.0.0.tar.gz", NEW),
        ]),
        json!(["1.0.0", "2.0.0"]),
    );
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());
    assert_eq!(d["versions"], json!(["1.0.0"]));
}

#[test]
fn unparseable_filename_stays_and_deletes_no_version() {
    // The file survives the age gate, so nothing was filtered — the version
    // list must be left as upstream published it even though no filename maps
    // to it.
    let mut d = doc(json!([file("nodash.tar.gz", OLD)]), json!(["1.0.0"]));
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());
    assert_eq!(d["files"].as_array().unwrap().len(), 1);
    assert_eq!(d["versions"], json!(["1.0.0"]));
}

#[test]
fn no_cutoff_keeps_everything_and_rewrites() {
    let mut d = doc(
        json!([file("foo-1.0.0.tar.gz", OLD), file("foo-2.0.0.tar.gz", NEW)]),
        json!(["1.0.0", "2.0.0"]),
    );
    filter_simple_json(&mut d, None, "foo", &proxy_url());
    let files = d["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(d["versions"], json!(["1.0.0", "2.0.0"]));
    assert_eq!(
        files[0]["url"],
        "http://localhost:3080/pypi/files/foo/packages/aa/bb/cc/foo-1.0.0.tar.gz"
    );
}

#[test]
fn rewrite_keeps_hashes_untouched() {
    let mut d = doc(json!([file("foo-1.0.0.tar.gz", OLD)]), json!(["1.0.0"]));
    filter_simple_json(&mut d, None, "foo", &proxy_url());
    assert_eq!(d["files"][0]["hashes"], json!({"sha256": "00"}));
}

#[test]
fn rewrite_handles_relative_and_rooted_urls() {
    assert_eq!(
        rewrite_file_url("/packages/aa/bb/cc/f.whl", "foo", &proxy_url()),
        "http://localhost:3080/pypi/files/foo/packages/aa/bb/cc/f.whl"
    );
    assert_eq!(
        rewrite_file_url("packages/aa/bb/cc/f.whl", "foo", &proxy_url()),
        "http://localhost:3080/pypi/files/foo/packages/aa/bb/cc/f.whl"
    );
}

#[test]
fn doc_without_versions_key_filters_files_only() {
    let mut d = json!({"files": [file("foo-1.0.0.tar.gz", NEW)]});
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());
    assert!(d["files"].as_array().unwrap().is_empty());
    assert!(d.get("versions").is_none());
}

#[test]
fn versions_without_any_file_are_left_alone() {
    // Real indexes list versions whose files were all removed upstream (e.g.
    // urllib3 lists `0.2` with no files). Cooldown never touched them, so the
    // recompute must not quietly delete them.
    let mut d = doc(
        json!([
            {"filename": "foo-1.0.0.tar.gz", "url": "https://f.test/packages/aa/bb/cc/foo-1.0.0.tar.gz",
             "upload-time": "2000-01-01T00:00:00Z", "hashes": {}},
            {"filename": "foo-2.0.0.tar.gz", "url": "https://f.test/packages/aa/bb/cc/foo-2.0.0.tar.gz",
             "upload-time": "2999-01-01T00:00:00Z", "hashes": {}}
        ]),
        json!(["0.2", "1.0.0", "2.0.0"]),
    );
    filter_simple_json(&mut d, Some(cutoff()), "foo", &proxy_url());

    let versions: Vec<&str> = d["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // 2.0.0 filtered (its only file is too new); 0.2 has no files either way.
    assert_eq!(versions, ["0.2", "1.0.0"]);
}
