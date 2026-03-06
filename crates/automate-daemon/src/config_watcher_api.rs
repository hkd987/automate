use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config_watcher::ConfigWatcher;
use crate::job_queue::JobSender;
use crate::scheduler::Scheduler;

#[derive(Clone)]
pub struct ConfigWatchAppState {
    pub db: Arc<Mutex<Connection>>,
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

pub fn create_config_watch_router(state: ConfigWatchAppState) -> Router {
    Router::new()
        .route("/config/watch", post(start_watch))
        .route("/config/watch", delete(stop_watch))
        .route("/config/watch", get(watch_status))
        .with_state(state)
}

async fn start_watch(
    State(state): State<ConfigWatchAppState>,
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
        .start_watching(
            path,
            state.db.clone(),
            state.job_tx.clone(),
            state.scheduler.clone(),
        )
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

async fn stop_watch(State(state): State<ConfigWatchAppState>) -> impl IntoResponse {
    state.watcher.stop_watching().await;
    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "stopped"})),
    )
}

async fn watch_status(State(state): State<ConfigWatchAppState>) -> impl IntoResponse {
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
    use crate::db;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn setup_app(dir_path: Option<&std::path::Path>) -> (Router, Arc<ConfigWatcher>) {
        let conn = db::init_db_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let scheduler = Arc::new(Scheduler::new().await.unwrap());
        let watcher = Arc::new(ConfigWatcher::new());

        let state = ConfigWatchAppState {
            db,
            job_tx: tx,
            scheduler,
            watcher: watcher.clone(),
        };

        let _ = dir_path;
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

        let conn = db::init_db_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let scheduler = Arc::new(Scheduler::new().await.unwrap());
        let watcher = Arc::new(ConfigWatcher::new());

        let state = ConfigWatchAppState {
            db,
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
