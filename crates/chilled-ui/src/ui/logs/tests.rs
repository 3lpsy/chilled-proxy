use super::line::{level_class, rank};

#[test]
fn ranks_order_severities() {
    assert!(rank("ERROR") > rank("WARN"));
    assert!(rank("WARN") > rank("INFO"));
    assert!(rank("INFO") > rank("DEBUG"));
    assert!(rank("DEBUG") > rank("TRACE"));
    assert_eq!(rank("info"), rank("INFO"));
    assert_eq!(rank("bogus"), 0);
}

#[test]
fn level_classes_colorize_by_severity() {
    assert_eq!(level_class("error"), "log-line log-error");
    assert_eq!(level_class("WARN"), "log-line log-warn");
    assert_eq!(level_class("debug"), "log-line log-dim");
    assert_eq!(level_class("trace"), "log-line log-dim");
    assert_eq!(level_class("INFO"), "log-line");
    assert_eq!(level_class("bogus"), "log-line");
}
