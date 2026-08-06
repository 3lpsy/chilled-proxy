//! `maven-metadata.xml` age-gating filter (quick-xml streaming rewrite).
//!
//! Drops `<version>` entries newer than the cutoff (or with no recorded age —
//! fail-closed), repoints `<latest>`/`<release>` at the surviving versions, and
//! leaves everything else (including `<lastUpdated>`) untouched.
//!
//! Ages come from POM probes, never from `<lastUpdated>`: trusting that field
//! would let an upstream that fails to maintain it bypass the gate.

#[cfg(test)]
mod tests;

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

use crate::sidecar::VersionTimes;

/// What the rewriter is currently capturing (element text to inspect/replace).
enum Capture {
    /// A `<version>` inside `<versions>`; accumulates its text.
    Version(String),
    /// The `<latest>` element (text replaced).
    Latest,
    /// The `<release>` element (text replaced).
    Release,
}

/// Lists the `<version>` texts inside `<versions>`, in document order.
pub(crate) fn list_versions(xml: &[u8]) -> Result<Vec<String>, String> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut versions = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) => stack.push(e.local_name().as_ref().to_vec()),
            Event::End(_) => {
                stack.pop();
            }
            Event::Text(t) => {
                let n = stack.len();
                if n >= 2 && stack[n - 1] == b"version" && stack[n - 2] == b"versions" {
                    let text = t.unescape().map_err(|e| e.to_string())?;
                    let text = text.trim();
                    if !text.is_empty() {
                        versions.push(text.to_owned());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(versions)
}

/// Filters pristine metadata against the sidecar ages: versions with
/// `ts > cutoff` (or unknown age) are dropped. Returns `None` when nothing
/// survives. The XML declaration and untouched elements are preserved.
///
/// `versions` is the document's own version list ([`list_versions`] of the
/// same bytes) — passed in so the caller's parse is not repeated here.
pub(crate) fn filter_metadata(
    xml: &[u8],
    versions: &[String],
    times: &VersionTimes,
    cutoff: u64,
) -> Result<Option<Vec<u8>>, String> {
    if versions.is_empty() {
        // Group-level (plugin-prefix) metadata carries no <versions> to gate;
        // filtering it to a 404 would break plugin-prefix resolution.
        return Ok(Some(xml.to_vec()));
    }
    let survivors: Vec<(&str, u64)> = versions
        .iter()
        .filter_map(|v| {
            times
                .get(v)
                .filter(|ts| *ts <= cutoff)
                .map(|ts| (v.as_str(), ts))
        })
        .collect();
    if survivors.is_empty() {
        return Ok(None);
    }

    let latest = survivors
        .iter()
        .max_by_key(|(_, ts)| *ts)
        .map(|(v, _)| (*v).to_owned())
        .expect("survivors are non-empty");
    let release = survivors
        .iter()
        .filter(|(v, _)| !v.ends_with("-SNAPSHOT"))
        .max_by_key(|(_, ts)| *ts)
        .map(|(v, _)| (*v).to_owned());
    let keep_set: std::collections::HashSet<&str> = survivors.iter().map(|(v, _)| *v).collect();
    let keep = |v: &str| keep_set.contains(v);

    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let mut stack: Vec<Vec<u8>> = Vec::new();
    // Active capture: the kind plus the nesting depth inside the captured element.
    let mut capture: Option<(Capture, usize)> = None;

    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?;

        if capture.is_some() {
            match event {
                Event::Start(_) => {
                    if let Some((_, depth)) = &mut capture {
                        *depth += 1;
                    }
                }
                Event::End(_) => {
                    if matches!(&capture, Some((_, 0))) {
                        let (kind, _) = capture.take().expect("capture is active");
                        emit_captured(&mut writer, kind, &keep, &latest, release.as_deref())?;
                    } else if let Some((_, depth)) = &mut capture {
                        *depth -= 1;
                    }
                }
                Event::Text(t) => {
                    if let Some((Capture::Version(text), _)) = &mut capture {
                        text.push_str(&t.unescape().map_err(|e| e.to_string())?);
                    }
                }
                Event::Eof => return Err("unexpected EOF inside element".to_owned()),
                _ => {}
            }
            buf.clear();
            continue;
        }

        match event {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_vec();
                let parent = stack.last().map(Vec::as_slice);
                if name == b"version" && parent == Some(b"versions") {
                    capture = Some((Capture::Version(String::new()), 0));
                } else if name == b"latest" && parent == Some(b"versioning") {
                    capture = Some((Capture::Latest, 0));
                } else if name == b"release" && parent == Some(b"versioning") {
                    capture = Some((Capture::Release, 0));
                } else {
                    stack.push(name);
                    writer.write_event(Event::Start(e)).map_err(err_str)?;
                }
            }
            Event::End(e) => {
                stack.pop();
                writer.write_event(Event::End(e)).map_err(err_str)?;
            }
            Event::Eof => break,
            other => writer.write_event(other).map_err(err_str)?,
        }
        buf.clear();
    }

    Ok(Some(writer.into_inner()))
}

/// Writes the replacement (or nothing) for a completed capture.
fn emit_captured(
    writer: &mut Writer<Vec<u8>>,
    kind: Capture,
    keep: &impl Fn(&str) -> bool,
    latest: &str,
    release: Option<&str>,
) -> Result<(), String> {
    match kind {
        Capture::Version(text) => {
            let version = text.trim();
            if keep(version) {
                write_simple(writer, "version", version)?;
            }
        }
        Capture::Latest => write_simple(writer, "latest", latest)?,
        // With no non-snapshot survivor the <release> element is dropped.
        Capture::Release => {
            if let Some(release) = release {
                write_simple(writer, "release", release)?;
            }
        }
    }
    Ok(())
}

/// Writes `<name>text</name>`.
fn write_simple(writer: &mut Writer<Vec<u8>>, name: &str, text: &str) -> Result<(), String> {
    writer
        .write_event(Event::Start(BytesStart::new(name)))
        .map_err(err_str)?;
    writer
        .write_event(Event::Text(BytesText::new(text)))
        .map_err(err_str)?;
    writer
        .write_event(Event::End(BytesEnd::new(name)))
        .map_err(err_str)
}

/// Stringifies a writer error.
fn err_str(e: impl std::fmt::Display) -> String {
    e.to_string()
}
