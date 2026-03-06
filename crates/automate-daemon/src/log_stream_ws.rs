use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, State, WebSocketUpgrade,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use tracing::{info, warn};

use crate::log_stream::LogStreamManager;

#[derive(Clone)]
pub struct WsState {
    pub log_stream_mgr: Arc<LogStreamManager>,
}

pub fn create_ws_router(state: WsState) -> Router {
    Router::new()
        .route("/ws/runs/:id/stream", get(ws_handler))
        .with_state(state)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(run_id): Path<String>,
    State(state): State<WsState>,
) -> impl IntoResponse {
    info!(run_id = %run_id, "WebSocket upgrade request for run log stream");
    ws.on_upgrade(move |socket| handle_socket(socket, run_id, state))
}

async fn handle_socket(socket: WebSocket, run_id: String, state: WsState) {
    let (mut sender, mut receiver) = socket.split();

    // Try to subscribe to the stream
    let subscription = state.log_stream_mgr.subscribe(&run_id).await;

    match subscription {
        Some((history, mut rx)) => {
            // Send historical lines first
            for line in history {
                let json = serde_json::to_string(&line).unwrap_or_default();
                if sender.send(Message::Text(json)).await.is_err() {
                    return;
                }
            }

            // Spawn a task to forward broadcast messages to the WebSocket
            let send_task = tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(line) => {
                            let json = serde_json::to_string(&line).unwrap_or_default();
                            if sender.send(Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            // Stream closed, send a completion message
                            let _ = sender
                                .send(Message::Text(r#"{"type":"stream_closed"}"#.to_string()))
                                .await;
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!(run_id = %run_id, skipped = n, "WebSocket subscriber lagged");
                            continue;
                        }
                    }
                }
            });

            // Wait for client disconnect
            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }

            send_task.abort();
        }
        None => {
            // No active stream for this run_id
            let _ = sender
                .send(Message::Text(
                    r#"{"type":"no_stream","message":"No active stream for this run"}"#.to_string(),
                ))
                .await;
            let _ = sender.send(Message::Close(None)).await;
        }
    }
}
