//! Simple-index content negotiation (PEP 691 JSON vs PEP 503 HTML).

#[cfg(test)]
mod tests;

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
