//! `GET /healthz` — health-check endpoint.

#[cfg(test)]
mod tests;

use axum::{body::Body, http::header, response::Response};

/// Handles `GET /healthz`: the conventional contract — HTTP 200, plain `ok`.
pub(crate) async fn handle_healthz() -> Response {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from("ok\n"))
        .expect("valid healthz response")
}
