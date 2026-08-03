use super::*;
use chilled_core::registry::CachedArtifact;

#[test]
fn metrics_json_shape() {
    let stats = vec![
        (
            "crates",
            CacheStats {
                artifacts: vec![CachedArtifact {
                    name: "serde".into(),
                    version: "1.0.0".into(),
                    cached_at: 42,
                }],
            },
        ),
        ("npm", CacheStats::default()),
    ];
    let json: serde_json::Value = serde_json::from_str(&metrics_json(&stats)).unwrap();
    assert_eq!(json["service"], "chilled-proxy");
    assert_eq!(json["registries"]["crates"]["cached_count"], 1);
    assert_eq!(
        json["registries"]["crates"]["artifacts"][0]["name"],
        "serde"
    );
    assert_eq!(json["registries"]["npm"]["cached_count"], 0);
}

#[test]
fn metrics_json_escapes_names() {
    // npm names can contain `@`/`/`; anything unexpected must stay valid JSON.
    let stats = vec![(
        "npm",
        CacheStats {
            artifacts: vec![CachedArtifact {
                name: "@scope/pkg\"x".into(),
                version: "1.0.0".into(),
                cached_at: 1,
            }],
        },
    )];
    let parsed: serde_json::Value = serde_json::from_str(&metrics_json(&stats)).unwrap();
    assert_eq!(
        parsed["registries"]["npm"]["artifacts"][0]["name"],
        "@scope/pkg\"x"
    );
}
