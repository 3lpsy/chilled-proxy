//! The [`PackageRef`] type and the cached packument entry alias.

use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use crate::valid;

/// Cached packument response metadata: HTTP validators and freshness.
pub(crate) type NpmEntry = chilled_core::cache::CacheEntry;

/// A validated npm package reference (optionally scoped).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackageRef {
    /// Scope without the leading `@`, if any.
    scope: Option<String>,
    /// Unscoped package name.
    name: String,
}

impl Display for PackageRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.scope {
            Some(scope) => write!(f, "@{scope}/{}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

impl PackageRef {
    /// Builds a validated reference; `scope` is given without its `@`.
    pub(crate) fn new(scope: Option<&str>, name: &str) -> Option<Self> {
        if !valid::is_name_part(name) {
            return None;
        }
        if let Some(scope) = scope {
            // The full `@scope/name` must stay within npm's 214-char cap.
            if !valid::is_name_part(scope) || scope.len() + name.len() + 2 > valid::MAX_NAME_LEN {
                return None;
            }
        }
        Some(PackageRef {
            scope: scope.map(str::to_owned),
            name: name.to_owned(),
        })
    }

    /// The full registry name (`name` or `@scope/name`).
    pub(crate) fn full_name(&self) -> String {
        self.to_string()
    }

    /// The unscoped name (tarball files are named after it).
    pub(crate) fn unscoped(&self) -> &str {
        &self.name
    }

    /// Relative packument cache path (under the packuments dir).
    pub(crate) fn packument_rel(&self) -> PathBuf {
        match &self.scope {
            Some(scope) => PathBuf::from(format!("@{scope}")).join(&self.name),
            None => PathBuf::from(&self.name),
        }
    }

    /// Relative tarball cache path (under the tarballs dir).
    pub(crate) fn tarball_rel(&self, file: &str) -> PathBuf {
        self.packument_rel().join(file)
    }

    /// Upstream packument path, relative to the registry root.
    pub(crate) fn upstream_packument_rel(&self) -> String {
        self.full_name()
    }

    /// Upstream tarball path, relative to the registry root.
    pub(crate) fn upstream_tarball_rel(&self, file: &str) -> String {
        format!("{self}/-/{file}")
    }
}
