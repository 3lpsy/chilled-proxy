//! The axum entry point: method check, classify, dispatch.

use axum::{
    extract::State,
    http::{HeaderMap, Method, Uri},
    response::Response,
};
use chilled_core::http::{error_response, method_not_allowed};
use log::warn;

use crate::routes::npm::packument::{handle_packument, handle_version_doc};
use crate::routes::npm::route::{parse_request, NpmRequest};
use crate::routes::npm::tarball::handle_tarball;
use crate::state::AppState;

pub(crate) async fn handle_npm(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return method_not_allowed();
    }
    let Some(request) = parse_request(uri.path()) else {
        warn!("proxy: malformed npm path: {}", uri.path());
        return error_response(404);
    };
    match request {
        NpmRequest::Packument(pkg) => handle_packument(&state, &headers, &pkg).await,
        NpmRequest::VersionDoc(pkg, version) => handle_version_doc(&state, &pkg, &version).await,
        NpmRequest::Tarball(pkg, file, version) => {
            handle_tarball(&state, &pkg, &file, &version).await
        }
    }
}
