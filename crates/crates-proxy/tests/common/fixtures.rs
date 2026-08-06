//! Test data and small helpers: sparse-index bodies, sentinel `pubtime`s, and
//! the sparse-path prefix rule.
#![allow(dead_code)]

pub use chilled_testkit::{rfc3339_from_now, OLD, TOO_NEW};

/// Opaque `.crate` payload. The proxy treats crate bytes as opaque, so the exact
/// contents don't matter — only that they round-trip byte-for-byte.
pub const CRATE_BYTES: &[u8] = b"\x1f\x8b\x08\x00chilled-crates-test-crate-bytes\x00";

/// Builds a newline-terminated sparse-index body (one compact JSON line per
/// version) for `name`, in the shape crates.io serves.
pub fn ndjson(name: &str, versions: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (vers, pubtime) in versions {
        out.push_str(&format!(
            r#"{{"name":"{name}","vers":"{vers}","deps":[],"cksum":"{cksum}","features":{{}},"yanked":false,"pubtime":"{pubtime}"}}"#,
            cksum = "0".repeat(64),
        ));
        out.push('\n');
    }
    out
}

/// The relative sparse-index path for a crate name (the 1/2/3/4+ prefix rule).
pub fn index_rel(name: &str) -> String {
    match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    }
}
