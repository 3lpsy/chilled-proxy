//! Simple-index content negotiation (PEP 691 JSON vs PEP 503 HTML).

use crate::constants::{HTML_CTYPE, SIMPLE_JSON_CTYPE, SIMPLE_JSON_LATEST_CTYPE};

/// The representation a simple-index response is served in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Json,
    Html,
}

impl Format {
    /// Single-letter representation tag used in ETag markers.
    pub(crate) fn tag(self) -> char {
        match self {
            Format::Json => 'j',
            Format::Html => 'h',
        }
    }

    /// The served `Content-Type`.
    pub(crate) fn ctype(self) -> &'static str {
        match self {
            Format::Json => SIMPLE_JSON_CTYPE,
            Format::Html => HTML_CTYPE,
        }
    }
}

/// Picks the response format from a client `Accept` header. Containment check
/// (q-values ignored): JSON when the header mentions a JSON simple type or
/// `application/json`, HTML otherwise (old pip sends `text/html` or nothing).
///
/// Media types are case-insensitive, and clients may ask for the version-
/// agnostic `...simple.latest+json` alias.
pub(crate) fn negotiate(accept: Option<&str>) -> Format {
    let Some(header) = accept else {
        return Format::Html;
    };
    let header = header.to_ascii_lowercase();
    let json = [
        SIMPLE_JSON_CTYPE,
        SIMPLE_JSON_LATEST_CTYPE,
        "application/json",
    ]
    .iter()
    .any(|ctype| header.contains(ctype));
    if json {
        Format::Json
    } else {
        Format::Html
    }
}

#[cfg(test)]
mod tests {
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
}
