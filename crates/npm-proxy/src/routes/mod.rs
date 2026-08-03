//! Request routing: a single fallback handler classifies every npm path.

pub(crate) mod npm;

pub(crate) use npm::handle_npm;
