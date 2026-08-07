//! The upstream packument fetch (conditional GET).

use chilled_core::http::{conditional_get, FetchError};

use crate::constants::PACKUMENT_CTYPE;
use crate::model::{NpmEntry, PackageRef};
use crate::state::AppState;

/// Packument download result.
pub(super) struct PackumentResponse {
    /// Entry with updated response metadata (etag / last-modified).
    pub(super) entry: NpmEntry,
    /// Upstream HTTP response status code.
    pub(super) status: u16,
    /// Upstream HTTP response body.
    pub(super) data: Vec<u8>,
}

/// Downloads a full packument, sending the conditional headers from `entry`.
pub(super) async fn download_packument(
    state: &AppState,
    mut entry: NpmEntry,
    pkg: &PackageRef,
) -> Result<PackumentResponse, FetchError> {
    // Charset-validated names cannot break `Url::join`.
    let url = state
        .config
        .upstream_url
        .join(&pkg.upstream_packument_rel())
        .expect("validated packument URL");

    // Full doc only — the abbreviated "corgi" form lacks the `time` map needed
    // for cooldown.
    let response = conditional_get(
        &state.client,
        url,
        Some(PACKUMENT_CTYPE),
        &mut entry,
        state.config.settings.max_metadata_size,
    )
    .await?;

    Ok(PackumentResponse {
        entry,
        status: response.status,
        data: response.data,
    })
}
