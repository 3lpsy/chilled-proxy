//! Cache-validator response headers for cooldown-marked bodies.

use axum::http::header;

use crate::cache::CacheEntry;

use super::{filtered_etag, Marker};

/// Attaches cooldown-aware cache validators: a weak marked ETag (and no
/// `Last-Modified`) for filtered bodies, the upstream validators otherwise.
pub fn cooldown_validators(
    mut builder: axum::http::response::Builder,
    entry: &CacheEntry,
    marker: Option<Marker>,
) -> axum::http::response::Builder {
    match marker {
        Some(marker) => {
            if let Some(etag) = entry.etag() {
                builder = builder.header(header::ETAG, filtered_etag(etag, marker));
            }
        }
        None => {
            if let Some(etag) = entry.etag() {
                builder = builder.header(header::ETAG, etag);
            }
            if let Some(last_modified) = entry.last_modified() {
                builder = builder.header(header::LAST_MODIFIED, last_modified);
            }
        }
    }
    builder
}
