//! Pure display helpers, unit-testable off the DOM.

/// Display names for the built-in mounts; custom mounts show their own name.
pub fn friendly_name(mount: &str) -> String {
    match mount {
        "crates" => "Crates".into(),
        "npm" => "NPM".into(),
        "pypi" => "PyPI".into(),
        "maven" => "Maven".into(),
        "gradle-plugins" => "Gradle Plugins".into(),
        "google-maven" => "Google Maven".into(),
        other => other.to_string(),
    }
}

/// A mount cooldown as a short label; zero means disabled.
pub fn cooldown_label(secs: u64) -> String {
    if secs == 0 {
        "off".into()
    } else {
        format!("{secs}s")
    }
}

/// Bytes to a short human string (powers of 1024, one decimal).
pub fn human_size(bytes: i64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes.max(0) as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes.max(0), UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A relative time like "3 h ago" against the given "now" (unix seconds).
pub fn human_time(unix_secs: i64, now_secs: i64) -> String {
    let delta = now_secs - unix_secs;
    if unix_secs <= 0 {
        return "unknown".into();
    }
    if delta < 0 {
        return "just now".into();
    }
    match delta {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", delta / 60),
        3600..=86_399 => format!("{} h ago", delta / 3600),
        86_400..=2_591_999 => format!("{} d ago", delta / 86_400),
        _ => format!("{} mo ago", delta / 2_592_000),
    }
}

/// Absolute timestamp for tooltips (UTC, ISO-ish). The upper bound (year
/// 9999) keeps `Date::to_iso_string` from throwing on garbage mtimes.
pub fn absolute_time(unix_secs: i64) -> String {
    if unix_secs <= 0 || unix_secs > 253_402_300_799 {
        return "unknown".into();
    }
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(unix_secs as f64 * 1000.0));
    date.to_iso_string().as_string().unwrap_or_default()
}

/// Unix seconds now, from the browser clock.
pub fn now_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_names_map_defaults_and_pass_custom() {
        assert_eq!(friendly_name("npm"), "NPM");
        assert_eq!(friendly_name("gradle-plugins"), "Gradle Plugins");
        assert_eq!(friendly_name("corp-mirror"), "corp-mirror");
    }

    #[test]
    fn cooldown_zero_reads_off() {
        assert_eq!(cooldown_label(0), "off");
        assert_eq!(cooldown_label(30), "30s");
    }

    #[test]
    fn sizes_scale_by_1024() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KiB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(human_size(-3), "0 B");
    }

    #[test]
    fn times_are_relative() {
        let now = 1_700_000_000;
        assert_eq!(human_time(now - 30, now), "just now");
        assert_eq!(human_time(now - 120, now), "2 min ago");
        assert_eq!(human_time(now - 7200, now), "2 h ago");
        assert_eq!(human_time(now - 172_800, now), "2 d ago");
        assert_eq!(human_time(0, now), "unknown");
    }
}
