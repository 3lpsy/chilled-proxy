//! The classified Maven repository request.

use crate::checksum::ChecksumAlgo;
use crate::coords::MavenCoords;

/// A classified Maven repository request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MavenRequest {
    /// Artifact-level `maven-metadata.xml` (or a checksum of it) — filterable.
    Metadata {
        coords: MavenCoords,
        algo: Option<ChecksumAlgo>,
    },
    /// Snapshot version-dir metadata — passed through ungated (v1 limitation).
    SnapshotMetadata { rel: String },
    /// An artifact file download (includes checksum and `.asc` files).
    Artifact {
        coords: MavenCoords,
        version: String,
        file: String,
    },
}
