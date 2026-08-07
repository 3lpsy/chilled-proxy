//! Uncached verbatim forwarding (snapshot metadata, unfiltered checksums).

use axum::response::Response;
use bytes::Bytes;
use chilled_core::http::{data_response, read_capped, text_response, FetchError};
use log::{error, warn};

use crate::constants::TEXT_CTYPE;
use crate::routes::maven::handler::plain_error;
use crate::state::AppState;

/// Fetches `rel` from upstream and forwards it verbatim, uncached.
pub(crate) async fn pass_through(state: &AppState, rel: &str, ctype: &str) -> Response {
    let url = state
        .config
        .upstream_url
        .join(rel)
        .expect("validated segments join onto the pinned upstream URL");

    let mut response = match state.client.get(url).send().await {
        Ok(response) => response,
        Err(err) => {
            error!("fetch: pass-through connection failed for {rel}: {err}");
            return plain_error(502, "upstream fetch failed");
        }
    };

    let status = response.status().as_u16();
    if !response.status().is_success() {
        warn!("fetch: upstream returned HTTP status {status} for {rel}");
        let body = response.text().await.unwrap_or_default();
        return text_response(status, TEXT_CTYPE, body);
    }

    match read_capped(&mut response, state.config.settings.max_metadata_size).await {
        Ok(data) => data_response(ctype, Bytes::from(data)),
        Err(FetchError::TooLarge) => plain_error(507, "upstream response too large"),
        Err(FetchError::Http(err)) => {
            error!("fetch: pass-through read failed for {rel}: {err}");
            plain_error(502, "upstream fetch failed")
        }
    }
}
