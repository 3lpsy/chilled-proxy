use super::*;

#[test]
fn store_fetch_invalidate_round_trip() {
    let cache: MetadataCache<String> = MetadataCache::new();

    assert_eq!(cache.fetch("serde"), None);
    cache.store("serde", "\"abc\"".to_string());
    assert_eq!(cache.fetch("serde"), Some("\"abc\"".to_string()));
    cache.invalidate("serde");
    assert_eq!(cache.fetch("serde"), None);
}

#[test]
fn store_overwrites() {
    let cache: MetadataCache<u32> = MetadataCache::new();
    cache.store("a", 1);
    cache.store("a", 2);
    assert_eq!(cache.fetch("a"), Some(2));
}
