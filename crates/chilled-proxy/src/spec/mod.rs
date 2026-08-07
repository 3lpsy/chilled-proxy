//! `--<registry>-mount` spec parsing: one spec describes one extra mount of a
//! registry as comma-separated `key=value` pairs, e.g.
//! `--maven-mount name=plugins,path=/gradle-plugins,upstream=https://plugins.gradle.org/m2/`.
//! Only `name` is required; everything else falls back to the registry's flags.

#[cfg(test)]
mod tests;

mod parse;
mod types;

pub(crate) use parse::parse;
pub(crate) use types::MountSpec;
