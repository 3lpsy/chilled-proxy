//! Program version banner.

use crate::constants::VERSION;

/// Prints the program version banner, enriched with build metadata when the
/// build set `CHILLED_BUILD_*` (see the release workflow).
pub(crate) fn version() {
    let build = option_env!("CHILLED_BUILD_ID");
    let rev = option_env!("CHILLED_BUILD_REV");
    let tag = option_env!("CHILLED_BUILD_REF");

    if let (Some(build), Some(rev), Some(tag)) = (build, rev, tag) {
        println!("chilled-proxy {VERSION}+{build}.g{rev}.{tag}");
    } else {
        println!("chilled-proxy {VERSION}");
    }
}
