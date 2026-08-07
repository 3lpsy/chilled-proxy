//! Per-version documents, carved out of the filtered packument so hidden
//! versions stay hidden.

use axum::response::Response;
use chilled_core::http::{error_response, json_response};
use log::error;

use crate::http::format_npm_error;
use crate::model::{NpmEntry, PackageRef};
use crate::routes::npm::packument::{serve_packument, Served};
use crate::state::AppState;

/// Handles a version doc request (`GET /{name}/{version}`), derived from the
/// filtered packument so hidden versions stay hidden.
pub(super) async fn handle_version_doc(
    state: &AppState,
    pkg: &PackageRef,
    version: &str,
) -> Response {
    match serve_packument(state, pkg, NpmEntry::new(), false).await {
        Served::Body(_, body) => {
            let version = version.to_owned();
            let extracted =
                tokio::task::spawn_blocking(move || extract_version_doc(&body, &version)).await;
            match extracted {
                Ok(Some(doc)) => json_response(200, doc),
                Ok(None) => json_response(404, format_npm_error("Not found")),
                Err(err) => {
                    error!("proxy: version doc task failed for {pkg}: {err}");
                    error_response(500)
                }
            }
        }
        Served::Done(response) => response,
        // Unreachable: version doc requests carry no client validators.
        Served::NotModified(_) => error_response(500),
    }
}

/// Extracts one version object from a serialized (filtered) packument.
fn extract_version_doc(body: &[u8], version: &str) -> Option<String> {
    let doc: serde_json::Value = serde_json::from_slice(body).ok()?;
    let versions = doc.get("versions")?;
    // npm also resolves dist-tags here (`GET /pkg/latest`). The tags were
    // already repaired by the filter, so a tag can only name a served version.
    let entry = versions.get(version).or_else(|| {
        let target = doc.get("dist-tags")?.get(version)?.as_str()?;
        versions.get(target)
    })?;
    serde_json::to_string(entry).ok()
}
