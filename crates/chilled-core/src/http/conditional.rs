//! Conditional upstream metadata fetch shared by every registry proxy.

use axum::http::header;
use url::Url;

use crate::cache::CacheEntry;
use crate::http::{read_capped, FetchError};

/// Result of a conditional upstream fetch.
pub struct ConditionalResponse {
    /// Upstream HTTP response status code.
    pub status: u16,
    /// Upstream `Content-Type` (empty when absent/unreadable).
    pub ctype: String,
    /// Upstream HTTP response body.
    pub data: Vec<u8>,
}

/// Performs a conditional `GET` for a metadata document, driving `entry`'s
/// validators: sends `If-None-Match` (else `If-Modified-Since`) from the entry
/// and harvests the response `ETag` / `Last-Modified` back into it.
///
/// Identity encoding is pinned unconditionally: a compressed body would fail
/// downstream parsing/filtering and could pass through unfiltered, silently
/// disabling age-gating.
pub async fn conditional_get(
    client: &reqwest::Client,
    url: Url,
    accept: Option<&str>,
    entry: &mut CacheEntry,
    max_size: usize,
) -> Result<ConditionalResponse, FetchError> {
    let mut request = client.get(url).header(header::ACCEPT_ENCODING, "identity");
    if let Some(accept) = accept {
        request = request.header(header::ACCEPT, accept);
    }
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

    let data = read_capped(&mut response, max_size).await?;
    Ok(ConditionalResponse {
        status,
        ctype,
        data,
    })
}
