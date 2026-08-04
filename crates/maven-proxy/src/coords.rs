//! Maven coordinates (`groupId` + `artifactId`): repository paths, the
//! cooldown-override lookup key, and display formatting.

use std::fmt::{Display, Formatter, Result};

use crate::constants::METADATA_FILE;

/// A validated `groupId`/`artifactId` pair (group as path segments).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MavenCoords {
    /// Group path segments (e.g. `["org", "apache", "commons"]`).
    group_segs: Vec<String>,
    /// The artifact directory name.
    artifact: String,
}

impl Display for MavenCoords {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}:{}", self.group_segs.join("."), self.artifact)
    }
}

impl MavenCoords {
    /// Builds coordinates from already-validated path segments.
    pub(crate) fn new(group_segs: &[&str], artifact: &str) -> Self {
        MavenCoords {
            group_segs: group_segs.iter().map(|s| (*s).to_owned()).collect(),
            artifact: artifact.to_owned(),
        }
    }

    /// Cooldown-override lookup key: `{groupId}:{artifactId}` lowercased
    /// (dotted group, e.g. `org.apache.commons:commons-lang3`).
    pub(crate) fn override_key(&self) -> String {
        self.to_string().to_ascii_lowercase()
    }

    /// Relative artifact directory path: `{group.../}{artifact}`. Also used as
    /// the metadata cache and memo key.
    pub(crate) fn dir_rel(&self) -> String {
        format!("{}/{}", self.group_segs.join("/"), self.artifact)
    }

    /// Relative path of the artifact-level `maven-metadata.xml`.
    pub(crate) fn metadata_rel(&self) -> String {
        format!("{}/{METADATA_FILE}", self.dir_rel())
    }

    /// Relative path of a version's POM (the age-probe target).
    pub(crate) fn pom_rel(&self, version: &str) -> String {
        format!(
            "{}/{version}/{artifact}-{version}.pom",
            self.dir_rel(),
            artifact = self.artifact
        )
    }
}

#[cfg(test)]
mod tests {
    use super::MavenCoords;

    fn commons() -> MavenCoords {
        MavenCoords::new(&["org", "apache", "commons"], "commons-lang3")
    }

    #[test]
    fn display_is_dotted_group_colon_artifact() {
        assert_eq!(commons().to_string(), "org.apache.commons:commons-lang3");
    }

    #[test]
    fn override_key_is_lowercased() {
        let coords = MavenCoords::new(&["com", "Example"], "MyLib");
        assert_eq!(coords.override_key(), "com.example:mylib");
    }

    #[test]
    fn dir_and_metadata_paths() {
        let coords = commons();
        assert_eq!(coords.dir_rel(), "org/apache/commons/commons-lang3");
        assert_eq!(
            coords.metadata_rel(),
            "org/apache/commons/commons-lang3/maven-metadata.xml"
        );
    }

    #[test]
    fn pom_rel_embeds_artifact_and_version() {
        assert_eq!(
            commons().pom_rel("3.14.0"),
            "org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.pom"
        );
    }
}
