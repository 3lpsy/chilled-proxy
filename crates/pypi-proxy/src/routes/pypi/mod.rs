//! PyPI route classification and handlers: `/simple/...` and `/files/...`.
//!
//! Split by role: path classification (`route`), the axum entry point
//! (`handler`), the simple-index serve ladder (`serve`), upstream fetching and
//! HTML normalization (`fetch`), and distribution files (`file`).

mod fetch;
mod file;
mod handler;
mod route;
mod serve;

pub(crate) use handler::handle_pypi;
