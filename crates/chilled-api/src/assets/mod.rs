//! Serving the embedded web UI under /ui. The compiled frontend (repo-root
//! `dist/`) is embedded at build time; `--ui-dev-dist-dir` serves from disk.

mod serve;
#[cfg(test)]
mod tests;

pub(crate) use serve::router;
