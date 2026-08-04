//! The axum entry point: method check, classify, dispatch.

use axum::{
    extract::State,
    http::{HeaderMap, Method, Uri},
    response::Response,
};
use chilled_core::http::{error_response, method_not_allowed};
use chilled_core::valid::decode_path_once;
use log::{debug, warn};

use crate::routes::pypi::file::serve_file;
use crate::routes::pypi::route::{classify, Route};
use crate::routes::pypi::serve::{project_list, redirect_to_project, serve_project};
use crate::state::AppState;

/// Handles every request under the `/pypi` mount.
pub(crate) async fn handle_pypi(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return method_not_allowed();
    }
    let raw = uri.path();
    let Some(path) = decode_path_once(raw) else {
        warn!("proxy: rejected undecodable request path: {raw}");
        return error_response(404);
    };

    match classify(&path) {
        Route::ProjectList => project_list(&headers),
        Route::Project(name) => serve_project(&state, &name, &headers).await,
        Route::Redirect(name) => redirect_to_project(&state.config, &name),
        Route::File {
            project,
            fhp_path,
            filename,
        } => serve_file(&state, &project, &fhp_path, &filename).await,
        Route::NotFound => {
            debug!("proxy: unrecognized request path: {path}");
            error_response(404)
        }
    }
}
