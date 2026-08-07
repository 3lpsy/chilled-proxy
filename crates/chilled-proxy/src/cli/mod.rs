//! CLI/env parsing and per-registry settings resolution.
//!
//! General knobs (`--cooldown`, ...) apply to every registry; per-registry and
//! per-mount variants override them. Env vars use the `CHILLED_*` prefix.

#[cfg(test)]
mod tests;

mod args;
mod instances;
pub(crate) mod settings;
mod ui;

pub use args::Cli;
pub use instances::{RegistryInstance, ResolvedConfig};
