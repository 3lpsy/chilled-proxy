//! Checksum algorithms served by the proxy (`.sha1`/`.md5`/`.sha256`/`.sha512`).
//!
//! Filtered metadata must be paired with checksums of the *filtered* bytes, so
//! these digests are computed locally instead of passed through.

#[cfg(test)]
mod tests;

use md5::Md5;
use sha1::{Digest, Sha1};
use sha2::{Sha256, Sha512};

/// A Maven repository checksum algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChecksumAlgo {
    Sha1,
    Md5,
    Sha256,
    Sha512,
}

impl ChecksumAlgo {
    /// Maps a file extension (without the dot) to an algorithm.
    pub(crate) fn from_ext(ext: &str) -> Option<Self> {
        match ext {
            "sha1" => Some(ChecksumAlgo::Sha1),
            "md5" => Some(ChecksumAlgo::Md5),
            "sha256" => Some(ChecksumAlgo::Sha256),
            "sha512" => Some(ChecksumAlgo::Sha512),
            _ => None,
        }
    }

    /// The file extension (without the dot).
    pub(crate) fn ext(self) -> &'static str {
        match self {
            ChecksumAlgo::Sha1 => "sha1",
            ChecksumAlgo::Md5 => "md5",
            ChecksumAlgo::Sha256 => "sha256",
            ChecksumAlgo::Sha512 => "sha512",
        }
    }

    /// Lowercase hex digest of `data` (bare hex, as Central serves it).
    pub(crate) fn hex(self, data: &[u8]) -> String {
        match self {
            ChecksumAlgo::Sha1 => to_hex(&Sha1::digest(data)),
            ChecksumAlgo::Md5 => to_hex(&Md5::digest(data)),
            ChecksumAlgo::Sha256 => to_hex(&Sha256::digest(data)),
            ChecksumAlgo::Sha512 => to_hex(&Sha512::digest(data)),
        }
    }
}

/// Splits an optional trailing checksum extension off a file name.
pub(crate) fn split_checksum(name: &str) -> (&str, Option<ChecksumAlgo>) {
    if let Some((base, ext)) = name.rsplit_once('.') {
        if let Some(algo) = ChecksumAlgo::from_ext(ext) {
            return (base, Some(algo));
        }
    }
    (name, None)
}

/// Formats bytes as lowercase hex.
fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
