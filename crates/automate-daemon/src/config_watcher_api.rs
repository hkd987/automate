use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use automate_shared::store::Store;

use crate::config_watcher::ConfigWatcher;
use crate::job_queue::JobSender;
use crate::scheduler::Scheduler;

#[derive(Clone)]
pub struct ConfigWatchAppState<S: Store> {
    pub store: S,
    pub job_tx: JobSender,
    pub scheduler: Arc<Scheduler>,
    pub watcher: Arc<ConfigWatcher>,
}

#[derive(Deserialize)]
struct WatchRequest {
    path: String,
}

#[derive(Serialize)]
struct WatchStatusResponse {
    active: bool,
    path: Option<String>,
    last_reload: Option<String>,
}

pub fn create_config_watch_router<S: Store>(state: ConfigWatchAppState<S>) -> Router {
    Router::new()
        .route("/config/watch", post(start_watch::<S>))
        .route("/config/watch", delete(stop_watch::<S>))
        .route("/config/watch", get(watch_status::<S>))
        .with_state(state)
}

async fn start_watch<S: Store>(
    State(state): State<ConfigWatchAppState<S>>,
    Json(req): Json<WatchRequest>,
) -> impl IntoResponse {
    let path = std::path::PathBuf::from(&req.path);
    if !path.exists() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Path does not exist: {}", req.path)})),
        )
            .into_response();
    }

    match state
        .watcher
        .start_watching(path, state.store, state.job_tx, state.scheduler)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "watching", "path": req.path})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn stop_watch<S: Store>(State(state): State<ConfigWatchAppState<S>>) -> impl IntoResponse {
    state.watcher.stop_watching().await;
    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "stopped"})),
    )
}

async fn watch_status<S: Store>(State(state): State<ConfigWatchAppState<S>>) -> impl IntoResponse {
    let (active, path, last_reload) = state.watcher.get_state().await;
    Json(WatchStatusResponse {
        active,
        path: path.map(|p| p.to_string_lossy().to_string()),
        last_reload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store_sqlx::SqlxStore;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn setup_app(_dir_path: Option<&std::path::Path>) -> (Router, Arc<ConfigWatcher>) {
        let store = SqlxStore::connect_in_memory().await.unwrap();
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let scheduler = Arc::new(Scheduler::new().await.unwrap());
        let watcher = Arc::new(ConfigWatcher::new());

        let state = ConfigWatchAppState {
            store,
            job_tx: tx,
            scheduler,
            watcher: watcher.clone(),
        };

        (create_config_watch_router(state), watcher)
    }

    #[tokio::test]
    async fn test_watch_status_initially_inactive() {
        let (app, _) = setup_app(None).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/config/watch")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["active"], false);
        assert!(json["path"].is_null());
    }

    #[tokio::test]
    async fn test_start_watch_nonexistent_path() {
        let (app, _) = setup_app(None).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/config/watch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"path": "/nonexistent/path"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_start_and_stop_watch() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".automate.yml"), "automations: []\n").unwrap();

        let store = SqlxStore::connect_in_memory().await.unwrap();
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let scheduler = Arc::new(Scheduler::new().await.unwrap());
        let watcher = Arc::new(ConfigWatcher::new());

        let state = ConfigWatchAppState {
            store,
            job_tx: tx,
            scheduler,
            watcher: watcher.clone(),
        };

        let app = create_config_watch_router(state);

        // Start watching
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/config/watch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"path": dir.path().to_str().unwrap()}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        // Check status
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/config/watch")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["active"], true);

        // Stop watching
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/config/watch")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        // Verify stopped
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/config/watch")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["active"], false);
    }
}
