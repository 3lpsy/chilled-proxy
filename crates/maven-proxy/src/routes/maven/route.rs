//! Request-path classification for the Maven mount.

use chilled_core::valid::decode_path_once;
use log::debug;

use crate::checksum::split_checksum;
use crate::constants::{MAX_PATH_LEN, MAX_SEGMENTS, METADATA_FILE};
use crate::coords::MavenCoords;
use crate::valid::{is_artifact_file, is_dir_segment, is_file_segment, is_version, MavenRequest};

/// nothing here may reach upstream.
pub(crate) fn classify(raw_path: &str) -> Option<MavenRequest> {
    let decoded = decode_path_once(raw_path)?;
    let path = decoded.strip_prefix('/').unwrap_or(&decoded);
    if path.is_empty() || path.len() > MAX_PATH_LEN {
        return None;
    }

    let segs: Vec<&str> = path.split('/').collect();
    if segs.len() > MAX_SEGMENTS {
        return None;
    }
    let (file, dirs) = segs.split_last()?;
    if !dirs.iter().all(|s| is_dir_segment(s)) || !is_file_segment(file) {
        return None;
    }

    let (base, algo) = split_checksum(file);
    if base == METADATA_FILE {
        let parent = dirs.last()?;
        // A snapshot *version* directory, not an artifactId that merely ends in
        // `-SNAPSHOT` — versions start with a digit, so requiring that keeps
        // such an artifact on the gated path instead of passing it through.
        let snapshot_version_dir = parent.ends_with("-SNAPSHOT")
            && parent.as_bytes().first().is_some_and(u8::is_ascii_digit);
        if snapshot_version_dir {
            // Central hosts no snapshots; snapshot version-dir metadata is
            // passed through ungated in v1.
            if segs.len() < 4 {
                return None;
            }
            debug!("proxy: snapshot metadata passes through ungated (v1 limitation): {path}");
            return Some(MavenRequest::SnapshotMetadata {
                rel: path.to_owned(),
            });
        }
        if segs.len() < 3 {
            return None;
        }
        let coords = MavenCoords::new(&dirs[..dirs.len() - 1], parent);
        return Some(MavenRequest::Metadata { coords, algo });
    }

    // Artifact download: {group...}/{artifact}/{version}/{file}.
    if segs.len() < 4 {
        return None;
    }
    let version = dirs[dirs.len() - 1];
    let artifact = dirs[dirs.len() - 2];
    if !is_version(version) || !is_artifact_file(artifact, version, file) {
        return None;
    }
    let coords = MavenCoords::new(&dirs[..dirs.len() - 2], artifact);
    Some(MavenRequest::Artifact {
        coords,
        version: version.to_owned(),
        file: (*file).to_owned(),
    })
}

#[cfg(test)]
mod tests;
