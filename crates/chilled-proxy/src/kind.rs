//! The registry kinds this binary can serve.

use std::fmt::{Display, Formatter};

/// A registry kind. Every per-registry dispatch matches on this enum, so
/// adding a variant turns each site into a compile error until it handles the
/// new registry — there is no string to forget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegistryKind {
    /// crates.io (sparse index + crate downloads).
    Crates,
    /// npm (packuments + tarballs).
    Npm,
    /// PyPI (simple indexes + distribution files).
    Pypi,
    /// Maven-layout repositories (metadata + artifacts).
    Maven,
}

impl RegistryKind {
    /// Every registry, in mount order.
    pub const ALL: [RegistryKind; 4] = [Self::Crates, Self::Npm, Self::Pypi, Self::Maven];

    /// Stable identifier: the flag suffix, default mount name, and log label.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Crates => "crates",
            Self::Npm => "npm",
            Self::Pypi => "pypi",
            Self::Maven => "maven",
        }
    }

    /// The mount-spec key naming this registry's second URL, where it has one
    /// (the crates.io sparse index, the PyPI file host).
    pub(crate) fn secondary_key(self) -> Option<&'static str> {
        match self {
            Self::Crates => Some("index"),
            Self::Pypi => Some("files"),
            Self::Npm | Self::Maven => None,
        }
    }
}

impl Display for RegistryKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}
