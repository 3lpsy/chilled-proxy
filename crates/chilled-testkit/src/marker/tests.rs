use super::*;

#[test]
fn prefix_matches_a_served_marker() {
    let served = "W/\"etag123.cd604800-480123\"";
    assert!(served.starts_with(&marker_prefix("\"etag123\"", 604_800)));
    assert!(served.starts_with(&marker_prefix("etag123", 604_800)));
}

#[test]
fn bucket_shift_round_trips() {
    let served = "W/\"etag123.cd604800-480123\"";
    assert_eq!(
        shift_marker_bucket(served, -1),
        "W/\"etag123.cd604800-480122\""
    );
    assert_eq!(
        shift_marker_bucket(&shift_marker_bucket(served, -5), 5),
        served
    );
}

#[test]
fn bucket_shift_preserves_a_format_tag() {
    // PyPI markers carry a trailing representation tag after the bucket.
    let served = "W/\"etag123.cd604800-480123.j\"";
    assert_eq!(
        shift_marker_bucket(served, -1),
        "W/\"etag123.cd604800-480122.j\""
    );
}
