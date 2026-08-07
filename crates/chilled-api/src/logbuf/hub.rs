//! The shared log hub: a bounded backlog plus a live broadcast feed.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chilled_wire::LogLine;
use tokio::sync::broadcast;

use crate::time::now_ms;

/// Backlog capacity — enough scrollback to be useful, bounded to stay cheap.
pub(super) const BUFFER_CAP: usize = 1000;

/// Broadcast capacity per subscriber before it lags.
const CHANNEL_CAP: usize = 256;

/// The shared hub: a bounded backlog plus a live feed.
pub struct LogHub {
    buf: Mutex<VecDeque<Arc<LogLine>>>,
    tx: broadcast::Sender<Arc<LogLine>>,
    seq: AtomicU64,
}

impl Default for LogHub {
    fn default() -> Self {
        LogHub {
            buf: Mutex::new(VecDeque::with_capacity(BUFFER_CAP)),
            tx: broadcast::channel(CHANNEL_CAP).0,
            seq: AtomicU64::new(1),
        }
    }
}

impl LogHub {
    /// Records one line: appended to the bounded backlog and broadcast. One
    /// lock orders seq/buffer/send — the SSE backlog/live dedup relies on it.
    pub fn push(&self, level: &str, target: &str, msg: String) {
        let mut buf = match self.buf.lock() {
            Ok(buf) => buf,
            // Poisoned (a panic mid-push): drop the line rather than the tee.
            Err(_) => return,
        };
        let line = Arc::new(LogLine {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            ts_ms: now_ms(),
            level: level.to_owned(),
            target: target.to_owned(),
            msg,
        });
        if buf.len() >= BUFFER_CAP {
            buf.pop_front();
        }
        buf.push_back(line.clone());
        let _ = self.tx.send(line);
    }

    /// The current backlog, oldest first.
    pub(crate) fn snapshot(&self) -> Vec<Arc<LogLine>> {
        self.buf
            .lock()
            .map(|buf| buf.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// A live subscription; subscribe *before* snapshotting to close the gap.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<LogLine>> {
        self.tx.subscribe()
    }
}

/// Severity rank for min-level filtering (unknown labels pass everything).
pub(crate) fn level_rank(level: &str) -> u8 {
    match level.to_ascii_uppercase().as_str() {
        "ERROR" => 5,
        "WARN" => 4,
        "INFO" => 3,
        "DEBUG" => 2,
        "TRACE" => 1,
        _ => 0,
    }
}
