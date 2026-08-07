//! The `key=value` grammar of one `--<registry>-mount` spec.

use chilled_core::config;
use chilled_core::cooldown;
use url::Url;

use super::types::MountSpec;
use crate::kind::RegistryKind;
use crate::mount;

/// The keys `kind` accepts, for error messages.
fn accepted_keys(kind: RegistryKind) -> String {
    let mut keys = vec![
        "name",
        "path",
        "upstream",
        "proxy-url",
        "cooldown",
        "cache-ttl",
        "restrict-downloads",
        "max-metadata-size",
        "max-artifact-size",
    ];
    if kind == RegistryKind::Pypi {
        keys.push("file-hosts");
    }
    if let Some(key) = kind.secondary_key() {
        keys.insert(3, key);
    }
    keys.join(", ")
}

/// Parses one `--<kind>-mount` spec.
pub(crate) fn parse(kind: RegistryKind, raw: &str) -> Result<MountSpec, String> {
    let err = |msg: String| format!("--{kind}-mount '{raw}': {msg}");

    let mut spec = MountSpec {
        name: String::new(),
        path: None,
        upstream: None,
        secondary: None,
        proxy_url: None,
        cooldown: None,
        cache_ttl: None,
        restrict_downloads: None,
        max_metadata_size: None,
        max_artifact_size: None,
        file_hosts: Vec::new(),
    };
    let mut seen_name = false;

    for pair in raw.split(',') {
        let pair = pair.trim();
        // Tolerate a trailing or doubled separator.
        if pair.is_empty() {
            continue;
        }
        let Some((key, value)) = pair.split_once('=') else {
            return Err(err(format!("expected key=value, got '{pair}'")));
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() {
            return Err(err(format!("'{key}' has no value")));
        }
        // Every key may appear once; a repeat is a config mistake worth
        // reporting rather than silently letting the last one win.
        let twice = || err(format!("'{key}' given twice"));

        match key.as_str() {
            "name" => {
                if seen_name {
                    return Err(twice());
                }
                seen_name = true;
                spec.name = parse_name(value).map_err(err)?;
            }
            "path" => {
                if spec.path.is_some() {
                    return Err(twice());
                }
                spec.path = Some(mount::parse(value).map_err(err)?);
            }
            "upstream" => {
                if spec.upstream.is_some() {
                    return Err(twice());
                }
                spec.upstream = Some(parse_url(&key, value).map_err(err)?);
            }
            "proxy-url" | "proxy_url" => {
                if spec.proxy_url.is_some() {
                    return Err(twice());
                }
                spec.proxy_url = Some(parse_url(&key, value).map_err(err)?);
            }
            "cooldown" => {
                if spec.cooldown.is_some() {
                    return Err(twice());
                }
                spec.cooldown = Some(cooldown::parse_duration(value).map_err(err)?);
            }
            "cache-ttl" | "cache_ttl" => {
                if spec.cache_ttl.is_some() {
                    return Err(twice());
                }
                spec.cache_ttl = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| err(format!("'{key}' expects seconds, got '{value}'")))?,
                );
            }
            "restrict-downloads" | "restrict_downloads" => {
                if spec.restrict_downloads.is_some() {
                    return Err(twice());
                }
                spec.restrict_downloads = Some(parse_bool(&key, value).map_err(err)?);
            }
            "max-metadata-size" | "max_metadata_size" => {
                if spec.max_metadata_size.is_some() {
                    return Err(twice());
                }
                spec.max_metadata_size = Some(config::parse_size(value).map_err(err)?);
            }
            "max-artifact-size" | "max_artifact_size" => {
                if spec.max_artifact_size.is_some() {
                    return Err(twice());
                }
                spec.max_artifact_size = Some(config::parse_size(value).map_err(err)?);
            }
            // Space-separated: the comma is already the pair separator.
            "file-hosts" | "file_hosts" if kind == RegistryKind::Pypi => {
                if !spec.file_hosts.is_empty() {
                    return Err(twice());
                }
                spec.file_hosts = value
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                if spec.file_hosts.is_empty() {
                    return Err(err("file-hosts is empty".to_owned()));
                }
            }
            other if kind.secondary_key() == Some(other) => {
                if spec.secondary.is_some() {
                    return Err(twice());
                }
                spec.secondary = Some(parse_url(&key, value).map_err(err)?);
            }
            _ => {
                return Err(err(format!(
                    "unknown key '{key}' (accepted: {})",
                    accepted_keys(kind)
                )))
            }
        }
    }

    if !seen_name {
        return Err(err("missing required key 'name'".to_owned()));
    }
    Ok(spec)
}

/// Validates an instance name. It keys a cache subdirectory, so it is held to
/// a conservative charset with no leading dot.
fn parse_name(value: &str) -> Result<String, String> {
    let ok = !value.starts_with('.')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(value.to_owned())
    } else {
        Err(format!(
            "name '{value}' must be [A-Za-z0-9._-] and may not start with '.'"
        ))
    }
}

/// Parses a URL value.
fn parse_url(key: &str, value: &str) -> Result<Url, String> {
    Url::parse(value).map_err(|e| format!("'{key}' is not a valid URL ('{value}'): {e}"))
}

/// Parses a boolean value, accepting the spellings clap's boolish parser takes.
fn parse_bool(key: &str, value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "on" | "1" => Ok(true),
        "false" | "f" | "no" | "n" | "off" | "0" => Ok(false),
        _ => Err(format!("'{key}' expects a boolean, got '{value}'")),
    }
}
