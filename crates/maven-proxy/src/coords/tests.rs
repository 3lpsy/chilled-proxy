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
