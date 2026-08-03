//! Capped upstream body reads.

#[cfg(test)]
mod tests;

use std::fmt::Display;

/// An upstream fetch failure: transport/decode error or an oversized body.
pub enum FetchError {
    /// Connection, TLS, or decode failure.
    Http(reqwest::Error),
    /// Body exceeded the size limit (declared or observed).
    TooLarge,
}

impl Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Http(e) => write!(f, "{e}"),
            FetchError::TooLarge => f.write_str("response body too large"),
        }
    }
}

/// Reads a response body into memory, capped at `max` bytes.
///
/// Rejects up front when `Content-Length` exceeds the cap, and again while
/// streaming, so a chunked response cannot exhaust memory. Errors rather than
/// truncating, so callers never serve a partial body.
pub async fn read_capped(
    response: &mut reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, FetchError> {
    if let Some(len) = response.content_length() {
        if len as usize > max {
            return Err(FetchError::TooLarge);
        }
    }

    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(FetchError::Http)? {
        if data.len().saturating_add(chunk.len()) > max {
            return Err(FetchError::TooLarge);
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}
