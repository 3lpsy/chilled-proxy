//! npm-shaped HTTP helpers (the generic builders live in `chilled_core`).

#[cfg(test)]
mod tests;

use std::fmt::Display;

use chilled_core::http::json_escape;

/// Formats an npm registry JSON error body (`{"error":"..."}`).
pub(crate) fn format_npm_error(error: impl Display) -> String {
    format!(r#"{{"error":"{}"}}"#, json_escape(&error.to_string()))
}
