//! PEP 503 project-name validation/normalization and file-path shape checks.
//!
//! Project names and file paths are attacker-controlled and fed into
//! `Url::join` and cache paths; the PyPI charset closes SSRF and traversal.

mod files;
mod project;

#[cfg(test)]
mod tests;

pub(crate) use files::{distribution_name, is_valid_filename, validate_fhp_path};
pub(crate) use project::{is_valid_name, normalize};
