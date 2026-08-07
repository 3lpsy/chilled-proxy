//! Per-line display helpers: severity rank, CSS class, timestamp.

/// Numeric severity for min-level filtering; unknown levels rank lowest.
pub(super) fn rank(level: &str) -> u8 {
    match level.to_ascii_uppercase().as_str() {
        "ERROR" => 5,
        "WARN" => 4,
        "INFO" => 3,
        "DEBUG" => 2,
        "TRACE" => 1,
        _ => 0,
    }
}

/// CSS classes colorizing a line by its level.
pub(super) fn level_class(level: &str) -> &'static str {
    match level.to_ascii_uppercase().as_str() {
        "ERROR" => "log-line log-error",
        "WARN" => "log-line log-warn",
        "DEBUG" | "TRACE" => "log-line log-dim",
        _ => "log-line",
    }
}

/// `HH:MM:SS` from epoch millis, browser-local.
pub(super) fn format_ts(ts_ms: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts_ms as f64));
    format!(
        "{:02}:{:02}:{:02}",
        date.get_hours(),
        date.get_minutes(),
        date.get_seconds()
    )
}
