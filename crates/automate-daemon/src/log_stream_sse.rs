use std::convert::Infallible;

use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::stream::Stream;
use tracing::{info, warn};

use automate_shared::store::Store;

use crate::api::AppState;

pub async fn sse_handler<S: Store>(
    Path(run_id): Path<String>,
    State(state): State<AppState<S>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mgr = state.log_stream_mgr;
    info!(run_id = %run_id, "SSE connection for run log stream");

    let stream = async_stream::stream! {
        match mgr.subscribe(&run_id).await {
            Some((history, mut rx)) => {
                // Send historical lines first
                for line in history {
                    let json = serde_json::to_string(&line).unwrap_or_default();
                    yield Ok(Event::default().data(json));
                }
                // Stream live updates from broadcast channel
                loop {
                    match rx.recv().await {
                        Ok(line) => {
                            let json = serde_json::to_string(&line).unwrap_or_default();
                            yield Ok(Event::default().data(json));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            yield Ok(Event::default().event("stream_closed").data("{}"));
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!(run_id = %run_id, skipped = n, "SSE subscriber lagged");
                            continue;
                        }
                    }
                }
            }
            None => {
                yield Ok(Event::default()
                    .event("no_stream")
                    .data(r#"{"message":"No active stream for this run"}"#));
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
