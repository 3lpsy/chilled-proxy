//! The fail-closed gate and proxy flow are exercised end-to-end by the
//! `downloads` integration suite; unit coverage here sticks to URL parsing glue.

use crate::cache::CrateInfo;

#[test]
fn download_path_round_trips_through_crate_info() {
    let info = CrateInfo::try_from_download_url("serde/1.0.0/download").unwrap();
    assert_eq!(info.to_download_url(), "serde/1.0.0/download");
}
