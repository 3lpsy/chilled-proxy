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
