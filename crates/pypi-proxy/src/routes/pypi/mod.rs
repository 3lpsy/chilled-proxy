//! PyPI route classification and handlers: `/simple/...` and `/files/...`.
//!
//! Split by role: classification (`route`), entry point (`handler`), serve
//! ladder (`serve`), responses (`respond`), cache (`cache`), fetch (`fetch`),
//! and distribution files (`file`).

mod cache;
mod fetch;
mod file;
mod handler;
mod respond;
mod route;
mod serve;

pub(crate) use handler::handle_pypi;
