//! npm data models: validated package references (cache/upstream paths) and
//! cached packument response metadata.

#[cfg(test)]
mod tests;

use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use chilled_core::http::{fmt_http_date, parse_http_date};

use crate::valid;

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

/// Cached packument response metadata: HTTP validators and freshness.
#[derive(Clone, Debug, Default)]
pub(crate) struct NpmEntry {
    /// HTTP entity tag header.
    etag: Option<String>,
    /// Packument file modification time.
    mtime: Option<SystemTime>,
    /// Last upstream update check time.
    atime: Option<Instant>,
}

impl NpmEntry {
    /// Creates an empty metadata entry.
    pub(crate) fn new() -> Self {
        NpmEntry::default()
    }

    /// Checks if this entry describes the same body as `other`.
    pub(crate) fn is_equivalent(&self, other: &NpmEntry) -> bool {
        (self.etag().is_some() && (self.etag() == other.etag()))
            || (self.last_modified().is_some() && (self.last_modified() == other.last_modified()))
    }

    /// Checks if this entry is expired for the TTL given.
    pub(crate) fn is_expired_with_ttl(&self, ttl: &Duration) -> bool {
        self.atime.is_some_and(|atime| atime.elapsed() > *ttl)
    }

    /// Gets the HTTP entity tag metadata.
    pub(crate) fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// Gets the HTTP Last-Modified metadata.
    pub(crate) fn last_modified(&self) -> Option<String> {
        self.mtime.map(fmt_http_date)
    }

    /// Gets the file modification time metadata.
    pub(crate) fn mtime(&self) -> Option<SystemTime> {
        self.mtime
    }

    /// Sets the HTTP entity tag metadata.
    pub(crate) fn set_etag(&mut self, etag: &str) {
        self.etag = Some(etag.to_owned());
    }

    /// Sets the HTTP Last-Modified metadata.
    pub(crate) fn set_last_modified(&mut self, last_modified: &str) {
        self.mtime = parse_http_date(last_modified);
    }

    /// Sets the file modification time metadata.
    pub(crate) fn set_mtime(&mut self, mtime: SystemTime) {
        self.mtime = Some(mtime);
    }

    /// Updates the last upstream access time metadata.
    pub(crate) fn set_last_updated(&mut self) {
        self.atime = Some(Instant::now());
    }
}
