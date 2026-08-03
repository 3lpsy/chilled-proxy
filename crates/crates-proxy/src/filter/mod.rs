//! Sparse-index age-gating filter (crates.io NDJSON).
//!
//! Drops version lines whose `pubtime` is newer than a cutoff. Each line is
//! compact JSON, so `pubtime` is extracted with a targeted byte scan instead of
//! a full parse — no allocation for large `deps` arrays. Ported from
//! menhera.org's crates.io cooldown proxy.

#[cfg(test)]
mod tests;

use chilled_core::time::parse_rfc3339z;

/// Filter raw sparse-index bytes, dropping any version line whose `pubtime` is
/// newer than `cutoff` (unix seconds). Non-UTF-8 bodies pass through unchanged.
pub(crate) fn filter_index(data: &[u8], cutoff: u64) -> Vec<u8> {
    match std::str::from_utf8(data) {
        Ok(body) => filter_body(body, cutoff),
        Err(_) => data.to_vec(),
    }
}

/// Extract the `pubtime` (unix seconds) of a specific `version` from a
/// sparse-index body. Used by `--restrict-downloads` to age-gate downloads.
/// Matches the compact `"vers":"<version>"` token (closing quote included).
pub(crate) fn version_pubtime(body: &str, version: &str) -> Option<u64> {
    let needle = format!("\"vers\":\"{version}\"");
    body.lines()
        .find(|line| line.contains(&needle))
        .and_then(line_pubtime_secs)
}

/// Walk the body line by line, dropping lines with `pubtime > cutoff`. Lines
/// without a `pubtime` (blanks, malformed) are kept verbatim, newlines preserved.
fn filter_body(body: &str, cutoff: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed.is_empty() {
            out.extend_from_slice(line.as_bytes());
            continue;
        }
        match line_pubtime_secs(trimmed) {
            Some(secs) if secs > cutoff => {}
            _ => out.extend_from_slice(line.as_bytes()),
        }
    }
    out
}

/// Extract the `pubtime` field from one JSON line as unix seconds.
///
/// Scans for the `"pubtime"` *key* (followed by a colon, so a string value
/// reading `pubtime` is skipped) and reads its quoted timestamp.
fn line_pubtime_secs(line: &str) -> Option<u64> {
    let mut start = 0;
    while let Some(rel) = line[start..].find("\"pubtime\"") {
        let after = line[start + rel + "\"pubtime\"".len()..].trim_start();
        match after.strip_prefix(':') {
            Some(rest) => {
                let rest = rest.trim_start().strip_prefix('"')?;
                let end = rest.find('"')?;
                return parse_rfc3339z(&rest[..end]);
            }
            // This occurrence was a string value, not a key — keep looking.
            None => start += rel + "\"pubtime\"".len(),
        }
    }
    None
}
