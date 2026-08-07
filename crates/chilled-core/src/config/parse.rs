//! Small config parsing helpers shared by the CLI layers.

use std::collections::HashSet;

/// Normalizes a requested log level to a known value, defaulting to `info`.
pub fn normalize_log_level(level: Option<String>) -> String {
    match level.as_deref().map(str::trim).map(str::to_ascii_lowercase) {
        Some(l)
            if matches!(
                l.as_str(),
                "error" | "warn" | "info" | "debug" | "trace" | "off"
            ) =>
        {
            l
        }
        _ => "info".to_string(),
    }
}

/// Parses a byte size: a plain byte count, or a number with a unit suffix
/// (`k`, `m`, `g`, case-insensitive, optionally spelled `KB`/`KiB`/…).
/// Every spelling is a power of 1024 — `1MB` and `1MiB` both mean 1048576 —
/// so a raised cap never lands quietly under the binary-unit default.
pub fn parse_size(s: &str) -> Result<usize, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size".to_string());
    }

    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if split == 0 {
        return Err(format!("invalid size value in '{s}' (expected a number)"));
    }
    let (digits, suffix) = s.split_at(split);

    // `b`/`ib` is decoration on the unit letter: k, kb, and kib all mean 1024.
    let suffix = suffix.trim().to_ascii_lowercase();
    let unit = suffix
        .strip_suffix("ib")
        .or_else(|| suffix.strip_suffix('b'))
        .unwrap_or(&suffix);
    let mult: usize = match unit {
        "" => 1,
        "k" => 1024,
        "m" => 1024 * 1024,
        "g" => 1024 * 1024 * 1024,
        _ => {
            return Err(format!(
                "invalid size unit '{suffix}' in '{s}' (use k, m, or g)"
            ))
        }
    };

    let value: usize = digits
        .parse()
        .map_err(|_| format!("invalid size value in '{s}'"))?;
    value
        .checked_mul(mult)
        .ok_or_else(|| format!("size '{s}' is too large"))
}

/// Parses a comma/whitespace-separated package list into a lower-cased set.
/// Registries with stronger normalization rules normalize again on lookup.
pub fn parse_overrides(list: &str) -> HashSet<String> {
    list.split([',', ' ', '\t', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}
