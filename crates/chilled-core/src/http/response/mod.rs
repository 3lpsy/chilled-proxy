//! Shared HTTP response builders. Error envelope *shapes* are per-registry and
//! live in each registry crate.

#[cfg(test)]
mod tests;

use axum::{body::Body, http::header, response::Response};
use bytes::Bytes;

/// HTTP Content-Type for JSON responses.
pub const JSON_CTYPE: &str = "application/json; charset=utf-8";

/// Builds an error response with an empty body.
pub fn error_response(status: u16) -> Response {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .expect("valid error response")
}

/// Builds a `405 Method Not Allowed` for the read-only registry surface.
pub fn method_not_allowed() -> Response {
    Response::builder()
        .status(405)
        .header(header::ALLOW, "GET, HEAD")
        .body(Body::empty())
        .expect("valid 405 response")
}

/// Builds a JSON response.
pub fn json_response(status: u16, body: String) -> Response {
    text_response(status, JSON_CTYPE, body)
}

/// Builds a text response with an explicit content type.
pub fn text_response(status: u16, ctype: &str, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, ctype)
        .body(Body::from(body))
        .expect("valid text response")
}

/// Builds a `200 OK` binary response (artifact downloads).
pub fn data_response(ctype: &str, data: Bytes) -> Response {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, ctype)
        .body(Body::from(data))
        .expect("valid data response")
}

/// Escapes a string for safe embedding inside a JSON string literal (RFC 8259).
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
