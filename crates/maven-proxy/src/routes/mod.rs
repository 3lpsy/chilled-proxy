//! Request routing: a single wildcard classified by path shape.

pub(crate) mod maven;

pub(crate) use maven::handle_maven;
