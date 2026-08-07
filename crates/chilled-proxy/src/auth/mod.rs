//! Per-mount upstream credentials and custom headers, installed as the default
//! headers of the mount's HTTP client so every upstream request carries them.
//! Sources, per mount name: `CHILLED_<NAME>_BASIC_AUTH_USERNAME`/`_PASSWORD`
//! and `CHILLED_<NAME>_HEADERS`, or `--upstream-basic-auth`/`--upstream-header`.

#[cfg(test)]
mod tests;

mod env;
mod resolve;
mod upstream;

#[cfg(test)]
pub(crate) use resolve::base64;

pub(crate) use env::{env_token, EnvSource, ProcessEnv, ENV_SUFFIXES};
pub(crate) use resolve::{parse_basic_spec, parse_header_spec, resolve};
pub use upstream::UpstreamAuth;
