//! The axum entry point: method check, classify, dispatch.

use axum::{
    extract::State,
    http::{HeaderMap, Method, Uri},
    response::Response,
};
use chilled_core::http::method_not_allowed;
use log::warn;

use crate::constants::{TEXT_CTYPE, XML_CTYPE};
use crate::routes::maven::artifact::serve_artifact;
use crate::routes::maven::metadata::{pass_through, serve_metadata};
use crate::routes::maven::route::classify;
use crate::state::AppState;
use crate::valid::MavenRequest;

/// Builds a plain-text error response.
pub(super) fn plain_error(status: u16, msg: &str) -> Response {
    chilled_core::http::text_response(status, TEXT_CTYPE, msg.to_owned())
}

/// Handles any request under the Maven mount.
pub(crate) async fn handle_maven(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return method_not_allowed();
    }
    let Some(request) = classify(uri.path()) else {
        warn!(
            "proxy: unrecognized or invalid repository path: {}",
            uri.path()
        );
        return plain_error(404, "not found");
    };

    match request {
        MavenRequest::Metadata { coords, algo } => {
            serve_metadata(&state, &coords, algo, &headers).await
        }
        MavenRequest::SnapshotMetadata { rel } => {
            let ctype = if rel.ends_with(".xml") {
                XML_CTYPE
            } else {
                TEXT_CTYPE
            };
            pass_through(&state, &rel, ctype).await
        }
        MavenRequest::Artifact {
            coords,
            version,
            file,
        } => serve_artifact(&state, &coords, &version, &file).await,
    }
}
