//! `GET /healthz` — health-check endpoint.

use axum::{body::Body, http::header, response::Response};

/// Handles `GET /healthz`: the conventional contract — HTTP 200, plain `ok`.
pub(crate) async fn handle_healthz() -> Response {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from("ok\n"))
        .expect("valid healthz response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn healthz_is_ok_text() {
        let resp = handle_healthz().await;
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers()[header::CONTENT_TYPE],
            "text/plain; charset=utf-8"
        );
    }
}
