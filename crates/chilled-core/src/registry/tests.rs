use super::*;

#[test]
fn cached_artifacts_sort_by_name_then_version() {
    let mut v = [
        CachedArtifact {
            name: "b".into(),
            version: "1.0.0".into(),
            cached_at: 5,
        },
        CachedArtifact {
            name: "a".into(),
            version: "2.0.0".into(),
            cached_at: 9,
        },
        CachedArtifact {
            name: "a".into(),
            version: "1.0.0".into(),
            cached_at: 7,
        },
    ];
    v.sort();
    let order: Vec<_> = v
        .iter()
        .map(|a| format!("{}-{}", a.name, a.version))
        .collect();
    assert_eq!(order, ["a-1.0.0", "a-2.0.0", "b-1.0.0"]);
}
