//! Registry-agnostic machinery shared by every chilled-proxy registry proxy:
//! cooldown math, cache primitives, ETag markers, HTTP plumbing, and serving.

pub mod cache;
pub mod config;
pub mod cooldown;
pub mod etag;
pub mod http;
pub mod registry;
pub mod serve;
pub mod state;
pub mod time;
pub mod valid;
