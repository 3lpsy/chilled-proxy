use super::hub::BUFFER_CAP;
use super::{level_rank, LogHub};

#[test]
fn buffer_trims_and_sequences() {
    let hub = LogHub::default();
    for i in 0..(BUFFER_CAP + 10) {
        hub.push("INFO", "test", format!("line {i}"));
    }
    let snap = hub.snapshot();
    assert_eq!(snap.len(), BUFFER_CAP);
    assert_eq!(snap[0].msg, "line 10");
    assert!(snap.windows(2).all(|w| w[0].seq < w[1].seq));
}

#[tokio::test]
async fn broadcast_delivers_to_subscribers() {
    let hub = LogHub::default();
    let mut rx = hub.subscribe();
    hub.push("WARN", "test", "hello".into());
    let line = rx.recv().await.unwrap();
    assert_eq!(line.level, "WARN");
    assert_eq!(line.msg, "hello");
}

#[test]
fn ranks_order_severities() {
    assert!(level_rank("error") > level_rank("warn"));
    assert!(level_rank("warn") > level_rank("info"));
    assert_eq!(level_rank("weird"), 0);
}
