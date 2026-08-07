//! `GET /api/logs` — server logs over SSE: backlog, then live follow.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use chilled_wire::LogLine;
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

use crate::logbuf::level_rank;
use crate::state::UiState;

#[derive(Debug, Deserialize)]
pub(crate) struct Params {
    /// Lines of backlog to replay first (capped at the buffer size).
    backlog: Option<usize>,
    /// Minimum severity (`trace`..`error`).
    level: Option<String>,
    /// Target prefix filter (e.g. `chilled_core`).
    target: Option<String>,
    /// `false` closes the stream after the backlog.
    follow: Option<bool>,
}

pub(crate) async fn handle_logs(
    State(state): State<UiState>,
    Query(params): Query<Params>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let min_rank = params.level.as_deref().map(level_rank).unwrap_or(0);
    let target = params.target.unwrap_or_default();
    let follow = params.follow.unwrap_or(true);
    let backlog_max = params.backlog.unwrap_or(200);

    // Subscribe first, then snapshot: anything pushed in between shows up in
    // both and is deduplicated by sequence number below.
    let live = state.log_hub.subscribe();
    let mut backlog = state.log_hub.snapshot();
    if backlog.len() > backlog_max {
        backlog.drain(..backlog.len() - backlog_max);
    }
    let last_seq = backlog.last().map(|l| l.seq).unwrap_or(0);

    let matches = move |line: &Arc<LogLine>| {
        level_rank(&line.level) >= min_rank
            && (target.is_empty() || line.target.starts_with(&target))
    };
    let matches_live = matches.clone();

    let backlog_stream = tokio_stream::iter(
        backlog
            .into_iter()
            .filter(move |l| matches(l))
            .map(|l| Ok(event(&l))),
    );
    let live_stream = BroadcastStream::new(live).filter_map(move |item| match item {
        Ok(line) if line.seq > last_seq && matches_live(&line) => Some(Ok(event(&line))),
        Ok(_) => None,
        // A lagged subscriber lost lines; say how many instead of hiding it.
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
            Some(Ok(Event::default().event("gap").data(n.to_string())))
        }
    });

    let stream: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> = if follow
    {
        Box::pin(backlog_stream.chain(live_stream))
    } else {
        Box::pin(backlog_stream)
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}

fn event(line: &LogLine) -> Event {
    Event::default()
        .event("log")
        .data(serde_json::to_string(line).unwrap_or_default())
}
