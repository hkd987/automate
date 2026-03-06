use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use automate_shared::config::AutomationDef;
use automate_shared::models::{RunRecord, RunStatus};

use crate::credentials;
use crate::db;
use crate::job_queue::{Job, JobSender};
use crate::updater;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub job_tx: JobSender,
    pub start_time: Instant,
    pub version: String,
    pub github_repo: String,
    pub whatsapp_qr: Arc<Mutex<Option<String>>>,
}

#[derive(Serialize)]
struct HealthResponse {
    version: String,
    uptime_secs: u64,
    scheduler: String,
    channels: ChannelStatus,
}

#[derive(Serialize)]
struct ChannelStatus {
    slack: String,
    whatsapp: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    code: String,
}

impl ErrorResponse {
    fn not_found(msg: String) -> Self {
        Self {
            error: msg,
            code: "NOT_FOUND".to_string(),
        }
    }

    fn conflict(msg: String) -> Self {
        Self {
            error: msg,
            code: "CONFLICT".to_string(),
        }
    }

    fn bad_request(msg: String) -> Self {
        Self {
            error: msg,
            code: "BAD_REQUEST".to_string(),
        }
    }

    fn internal(msg: String) -> Self {
        Self {
            error: msg,
            code: "INTERNAL_ERROR".to_string(),
        }
    }
}

#[derive(Serialize)]
struct RunTriggerResponse {
    run_id: String,
    status: RunStatus,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/automations", post(create_automation))
        .route("/automations", get(list_automations))
        .route("/automations/:name/run", post(trigger_run))
        .route("/automations/:name", delete(delete_automation))
        .route("/runs", get(list_runs))
        .route("/credentials", post(set_credential))
        .route("/credentials/keys", get(list_credential_keys))
        .route("/credentials/:key", delete(delete_credential))
        .route("/update/check", get(check_update))
        .route("/update/apply", post(apply_update))
        .route("/channels/whatsapp/qr", get(get_whatsapp_qr))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    Json(HealthResponse {
        version: state.version.clone(),
        uptime_secs: uptime,
        scheduler: "running".to_string(),
        channels: ChannelStatus {
            slack: "disconnected".to_string(),
            whatsapp: "disconnected".to_string(),
        },
    })
}

async fn create_automation(
    State(state): State<AppState>,
    Json(def): Json<AutomationDef>,
) -> impl IntoResponse {
    if def.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::to_value(ErrorResponse::bad_request(
                    "Automation name is required".to_string(),
                ))
                .unwrap(),
            ),
        )
            .into_response();
    }

    if def.prompt.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::to_value(ErrorResponse::bad_request(
                    "Automation prompt is required".to_string(),
                ))
                .unwrap(),
            ),
        )
            .into_response();
    }

    let conn = state.db.lock().await;
    match db::insert_automation(&conn, &def) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(&def).unwrap()),
        )
            .into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE constraint") {
                (
                    StatusCode::CONFLICT,
                    Json(
                        serde_json::to_value(ErrorResponse::conflict(format!(
                            "Automation '{}' already exists",
                            def.name
                        )))
                        .unwrap(),
                    ),
                )
                    .into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::to_value(ErrorResponse::internal(msg)).unwrap()),
                )
                    .into_response()
            }
        }
    }
}

async fn list_automations(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    match db::list_automations(&conn) {
        Ok(automations) => Json(automations).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::internal(e.to_string())).unwrap()),
        )
            .into_response(),
    }
}

async fn trigger_run(State(state): State<AppState>, Path(name): Path<String>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let automation = match db::get_automation(&conn, &name) {
        Ok(Some(a)) => a,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::to_value(ErrorResponse::not_found(format!(
                        "Automation '{}' not found",
                        name
                    )))
                    .unwrap(),
                ),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::to_value(ErrorResponse::internal(e.to_string())).unwrap()),
            )
                .into_response();
        }
    };

    let run_id = Uuid::new_v4();
    let run = RunRecord {
        id: run_id,
        automation_name: name.clone(),
        status: RunStatus::Pending,
        trigger_source: "manual".to_string(),
        started_at: Utc::now(),
        finished_at: None,
        output: None,
        error: None,
    };

    if let Err(e) = db::insert_run(&conn, &run) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::internal(e.to_string())).unwrap()),
        )
            .into_response();
    }

    // Release the DB lock before sending to channel
    drop(conn);

    let job = Job {
        automation_name: name,
        trigger_source: "manual".to_string(),
        prompt: automation.prompt,
        env: std::collections::HashMap::new(),
        max_retries: crate::job_queue::DEFAULT_MAX_RETRIES,
        retry_count: 0,
    };

    if let Err(e) = state.job_tx.send((job, run_id)).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::to_value(ErrorResponse::internal(format!(
                    "Failed to enqueue job: {}",
                    e
                )))
                .unwrap(),
            ),
        )
            .into_response();
    }

    (
        StatusCode::ACCEPTED,
        Json(RunTriggerResponse {
            run_id: run_id.to_string(),
            status: RunStatus::Pending,
        }),
    )
        .into_response()
}

async fn delete_automation(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let conn = state.db.lock().await;
    match db::delete_automation(&conn, &name) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::to_value(ErrorResponse::not_found(format!(
                    "Automation '{}' not found",
                    name
                )))
                .unwrap(),
            ),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::internal(e.to_string())).unwrap()),
        )
            .into_response(),
    }
}

async fn list_runs(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(100);
    let conn = state.db.lock().await;
    match db::list_runs(&conn, Some(limit)) {
        Ok(runs) => Json(runs).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::internal(e.to_string())).unwrap()),
        )
            .into_response(),
    }
}

// --- Credential endpoints ---

#[derive(Deserialize)]
struct SetCredentialRequest {
    key: String,
    value: String,
}

async fn set_credential(
    State(state): State<AppState>,
    Json(req): Json<SetCredentialRequest>,
) -> impl IntoResponse {
    if req.key.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::to_value(ErrorResponse::bad_request(
                    "Credential key is required".to_string(),
                ))
                .unwrap(),
            ),
        )
            .into_response();
    }

    let conn = state.db.lock().await;
    match credentials::set_credential(&conn, &req.key, &req.value) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::internal(e.to_string())).unwrap()),
        )
            .into_response(),
    }
}

async fn list_credential_keys(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    match credentials::list_credential_keys(&conn) {
        Ok(keys) => Json(keys).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::internal(e.to_string())).unwrap()),
        )
            .into_response(),
    }
}

async fn delete_credential(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let conn = state.db.lock().await;
    match credentials::delete_credential(&conn, &key) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::to_value(ErrorResponse::not_found(format!(
                    "Credential '{}' not found",
                    key
                )))
                .unwrap(),
            ),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::internal(e.to_string())).unwrap()),
        )
            .into_response(),
    }
}

// --- Update endpoints ---

#[derive(Serialize)]
struct UpdateCheckResponse {
    up_to_date: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    update: Option<updater::UpdateInfo>,
}

async fn check_update(State(state): State<AppState>) -> impl IntoResponse {
    match updater::check_for_update(&state.version, &state.github_repo).await {
        Ok(Some(info)) => Json(UpdateCheckResponse {
            up_to_date: false,
            update: Some(info),
        })
        .into_response(),
        Ok(None) => Json(UpdateCheckResponse {
            up_to_date: true,
            update: None,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
struct UpdateApplyResponse {
    status: String,
    version: String,
}

async fn apply_update(State(state): State<AppState>) -> impl IntoResponse {
    let info = match updater::check_for_update(&state.version, &state.github_repo).await {
        Ok(Some(info)) => info,
        Ok(None) => {
            return Json(serde_json::json!({"status": "up_to_date", "version": state.version}))
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    let downloaded = match updater::download_update(&info.download_url).await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Download failed: {}", e)})),
            )
                .into_response();
        }
    };

    // Verify checksum if available
    if let Some(ref expected_sha) = info.sha256 {
        match updater::verify_sha256(&downloaded, expected_sha) {
            Ok(true) => {}
            Ok(false) => {
                let _ = std::fs::remove_file(&downloaded);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "SHA256 checksum mismatch"})),
                )
                    .into_response();
            }
            Err(e) => {
                let _ = std::fs::remove_file(&downloaded);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Checksum verification failed: {}", e)})),
                )
                    .into_response();
            }
        }
    }

    match updater::apply_update(&downloaded) {
        Ok(()) => Json(UpdateApplyResponse {
            status: "updated".to_string(),
            version: info.latest_version,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Apply failed: {}", e)})),
        )
            .into_response(),
    }
}

// --- WhatsApp QR endpoint ---

#[derive(Serialize)]
struct WhatsAppQrResponse {
    qr: String,
}

async fn get_whatsapp_qr(State(state): State<AppState>) -> impl IntoResponse {
    let qr = state.whatsapp_qr.lock().await;
    match qr.as_ref() {
        Some(qr_data) => Json(WhatsAppQrResponse {
            qr: qr_data.clone(),
        })
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::to_value(ErrorResponse::not_found(
                "No QR code available. Either already authenticated or not yet started."
                    .to_string(),
            ))
            .unwrap()),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn setup_app() -> Router {
        let conn = db::init_db_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let state = AppState {
            db,
            job_tx: tx,
            start_time: Instant::now(),
            version: "0.1.0-test".to_string(),
            github_repo: "test/repo".to_string(),
            whatsapp_qr: Arc::new(Mutex::new(None)),
        };
        create_router(state)
    }

    fn make_automation_json(name: &str) -> String {
        serde_json::json!({
            "name": name,
            "trigger": "manual",
            "prompt": "do stuff"
        })
        .to_string()
    }

    #[tokio::test]
    async fn test_health() {
        let app = setup_app().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
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
        assert_eq!(json["version"], "0.1.0-test");
        assert!(json["uptime_secs"].is_number());
        assert_eq!(json["scheduler"], "running");
        assert_eq!(json["channels"]["slack"], "disconnected");
        assert_eq!(json["channels"]["whatsapp"], "disconnected");
    }

    async fn get_json(resp: axum::http::Response<Body>) -> serde_json::Value {
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn test_create_automation() {
        let app = setup_app().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations")
                    .header("content-type", "application/json")
                    .body(Body::from(make_automation_json("test-auto")))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "test-auto");
    }

    #[tokio::test]
    async fn test_create_duplicate_automation() {
        let conn = db::init_db_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let state = AppState {
            db,
            job_tx: tx,
            start_time: Instant::now(),
            version: "0.1.0-test".to_string(),
            github_repo: "test/repo".to_string(),
            whatsapp_qr: Arc::new(Mutex::new(None)),
        };
        let app = create_router(state);

        // First create
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations")
                    .header("content-type", "application/json")
                    .body(Body::from(make_automation_json("dup")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Second create - duplicate
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations")
                    .header("content-type", "application/json")
                    .body(Body::from(make_automation_json("dup")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_list_automations() {
        let conn = db::init_db_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let state = AppState {
            db,
            job_tx: tx,
            start_time: Instant::now(),
            version: "0.1.0-test".to_string(),
            github_repo: "test/repo".to_string(),
            whatsapp_qr: Arc::new(Mutex::new(None)),
        };
        let app = create_router(state);

        // Create two automations
        for name in ["auto-1", "auto-2"] {
            let _ = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/automations")
                        .header("content-type", "application/json")
                        .body(Body::from(make_automation_json(name)))
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        // List
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/automations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_automation() {
        let conn = db::init_db_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let state = AppState {
            db,
            job_tx: tx,
            start_time: Instant::now(),
            version: "0.1.0-test".to_string(),
            github_repo: "test/repo".to_string(),
            whatsapp_qr: Arc::new(Mutex::new(None)),
        };
        let app = create_router(state);

        // Create
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations")
                    .header("content-type", "application/json")
                    .body(Body::from(make_automation_json("to-delete")))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Delete
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/automations/to-delete")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify gone
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/automations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_nonexistent_automation() {
        let app = setup_app().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/automations/no-such")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_trigger_run_not_found() {
        let app = setup_app().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations/nonexistent/run")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_trigger_run_success() {
        let conn = db::init_db_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let state = AppState {
            db,
            job_tx: tx,
            start_time: Instant::now(),
            version: "0.1.0-test".to_string(),
            github_repo: "test/repo".to_string(),
            whatsapp_qr: Arc::new(Mutex::new(None)),
        };
        let app = create_router(state);

        // Create automation first
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations")
                    .header("content-type", "application/json")
                    .body(Body::from(make_automation_json("runnable")))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Trigger run
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations/runnable/run")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["run_id"].is_string());
        assert_eq!(json["status"], "pending");

        // Verify run appears in list
        let resp = app
            .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.len(), 1);
    }

    #[tokio::test]
    async fn test_list_runs_empty() {
        let app = setup_app().await;
        let resp = app
            .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(json.is_empty());
    }

    #[tokio::test]
    async fn test_set_and_list_credentials() {
        let app = setup_app().await;

        // Set a credential
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/credentials")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"key": "MY_KEY", "value": "secret123"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // List keys
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/credentials/keys")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let keys: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert_eq!(keys, vec!["MY_KEY"]);

        // Delete
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/credentials/MY_KEY")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify gone
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/credentials/keys")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let keys: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_credential() {
        let app = setup_app().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/credentials/NOPE")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let json = get_json(resp).await;
        assert_eq!(json["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn test_error_codes_on_not_found() {
        let app = setup_app().await;

        // Delete nonexistent automation returns NOT_FOUND code
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/automations/no-such")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let json = get_json(resp).await;
        assert_eq!(json["code"], "NOT_FOUND");

        // Trigger nonexistent automation returns NOT_FOUND code
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations/nonexistent/run")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let json = get_json(resp).await;
        assert_eq!(json["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn test_error_codes_on_conflict() {
        let conn = db::init_db_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        let (tx, _rx) = crate::job_queue::create_channel(100);
        let state = AppState {
            db,
            job_tx: tx,
            start_time: Instant::now(),
            version: "0.1.0-test".to_string(),
            github_repo: "test/repo".to_string(),
            whatsapp_qr: Arc::new(Mutex::new(None)),
        };
        let app = create_router(state);

        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations")
                    .header("content-type", "application/json")
                    .body(Body::from(make_automation_json("dup2")))
                    .unwrap(),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations")
                    .header("content-type", "application/json")
                    .body(Body::from(make_automation_json("dup2")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let json = get_json(resp).await;
        assert_eq!(json["code"], "CONFLICT");
    }

    #[tokio::test]
    async fn test_bad_request_empty_name() {
        let app = setup_app().await;
        let body = serde_json::json!({
            "name": "",
            "trigger": "manual",
            "prompt": "do stuff"
        })
        .to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = get_json(resp).await;
        assert_eq!(json["code"], "BAD_REQUEST");
    }

    #[tokio::test]
    async fn test_bad_request_empty_prompt() {
        let app = setup_app().await;
        let body = serde_json::json!({
            "name": "test",
            "trigger": "manual",
            "prompt": ""
        })
        .to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/automations")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = get_json(resp).await;
        assert_eq!(json["code"], "BAD_REQUEST");
    }

    #[tokio::test]
    async fn test_bad_request_empty_credential_key() {
        let app = setup_app().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/credentials")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"key": "", "value": "secret"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = get_json(resp).await;
        assert_eq!(json["code"], "BAD_REQUEST");
    }
}
