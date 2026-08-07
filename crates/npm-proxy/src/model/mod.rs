//! npm data models: validated package references (cache/upstream paths) and
//! cached packument response metadata.

mod package_ref;

#[cfg(test)]
mod tests;

pub(crate) use package_ref::{NpmEntry, PackageRef};
