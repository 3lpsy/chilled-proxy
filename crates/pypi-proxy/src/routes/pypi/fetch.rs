//! Fetching a simple index upstream, normalizing HTML to the JSON model.

use axum::{
    body::Body,
    http::{header, HeaderValue},
    response::Response,
};
use chilled_core::http::{read_capped, FetchError};
use log::debug;
use serde_json::Value;

use crate::constants::{SIMPLE_HTML_CTYPE, SIMPLE_JSON_CTYPE};
use crate::html::{has_upload_times, is_html_simple, parse_simple_html};
use crate::model::PypiEntry;
use crate::state::AppState;

/// Simple-index download result.
pub(super) struct SimpleResponse {
    /// Entry plus updated response metadata (etag / last-modified).
    pub(super) entry: PypiEntry,
    /// Upstream HTTP response status code.
    pub(super) status: u16,
    /// Upstream `Content-Type` (empty when absent/unreadable).
    pub(super) ctype: String,
    /// Upstream HTTP response body.
    pub(super) data: Vec<u8>,
    /// Set when an HTML index carried no upload times, so a cooldown on it
    /// cannot be honored.
    pub(super) undatable: bool,
}

/// Whether an upstream `Content-Type` is the PEP 691 JSON simple type.
pub(super) fn is_json_simple(ctype: &str) -> bool {
    ctype
        .split(';')
        .next()
        .is_some_and(|t| t.trim().eq_ignore_ascii_case(SIMPLE_JSON_CTYPE))
}

/// Downloads a project's simple index from upstream (always requesting PEP 691
/// JSON), sending the conditional-request headers carried by `entry`.
pub(super) async fn download_simple(
    state: &AppState,
    mut entry: PypiEntry,
) -> Result<SimpleResponse, FetchError> {
    let url = state
        .config
        .upstream_url
        .join(&format!("{}/", entry.name()))
        .expect("valid normalized project URL");

    // Pin identity encoding so the cap and cache see the real bytes.
    let mut request = state
        .client
        .get(url.clone())
        .header(header::ACCEPT, upstream_accept())
        .header(header::ACCEPT_ENCODING, "identity");
    if let Some(etag) = entry.etag() {
        request = request.header(header::IF_NONE_MATCH, etag);
    } else if let Some(last_modified) = entry.last_modified() {
        request = request.header(header::IF_MODIFIED_SINCE, last_modified);
    }

    let mut response = request.send().await.map_err(FetchError::Http)?;
    let status = response.status().as_u16();
    let ctype = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();

    if let Some(etag) = response
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
    {
        entry.set_etag(etag);
    }
    if let Some(last_modified) = response
        .headers()
        .get(header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
    {
        entry.set_last_modified(last_modified);
    }
    entry.set_last_updated();

    let data = read_capped(&mut response, state.config.settings.max_metadata_size).await?;

    // An HTML index is normalized to the PEP 691 model right here, so filtering,
    // URL rewriting, caching, and rendering downstream never learn that upstream
    // spoke a different dialect. `undatable` records the one thing normalizing
    // cannot supply: an index whose anchors carry no upload times at all.
    if is_html_simple(&ctype) {
        let body = String::from_utf8_lossy(&data);
        let doc = parse_simple_html(&body, entry.name(), &url);
        let undatable = !has_upload_times(&doc);
        debug!(
            "fetch: normalized HTML simple index for {} ({} file(s), datable: {})",
            entry.name(),
            doc.get("files")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            !undatable
        );
        return Ok(SimpleResponse {
            entry,
            status,
            ctype: SIMPLE_JSON_CTYPE.to_owned(),
            data: serde_json::to_vec(&doc).unwrap_or_default(),
            undatable,
        });
    }

    Ok(SimpleResponse {
        entry,
        status,
        ctype,
        data,
        undatable: false,
    })
}

/// The `Accept` sent upstream: PEP 691 JSON preferred, HTML accepted so an
/// HTML-only index can still be normalized and gated rather than refused.
fn upstream_accept() -> String {
    format!("{SIMPLE_JSON_CTYPE}, {SIMPLE_HTML_CTYPE};q=0.2, text/html;q=0.1")
}

/// Passes a non-JSON upstream body through verbatim (no cooldown active).
pub(super) fn passthrough_response(ctype: &str, data: Vec<u8>) -> Response {
    let ctype = HeaderValue::from_str(ctype)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, ctype)
        .header(header::VARY, "Accept")
        .body(Body::from(data))
        .expect("valid passthrough response")
}

/// Fetches a project index from upstream (or stale cache) and serves it.
///
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_simple_ctype_detection() {
        assert!(is_json_simple("application/vnd.pypi.simple.v1+json"));
        assert!(is_json_simple(
            "application/vnd.pypi.simple.v1+json; charset=utf-8"
        ));
        assert!(is_json_simple("Application/VND.pypi.simple.v1+JSON"));
        assert!(!is_json_simple("text/html"));
        assert!(!is_json_simple("application/json"));
        assert!(!is_json_simple(""));
    }
}
