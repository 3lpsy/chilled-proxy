//! crates.io-shaped HTTP helpers (the generic builders live in `chilled_core`).

#[cfg(test)]
mod tests;

use std::fmt::Display;

use chilled_core::http::json_escape;

/// Formats a crates.io API JSON error body (`{"errors":[{"detail":...}]}`).
pub(crate) fn format_json_error(error: impl Display) -> String {
    format!(
        r#"{{"errors":[{{"detail":"{}"}}]}}"#,
        json_escape(&error.to_string())
    )
}
