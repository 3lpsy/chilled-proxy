use super::*;
use serde_json::json;

#[test]
fn escape_covers_all_five_entities() {
    assert_eq!(html_escape(r#"&<>"'"#), "&amp;&lt;&gt;&quot;&#x27;");
    assert_eq!(html_escape("plain-1.0"), "plain-1.0");
}

#[test]
fn golden_full_page() {
    let doc = json!({
        "files": [{
            "filename": "foo-1.0.0-py3-none-any.whl",
            "url": "http://localhost:3080/pypi/files/foo/packages/aa/bb/cc/foo-1.0.0-py3-none-any.whl",
            "hashes": {"sha256": "deadbeef"},
            "requires-python": ">=3.8",
        }],
    });
    let want = concat!(
        "<!DOCTYPE html><html><head><meta name=\"pypi:repository-version\" content=\"1.0\">",
        "<title>Links for foo</title></head><body><h1>Links for foo</h1>\n",
        "<a href=\"http://localhost:3080/pypi/files/foo/packages/aa/bb/cc/foo-1.0.0-py3-none-any.whl#sha256=deadbeef\"",
        " data-requires-python=\"&gt;=3.8\">foo-1.0.0-py3-none-any.whl</a><br/>\n",
        "</body></html>",
    );
    assert_eq!(render_html(&doc, "foo"), want);
}

#[test]
fn no_sha256_omits_fragment() {
    let doc = json!({
        "files": [{
            "filename": "foo-1.0.0.tar.gz",
            "url": "u",
            "hashes": {"md5": "aa"},
        }],
    });
    let html = render_html(&doc, "foo");
    assert!(html.contains("<a href=\"u\">foo-1.0.0.tar.gz</a><br/>"));
    assert!(!html.contains("#sha256="));
}

#[test]
fn yanked_bool_and_reason() {
    let doc = json!({
        "files": [
            {"filename": "a-1.tar.gz", "url": "u1", "yanked": true},
            {"filename": "a-2.tar.gz", "url": "u2", "yanked": "broken <dist>"},
            {"filename": "a-3.tar.gz", "url": "u3", "yanked": false},
        ],
    });
    let html = render_html(&doc, "a");
    assert!(html.contains("<a href=\"u1\" data-yanked=\"\">a-1.tar.gz</a>"));
    assert!(html.contains("<a href=\"u2\" data-yanked=\"broken &lt;dist&gt;\">a-2.tar.gz</a>"));
    assert!(html.contains("<a href=\"u3\">a-3.tar.gz</a>"));
}

#[test]
fn empty_or_missing_files_renders_bare_page() {
    let html = render_html(&json!({"files": []}), "foo");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.ends_with("</body></html>"));
    assert!(!html.contains("<a "));
    // No files key at all behaves the same.
    assert_eq!(render_html(&json!({}), "foo"), html);
}

#[test]
fn malformed_file_entries_are_skipped() {
    let doc = json!({"files": [{"url": "u"}, {"filename": "f-1.tar.gz"}, "junk"]});
    assert!(!render_html(&doc, "foo").contains("<a "));
}
