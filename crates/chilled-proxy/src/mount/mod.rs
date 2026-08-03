//! Mount-path parsing for the per-registry `--<registry>-path` flags.

#[cfg(test)]
mod tests;

/// Path prefixes the server keeps for itself: the status endpoints plus the
/// space set aside for the management API and web UI. A mount may neither be
/// one of these nor sit underneath one.
pub(crate) const RESERVED: &[&str] = &["/healthz", "/metrics", "/ui", "/api"];

/// The reserved prefix `path` would collide with, if any.
fn reserved_conflict(path: &str) -> Option<&'static str> {
    RESERVED
        .iter()
        .find(|reserved| path == **reserved || path.starts_with(&format!("{reserved}/")))
        .copied()
}

/// Parses and normalizes a mount path: it must be absolute, and a trailing
/// slash is dropped so `/npm` and `/npm/` mean the same mount. `/` is the root
/// mount, which is only legal when a single registry is enabled.
pub(crate) fn parse(raw: &str) -> Result<String, String> {
    let path = raw.trim();
    if !path.starts_with('/') {
        return Err(format!("mount path must start with '/': '{raw}'"));
    }
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return Ok("/".to_owned());
    }

    for segment in path.split('/').skip(1) {
        if segment.is_empty() {
            return Err(format!("mount path has an empty segment: '{raw}'"));
        }
        if segment == "." || segment == ".." {
            return Err(format!("mount path may not contain '.' or '..': '{raw}'"));
        }
        let ok = segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'~'));
        if !ok {
            return Err(format!(
                "mount path segment '{segment}' has characters outside [A-Za-z0-9._~-]"
            ));
        }
    }
    Ok(path.to_owned())
}

/// Checks the resolved mounts against each other: root is exclusive, mounts
/// must be distinct, and none may shadow a top-level endpoint.
pub(crate) fn check(mounts: &[(&str, String)]) -> Result<(), String> {
    if let Some((id, _)) = mounts.iter().find(|(_, path)| path == "/") {
        if mounts.len() > 1 {
            let others: Vec<&str> = mounts
                .iter()
                .filter(|(other, _)| *other != *id)
                .map(|(other, _)| *other)
                .collect();
            return Err(format!(
                "registry '{id}' is mounted at '/', which only works when it is the \
                 only one enabled — also enabled: {}. Disable the others or give \
                 '{id}' its own path.",
                others.join(", ")
            ));
        }
    }

    for (index, (id, path)) in mounts.iter().enumerate() {
        if let Some(reserved) = reserved_conflict(path) {
            return Err(format!(
                "registry '{id}' cannot mount at '{path}': '{reserved}' is reserved \
                 for the server (status endpoints, management API, and web UI)"
            ));
        }
        if let Some((other, _)) = mounts[..index].iter().find(|(_, prior)| prior == path) {
            return Err(format!(
                "registries '{other}' and '{id}' are both mounted at '{path}'"
            ));
        }
    }
    Ok(())
}
