//! Checksum algorithms served by the proxy (`.sha1`/`.md5`/`.sha256`/`.sha512`).
//!
//! Filtered metadata must be paired with checksums of the *filtered* bytes, so
//! these digests are computed locally instead of passed through.

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

#[cfg(test)]
mod tests {
    use super::{split_checksum, ChecksumAlgo};

    #[test]
    fn sha1_known_vector() {
        assert_eq!(
            ChecksumAlgo::Sha1.hex(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn md5_known_vector() {
        assert_eq!(
            ChecksumAlgo::Md5.hex(b"abc"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            ChecksumAlgo::Sha256.hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha512_known_vector() {
        assert_eq!(
            ChecksumAlgo::Sha512.hex(b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn split_checksum_recognizes_extensions() {
        assert_eq!(
            split_checksum("maven-metadata.xml.sha1"),
            ("maven-metadata.xml", Some(ChecksumAlgo::Sha1))
        );
        assert_eq!(
            split_checksum("a-1.0.jar.sha512"),
            ("a-1.0.jar", Some(ChecksumAlgo::Sha512))
        );
        assert_eq!(split_checksum("a-1.0.jar"), ("a-1.0.jar", None));
        assert_eq!(split_checksum("sha1"), ("sha1", None));
    }
}
