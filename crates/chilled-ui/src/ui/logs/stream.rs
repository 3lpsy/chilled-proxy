//! SSE plumbing: the EventSource, its handlers, and their lifetimes.

use std::collections::VecDeque;

use chilled_wire::LogLine;
use dioxus::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{EventSource, MessageEvent};

/// Total scrollback kept.
pub(super) const MAX_LINES: usize = 5000;

/// The stream plus its callbacks, held together so both live as long as the
/// page (no per-visit leak from `Closure::forget`).
pub(super) struct LogStream {
    es: EventSource,
    _message_handlers: Vec<Closure<dyn FnMut(MessageEvent)>>,
    _event_handlers: Vec<Closure<dyn FnMut(web_sys::Event)>>,
}

impl LogStream {
    /// Close first so no event fires into the about-to-drop closures.
    pub(super) fn close(&self) {
        self.es.close();
    }
}

/// Opens the log stream and wires its events into the given signals.
pub(super) fn connect(
    mut lines: Signal<VecDeque<LogLine>>,
    mut connected: Signal<bool>,
    mut gap: Signal<u64>,
) -> Option<LogStream> {
    let es = EventSource::new("/api/logs?backlog=1000").ok()?;
    let on_log = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        let Some(data) = evt.data().as_string() else {
            return;
        };
        let Ok(line) = serde_json::from_str::<LogLine>(&data) else {
            return;
        };
        let mut buf = lines.write();
        if buf.len() >= MAX_LINES {
            buf.pop_front();
        }
        buf.push_back(line);
    });
    let on_gap = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        let dropped = evt
            .data()
            .as_string()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        gap += dropped;
    });
    let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| connected.set(true));
    let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| connected.set(false));
    let _ = es.add_event_listener_with_callback("log", on_log.as_ref().unchecked_ref());
    let _ = es.add_event_listener_with_callback("gap", on_gap.as_ref().unchecked_ref());
    es.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    Some(LogStream {
        es,
        _message_handlers: vec![on_log, on_gap],
        _event_handlers: vec![on_open, on_error],
    })
}
