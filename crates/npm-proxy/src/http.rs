//! npm-shaped HTTP helpers (the generic builders live in `chilled_core`).

use std::fmt::Display;

use chilled_core::http::json_escape;

/// Formats an npm registry JSON error body (`{"error":"..."}`).
pub(crate) fn format_npm_error(error: impl Display) -> String {
    format!(r#"{{"error":"{}"}}"#, json_escape(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_error_envelope() {
        assert_eq!(format_npm_error("Not found"), r#"{"error":"Not found"}"#);
    }

    #[test]
    fn escapes_error_message() {
        assert_eq!(
            format_npm_error("bad \"quote\"\nnewline"),
            r#"{"error":"bad \"quote\"\nnewline"}"#
        );
    }
}
