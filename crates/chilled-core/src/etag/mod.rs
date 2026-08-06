//! Cooldown-aware ETag marker scheme.
//!
//! A filtered/rewritten body must not be served under the upstream's strong
//! ETag, so proxies issue a weak marked variant and strip the marker back off
//! when a client revalidates. Marker grammar (inside the quotes):
//! `<inner>[.cd<secs>-<bucket>|.rw][.<fmt>]` where `<fmt>` is a single letter.
//!
//! The marker carries the *cutoff bucket* as well as the window length: a
//! filtered body is only equivalent to the client's copy when it was produced
//! for the same bucket, otherwise versions that have aged past the cooldown
//! would never reach a revalidating client.

#[cfg(test)]
mod tests;

use axum::http::header;

use crate::cache::CacheEntry;

/// A parsed cooldown marker: the window length and the cutoff bucket the body
/// was filtered at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    /// Cooldown window in seconds.
    pub window: u64,
    /// Cutoff bucket (`cutoff / MEMO_BUCKET_SECS`) the body was filtered for.
    pub bucket: u64,
}

/// Strips an optional weak prefix and surrounding quotes from an ETag value.
pub fn etag_inner(value: &str) -> &str {
    value.strip_prefix("W/").unwrap_or(value).trim_matches('"')
}

/// Client-facing weak ETag for a filtered body: `W/"<inner>.cd<secs>-<bucket>"`.
pub fn filtered_etag(upstream_etag: &str, marker: Marker) -> String {
    format!(
        "W/\"{}.cd{}-{}\"",
        etag_inner(upstream_etag),
        marker.window,
        marker.bucket
    )
}

/// Client-facing weak ETag for a rewritten-but-unfiltered body: `W/"<inner>.rw"`.
pub fn rewrite_etag(upstream_etag: &str) -> String {
    format!("W/\"{}.rw\"", etag_inner(upstream_etag))
}

/// Appends a single-letter representation tag (e.g. `j`/`h`) to a marked ETag.
pub fn format_etag(etag: &str, fmt: char) -> String {
    format!("W/\"{}.{fmt}\"", etag_inner(etag))
}

/// Splits an optional trailing single-letter format tag off a marker inner.
fn split_format(inner: &str) -> (&str, Option<char>) {
    match inner.rsplit_once('.') {
        Some((base, tag)) if tag.len() == 1 && tag.as_bytes()[0].is_ascii_alphabetic() => {
            (base, tag.chars().next())
        }
        _ => (inner, None),
    }
}

/// Splits an optional trailing `.cd<secs>-<bucket>` / `.rw` marker off an inner
/// value. A marker from an older grammar simply fails to parse, so the body is
/// re-served rather than wrongly reused.
fn split_marker(inner: &str) -> (&str, Option<Marker>) {
    if let Some(base) = inner.strip_suffix(".rw") {
        return (base, None);
    }
    let Some((base, tail)) = inner.rsplit_once(".cd") else {
        return (inner, None);
    };
    let Some((window, bucket)) = tail.split_once('-') else {
        return (inner, None);
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(window) || !digits(bucket) {
        return (inner, None);
    }
    match (window.parse(), bucket.parse()) {
        (Ok(window), Ok(bucket)) => (base, Some(Marker { window, bucket })),
        _ => (inner, None),
    }
}

/// Recovers the upstream strong ETag from a client `If-None-Match` value,
/// undoing any marker so upstream revalidation matches.
pub fn unmark_etag(client_value: &str) -> String {
    let inner = etag_inner(client_value);
    // A format tag only counts when a real marker precedes it, so a bare
    // upstream etag that happens to end in `.x` survives unmangled.
    let (rest, _) = split_format(inner);
    let (base, _) = split_marker(rest);
    if base != rest {
        format!("\"{base}\"")
    } else {
        format!("\"{inner}\"")
    }
}

/// Extracts the cooldown marker encoded in a client ETag, or `None` if the
/// ETag is unmarked, rewrite-only, or carries an unrecognized marker.
pub fn etag_marker(client_value: &str) -> Option<Marker> {
    let inner = etag_inner(client_value);
    let (rest, _) = split_format(inner);
    if let (_, Some(marker)) = split_marker(rest) {
        return Some(marker);
    }
    split_marker(inner).1
}

/// Attaches cooldown-aware cache validators: a weak marked ETag (and no
/// `Last-Modified`) for filtered bodies, the upstream validators otherwise.
pub fn cooldown_validators(
    mut builder: axum::http::response::Builder,
    entry: &CacheEntry,
    marker: Option<Marker>,
) -> axum::http::response::Builder {
    match marker {
        Some(marker) => {
            if let Some(etag) = entry.etag() {
                builder = builder.header(header::ETAG, filtered_etag(etag, marker));
            }
        }
        None => {
            if let Some(etag) = entry.etag() {
                builder = builder.header(header::ETAG, etag);
            }
            if let Some(last_modified) = entry.last_modified() {
                builder = builder.header(header::LAST_MODIFIED, last_modified);
            }
        }
    }
    builder
}

/// Extracts the representation tag (e.g. `j`/`h`) from a marked client ETag.
pub fn etag_format(client_value: &str) -> Option<char> {
    let inner = etag_inner(client_value);
    let (rest, fmt) = split_format(inner);
    let (base, _) = split_marker(rest);
    // The format tag only counts when it follows a real marker.
    (base != rest).then_some(fmt).flatten()
}
