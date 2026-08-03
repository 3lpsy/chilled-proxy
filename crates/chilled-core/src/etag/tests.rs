use super::*;

const WEEK: Marker = Marker {
    window: 604_800,
    bucket: 480_000,
};

#[test]
fn etag_marker_round_trip() {
    let upstream = "\"abc123\"";
    let client = filtered_etag(upstream, WEEK);
    assert_eq!(client, "W/\"abc123.cd604800-480000\"");
    assert_eq!(unmark_etag(&client), "\"abc123\"");
    assert_eq!(unmark_etag(upstream), "\"abc123\"");
    assert_eq!(etag_marker(&client), Some(WEEK));
    assert_eq!(etag_marker(upstream), None);
}

#[test]
fn etag_inner_strips_weak_prefix_and_quotes() {
    assert_eq!(etag_inner("W/\"abc\""), "abc");
    assert_eq!(etag_inner("\"abc\""), "abc");
    assert_eq!(etag_inner("abc"), "abc");
}

#[test]
fn unmark_etag_accepts_weak_input() {
    assert_eq!(unmark_etag("W/\"abc.cd604800-480000\""), "\"abc\"");
    assert_eq!(unmark_etag("W/\"abc\""), "\"abc\"");
}

#[test]
fn unmark_etag_strips_only_trailing_marker() {
    // rsplit on `.cd` peels just the final marker, so an etag whose own bytes
    // contain `.cd...` is preserved up to the real trailing marker.
    assert_eq!(unmark_etag("W/\"v.cd1-2.cd3-4\""), "\"v.cd1-2\"");
}

#[test]
fn marker_rejects_malformed_windows() {
    assert_eq!(etag_marker("W/\"abc.cdNOPE-1\""), None);
    assert_eq!(etag_marker("W/\"abc.cd1-NOPE\""), None);
    assert_eq!(etag_marker("W/\"abc.cd-\""), None);
    assert_eq!(etag_marker("W/\"abc.cd\""), None);
    // The pre-bucket grammar is no longer recognized, so such a client copy is
    // treated as unmarked and re-served rather than wrongly reused.
    assert_eq!(etag_marker("W/\"abc.cd604800\""), None);
}

#[test]
fn different_buckets_are_different_markers() {
    // The whole point: same window, later cutoff bucket -> not equivalent, so
    // a revalidating client is re-served once versions age past the cutoff.
    let earlier = filtered_etag("\"abc\"", WEEK);
    let later = filtered_etag(
        "\"abc\"",
        Marker {
            window: WEEK.window,
            bucket: WEEK.bucket + 1,
        },
    );
    assert_ne!(earlier, later);
    assert_ne!(etag_marker(&earlier), etag_marker(&later));
    // ...while the upstream validator recovered from both is identical.
    assert_eq!(unmark_etag(&earlier), unmark_etag(&later));
}

#[test]
fn filtered_etag_with_zero_window() {
    let marker = Marker {
        window: 0,
        bucket: 0,
    };
    let client = filtered_etag("\"abc\"", marker);
    assert_eq!(client, "W/\"abc.cd0-0\"");
    assert_eq!(etag_marker(&client), Some(marker));
    assert_eq!(unmark_etag(&client), "\"abc\"");
}

#[test]
fn rewrite_marker_round_trip() {
    let client = rewrite_etag("\"abc\"");
    assert_eq!(client, "W/\"abc.rw\"");
    assert_eq!(unmark_etag(&client), "\"abc\"");
    assert_eq!(etag_marker(&client), None);
}

#[test]
fn format_tag_round_trip() {
    // A representation tag rides on top of either marker kind.
    let json = format_etag(&filtered_etag("\"abc\"", WEEK), 'j');
    assert_eq!(json, "W/\"abc.cd604800-480000.j\"");
    assert_eq!(unmark_etag(&json), "\"abc\"");
    assert_eq!(etag_marker(&json), Some(WEEK));
    assert_eq!(etag_format(&json), Some('j'));

    let html = format_etag(&rewrite_etag("\"abc\""), 'h');
    assert_eq!(html, "W/\"abc.rw.h\"");
    assert_eq!(unmark_etag(&html), "\"abc\"");
    assert_eq!(etag_marker(&html), None);
    assert_eq!(etag_format(&html), Some('h'));
}

#[test]
fn bare_etag_with_dotted_letter_is_preserved() {
    // An upstream etag ending in `.j` with no marker must not be stripped.
    assert_eq!(unmark_etag("\"foo.j\""), "\"foo.j\"");
    assert_eq!(etag_format("\"foo.j\""), None);
}
