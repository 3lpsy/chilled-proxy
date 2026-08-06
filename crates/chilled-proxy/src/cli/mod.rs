//! CLI/env parsing and per-registry settings resolution.
//!
//! General knobs (`--cooldown`, `--cache-ttl`, ...) apply to every registry;
//! per-registry variants (`--cooldown-npm`, ...) override them. Env vars use
//! the `CHILLED_*` prefix (flag name uppercased, dashes to underscores).
//!
//! Split by role: the flag declarations (`args`), turning them into mounted
//! instances (`instances`), and resolving one instance's settings against the
//! general/per-registry/per-mount fall-back chain (`settings`).

#[cfg(test)]
mod tests;

mod args;
mod instances;
pub(crate) mod settings;

pub use args::Cli;
pub use instances::{RegistryInstance, ResolvedConfig};
