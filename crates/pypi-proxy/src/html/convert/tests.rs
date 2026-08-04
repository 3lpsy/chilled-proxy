use super::*;

fn base() -> Url {
    Url::parse("https://example.test/simple/torch/").unwrap()
}

fn parse(body: &str) -> Value {
    parse_simple_html(body, "torch", &base())
}

fn files(doc: &Value) -> Vec<Value> {
    doc["files"].as_array().cloned().unwrap_or_default()
}

#[test]
fn builds_a_pep691_document() {
    let doc = parse(r#"<a href="https://f.test/torch-1.0.whl#sha256=abc">torch-1.0.whl</a>"#);
    assert_eq!(doc["meta"]["api-version"], "1.0");
    assert_eq!(doc["name"], "torch");
    let files = files(&doc);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["filename"], "torch-1.0.whl");
    // The fragment becomes the hash map and is stripped from the URL.
    assert_eq!(files[0]["url"], "https://f.test/torch-1.0.whl");
    assert_eq!(files[0]["hashes"]["sha256"], "abc");
}

#[test]
fn resolves_relative_hrefs_against_the_index_url() {
    // PEP 503 permits relative links; leaving them relative would send the
    // rewritten download URL to the wrong place.
    let doc = parse(r#"<a href="../../packages/aa/bb/torch-1.0.whl">torch-1.0.whl</a>"#);
    assert_eq!(
        files(&doc)[0]["url"],
        "https://example.test/packages/aa/bb/torch-1.0.whl"
    );
}

#[test]
fn reads_upload_time_which_is_what_makes_gating_possible() {
    let doc = parse(r#"<a href="t-1.0.whl" data-upload-time="2026-01-23T15:10:22Z">t-1.0.whl</a>"#);
    assert_eq!(files(&doc)[0]["upload-time"], "2026-01-23T15:10:22Z");
    assert!(has_upload_times(&doc));
}

#[test]
fn an_index_without_upload_times_is_reported_as_undatable() {
    let doc = parse(r#"<a href="t-1.0.whl">t-1.0.whl</a>"#);
    assert!(files(&doc)[0].get("upload-time").is_none());
    assert!(!has_upload_times(&doc));
    // An empty index is undatable too, rather than vacuously datable.
    assert!(!has_upload_times(&parse("<html></html>")));
}

#[test]
fn carries_requires_python_yanked_and_core_metadata() {
    let doc = parse(
        r#"<a href="t-1.0.whl" data-requires-python="&gt;=3.9" data-yanked="broken"
             data-core-metadata="sha256=deadbeef">t-1.0.whl</a>"#,
    );
    let f = &files(&doc)[0];
    assert_eq!(f["requires-python"], ">=3.9");
    assert_eq!(f["yanked"], "broken");
    assert_eq!(f["core-metadata"]["sha256"], "deadbeef");
}

#[test]
fn a_bare_yanked_attribute_means_yanked_without_a_reason() {
    let doc = parse(r#"<a href="t-1.0.whl" data-yanked>t-1.0.whl</a>"#);
    assert_eq!(files(&doc)[0]["yanked"], Value::Bool(true));
    // Absent entirely means not yanked, not "yanked: false".
    let doc = parse(r#"<a href="t-1.0.whl">t-1.0.whl</a>"#);
    assert!(files(&doc)[0].get("yanked").is_none());
}

#[test]
fn accepts_the_pep714_metadata_spelling() {
    let doc = parse(r#"<a href="t-1.0.whl" data-dist-info-metadata="sha256=aa">t-1.0.whl</a>"#);
    assert_eq!(files(&doc)[0]["core-metadata"]["sha256"], "aa");
    // A bare attribute means "available" without naming a digest.
    let doc = parse(r#"<a href="t-1.0.whl" data-core-metadata>t-1.0.whl</a>"#);
    assert_eq!(files(&doc)[0]["core-metadata"], Value::Bool(true));
}

#[test]
fn unescapes_entities_in_hrefs_and_filenames() {
    let doc = parse(r#"<a href="t-1.0%2Bcpu.whl?a=1&amp;b=2">t-1.0&#x2B;cpu.whl</a>"#);
    let f = &files(&doc)[0];
    assert_eq!(f["filename"], "t-1.0+cpu.whl");
    assert_eq!(
        f["url"],
        "https://example.test/simple/torch/t-1.0%2Bcpu.whl?a=1&b=2"
    );
}

#[test]
fn falls_back_to_the_url_segment_when_link_text_is_empty() {
    let doc = parse(r#"<a href="https://f.test/pkgs/t-9.9.whl"></a>"#);
    assert_eq!(files(&doc)[0]["filename"], "t-9.9.whl");
}

#[test]
fn skips_unusable_anchors_without_dropping_the_page() {
    // A navigation link with no href sits between two real entries.
    let doc = parse(
        r#"<a href="a-1.0.whl">a-1.0.whl</a><a name="top">top</a><a href="b-1.0.whl">b-1.0.whl</a>"#,
    );
    let files = files(&doc);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["filename"], "a-1.0.whl");
    assert_eq!(files[1]["filename"], "b-1.0.whl");
}

#[test]
fn recognizes_both_html_content_types() {
    assert!(is_html_simple("text/html"));
    assert!(is_html_simple("text/html; charset=utf-8"));
    assert!(is_html_simple("application/vnd.pypi.simple.v1+html"));
    assert!(!is_html_simple("application/vnd.pypi.simple.v1+json"));
    assert!(!is_html_simple("application/octet-stream"));
}

#[test]
fn parses_a_pytorch_shaped_page() {
    // The real shape: absolute CDN href, percent-encoded local version, hash
    // fragment, and the upload time that makes the mount gate-able.
    let doc = parse(
        r#"<!DOCTYPE html><html><body><h1>Links for torch</h1>
        <a href="https://download-r2.pytorch.org/whl/cpu/torch-2.10.0%2Bcpu-cp312-cp312-linux_aarch64.whl#sha256=8de5a3"
           data-core-metadata="sha256=d6031f" data-upload-time="2026-01-23T15:10:22Z">torch-2.10.0+cpu-cp312-cp312-linux_aarch64.whl</a><br/>
        </body></html>"#,
    );
    let f = &files(&doc)[0];
    assert_eq!(
        f["filename"],
        "torch-2.10.0+cpu-cp312-cp312-linux_aarch64.whl"
    );
    assert_eq!(
        f["url"],
        "https://download-r2.pytorch.org/whl/cpu/torch-2.10.0%2Bcpu-cp312-cp312-linux_aarch64.whl"
    );
    assert_eq!(f["hashes"]["sha256"], "8de5a3");
    assert_eq!(f["upload-time"], "2026-01-23T15:10:22Z");
}
