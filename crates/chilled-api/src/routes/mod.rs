//! /api route handlers and router assembly.

pub(crate) mod artifacts;
pub(crate) mod config_view;
pub(crate) mod logs;
pub(crate) mod meta;
pub(crate) mod purge;
pub(crate) mod registries;
mod router;
pub(crate) mod session;
pub(crate) mod setup;
pub(crate) mod snapshots;
pub(crate) mod users;

pub(crate) use router::{api_error, api_router};
