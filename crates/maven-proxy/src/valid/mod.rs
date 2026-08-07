//! Maven request-path validation and classification types.
//!
//! Attacker-controlled path segments feed `Url::join` and cache paths; a
//! conservative charset (no leading dot, `:`, `@`, or separators) closes
//! SSRF and traversal.

mod request;
mod rules;

#[cfg(test)]
mod tests;

pub(crate) use request::MavenRequest;
pub(crate) use rules::{is_artifact_file, is_dir_segment, is_file_segment, is_version};
