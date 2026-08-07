//! Mount-path parsing for the per-registry `--<registry>-path` flags.

#[cfg(test)]
mod tests;

mod path;

#[cfg(test)]
pub(crate) use path::RESERVED;

pub(crate) use path::{check, parse};
