//! Classifying a raw npm request path with exactly one percent-decode.

use chilled_core::valid::decode_path_once;

use crate::model::PackageRef;
use crate::valid;

/// A classified npm request (after exactly one percent-decode).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NpmRequest {
    /// Full package document.
    Packument(PackageRef),
    /// Single version document, derived from the filtered packument.
    VersionDoc(PackageRef, String),
    /// Tarball download: the file (`{unscoped}-{version}.tgz`) and its version.
    Tarball(PackageRef, String, String),
}

/// Decodes a raw request path exactly once and classifies it.
pub(crate) fn parse_request(raw_path: &str) -> Option<NpmRequest> {
    let raw = raw_path.strip_prefix('/').unwrap_or(raw_path);
    classify(&decode_path_once(raw)?)
}

/// Classifies a decoded path: packument, version doc, or tarball.
fn classify(path: &str) -> Option<NpmRequest> {
    let segments: Vec<&str> = path.split('/').collect();
    let (pkg, rest) = if let Some(scope) = segments[0].strip_prefix('@') {
        if segments.len() < 2 {
            return None;
        }
        (PackageRef::new(Some(scope), segments[1])?, &segments[2..])
    } else {
        (PackageRef::new(None, segments[0])?, &segments[1..])
    };
    match rest {
        [] => Some(NpmRequest::Packument(pkg)),
        [version] if valid::is_version(version) => {
            Some(NpmRequest::VersionDoc(pkg, (*version).to_owned()))
        }
        ["-", file] => {
            let version = valid::tarball_version(pkg.unscoped(), file)?;
            Some(NpmRequest::Tarball(pkg, (*file).to_owned(), version))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
