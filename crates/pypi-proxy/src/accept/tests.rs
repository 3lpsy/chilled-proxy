use super::*;

#[test]
fn pip_json_accept_negotiates_json() {
    // Modern pip's exact Accept header.
    let pip = "application/vnd.pypi.simple.v1+json, \
               application/vnd.pypi.simple.v1+html;q=0.1, text/html;q=0.01";
    assert_eq!(negotiate(Some(pip)), Format::Json);
    assert_eq!(
        negotiate(Some("application/vnd.pypi.simple.v1+json")),
        Format::Json
    );
    assert_eq!(negotiate(Some("application/json")), Format::Json);
}

#[test]
fn html_or_absent_accept_negotiates_html() {
    assert_eq!(negotiate(Some("text/html")), Format::Html);
    assert_eq!(negotiate(Some("*/*")), Format::Html);
    assert_eq!(
        negotiate(Some("application/vnd.pypi.simple.v1+html")),
        Format::Html
    );
    assert_eq!(negotiate(None), Format::Html);
}

#[test]
fn format_tags_and_ctypes() {
    assert_eq!(Format::Json.tag(), 'j');
    assert_eq!(Format::Html.tag(), 'h');
    assert_eq!(Format::Json.ctype(), SIMPLE_JSON_CTYPE);
    assert_eq!(Format::Html.ctype(), HTML_CTYPE);
}

#[test]
fn negotiation_is_case_insensitive() {
    // Media types are case-insensitive per RFC 9110.
    assert_eq!(negotiate(Some("APPLICATION/JSON")), Format::Json);
    assert_eq!(
        negotiate(Some("Application/Vnd.PyPI.Simple.V1+JSON")),
        Format::Json
    );
}

#[test]
fn latest_alias_is_json() {
    assert_eq!(
        negotiate(Some("application/vnd.pypi.simple.latest+json")),
        Format::Json
    );
}
