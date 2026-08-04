//! crates.io-shaped HTTP helpers (the generic builders live in `chilled_core`).

use std::fmt::Display;

use chilled_core::http::json_escape;

/// Formats a crates.io API JSON error body (`{"errors":[{"detail":...}]}`).
pub(crate) fn format_json_error(error: impl Display) -> String {
    format!(
        r#"{{"errors":[{{"detail":"{}"}}]}}"#,
        json_escape(&error.to_string())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_body_is_well_formed() {
        let body = format_json_error("bad \"quote\" and \\slash");
        assert_eq!(
            body,
            r#"{"errors":[{"detail":"bad \"quote\" and \\slash"}]}"#
        );
    }
}
