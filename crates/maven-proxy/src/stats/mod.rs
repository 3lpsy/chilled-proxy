//! Cache statistics scan, reported through `RegistryProxy::cache_stats`.

#[cfg(test)]
mod tests;

use std::path::Path;
use std::time::UNIX_EPOCH;

use chilled_core::registry::{CacheStats, CachedArtifact};

use crate::constants::METADATA_FILE;

/// Maximum directory depth walked below the repo root.
const MAX_DEPTH: usize = 32;

/// Sidecar suffixes that describe an artifact already counted.
const CHECKSUM_EXTS: &[&str] = &[".sha1", ".md5", ".sha256", ".sha512", ".asc"];

/// Scans the repository cache into sorted [`CacheStats`]. Best-effort:
/// unreadable or malformed entries are skipped rather than failing the report.
pub(crate) fn cache_stats(repo_dir: &Path) -> CacheStats {
    let mut artifacts = Vec::new();
    let mut segs = Vec::new();
    walk(repo_dir, &mut segs, &mut artifacts);
    artifacts.sort();
    // One cached version yields several files (jar, pom, ...); report it once,
    // stamped with the newest of them, so counts mean the same thing as in the
    // other registries.
    artifacts.dedup_by(|dup, kept| {
        let same = dup.name == kept.name && dup.version == kept.version;
        if same {
            kept.cached_at = kept.cached_at.max(dup.cached_at);
        }
        same
    });
    CacheStats { artifacts }
}

/// Recursively collects artifact files at `{group...}/{artifact}/{version}/{file}`,
/// skipping sidecars, metadata files, and hidden entries.
fn walk(dir: &Path, segs: &mut Vec<String>, out: &mut Vec<CachedArtifact>) {
    if segs.len() > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if file_type.is_dir() {
            segs.push(name);
            walk(&entry.path(), segs, out);
            segs.pop();
            continue;
        }
        // Need at least group/artifact/version dirs above a counted file, and
        // checksum/signature sidecars describe a file already counted.
        let sidecar = CHECKSUM_EXTS.iter().any(|ext| name.ends_with(ext));
        if !file_type.is_file() || name == METADATA_FILE || sidecar || segs.len() < 3 {
            continue;
        }
        let version = segs[segs.len() - 1].clone();
        let artifact = &segs[segs.len() - 2];
        let group = segs[..segs.len() - 2].join(".");
        let cached_at = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        out.push(CachedArtifact {
            name: format!("{group}:{artifact}"),
            version,
            cached_at,
        });
    }
}
