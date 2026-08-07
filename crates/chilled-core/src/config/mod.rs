//! Per-registry runtime settings and small config parsing helpers.

#[cfg(test)]
mod tests;

mod parse;
mod settings;

pub use parse::{normalize_log_level, parse_overrides, parse_size};
pub use settings::RegistrySettings;
