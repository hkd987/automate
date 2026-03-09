use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{info, warn};
use uuid::Uuid;

use automate_shared::store::Store;

use crate::api::AppState;
use crate::job_queue::Job;

type HmacSha256 = Hmac<Sha256>;

pub struct WebhookEntry {
    pub secret: String,
    pub prompt: String,
}

pub fn verify_hmac(secret: &str, body: &[u8], signature_hex: &str) -> bool {
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    let Ok(expected) = hex::decode(signature_hex) else {
        return false;
    };
    mac.verify_slice(&expected).is_ok()
}

pub async fn handle_webhook<S: Store>(
    State(state): State<AppState<S>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let entries_lock = state.webhook_entries.lock().await;
    let entry = match entries_lock.get(&name) {
        Some(e) => WebhookEntry {
            secret: e.secret.clone(),
            prompt: e.prompt.clone(),
        },
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "webhook not found"})),
            )
                .into_response();
        }
    };
    drop(entries_lock);

    // Validate HMAC signature
    let signature = match headers.get("X-Signature-256") {
        Some(val) => match val.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "invalid signature header"})),
                )
                    .into_response();
            }
        },
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "missing X-Signature-256 header"})),
            )
                .into_response();
        }
    };

    // Strip optional "sha256=" prefix
    let sig_hex = signature.strip_prefix("sha256=").unwrap_or(&signature);

    if !verify_hmac(&entry.secret, &body, sig_hex) {
        warn!(webhook = %name, "Invalid HMAC signature");
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid signature"})),
        )
            .into_response();
    }

    let body_str = String::from_utf8_lossy(&body).to_string();

    let prompt = entry.prompt.replace("$WEBHOOK_BODY", &body_str);

    let run_id = Uuid::new_v4();
    let mut env = HashMap::new();
    env.insert("WEBHOOK_BODY".to_string(), body_str);

    let job = Job {
        automation_name: name.clone(),
        trigger_source: "webhook".to_string(),
        prompt,
        env,
        max_retries: crate::job_queue::DEFAULT_MAX_RETRIES,
        retry_count: 0,
    };

    if let Err(e) = state.job_tx.send((job, run_id)).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to enqueue: {}", e)})),
        )
            .into_response();
    }

    info!(webhook = %name, run_id = %run_id, "Webhook job enqueued");
    (
        StatusCode::OK,
        Json(serde_json::json!({"run_id": run_id.to_string(), "status": "enqueued"})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{AppState, DaemonMeta, WhatsAppQrState};
    use crate::log_stream::LogStreamManager;
    use crate::store_sqlx::SqlxStore;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use axum::Router;
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    fn make_signature(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }

    async fn setup_webhook_app(
        name: &str,
        secret: &str,
        prompt: &str,
    ) -> (Router, crate::job_queue::JobReceiver) {
        let mut entries = HashMap::new();
        entries.insert(
            name.to_string(),
            WebhookEntry {
                secret: secret.to_string(),
                prompt: prompt.to_string(),
            },
        );

        let store = SqlxStore::connect_in_memory().await.unwrap();
        let (tx, rx) = crate::job_queue::create_channel(10);
        let state = AppState {
            store,
            job_tx: tx,
            meta: DaemonMeta {
                start_time: Instant::now(),
                version: "test".to_string(),
                github_repo: "test/repo".to_string(),
            },
            whatsapp_qr: WhatsAppQrState(Arc::new(Mutex::new(None))),
            log_stream_mgr: Arc::new(LogStreamManager::new()),
            webhook_entries: Arc::new(Mutex::new(entries)),
        };
        let app = Router::new()
            .route("/hooks/:name", post(handle_webhook::<SqlxStore>))
            .with_state(state);
        (app, rx)
    }

    #[tokio::test]
    async fn test_valid_hmac_enqueues_job() {
        let (app, mut rx) =
            setup_webhook_app("test-hook", "my-secret", "Process: $WEBHOOK_BODY").await;

        let body = b"hello webhook";
        let sig = make_signature("my-secret", body);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hooks/test-hook")
                    .header("X-Signature-256", &sig)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        // Verify job was enqueued
        let (job, _run_id) = rx.try_recv().unwrap();
        assert_eq!(job.automation_name, "test-hook");
        assert_eq!(job.trigger_source, "webhook");
        assert_eq!(job.prompt, "Process: hello webhook");
        assert_eq!(
            job.env.get("WEBHOOK_BODY"),
            Some(&"hello webhook".to_string())
        );
    }

    #[tokio::test]
    async fn test_invalid_hmac_returns_401() {
        let (app, _rx) = setup_webhook_app("test-hook", "my-secret", "prompt").await;

        let body = b"hello webhook";
        let bad_sig = "deadbeef";

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hooks/test-hook")
                    .header("X-Signature-256", bad_sig)
                    .body(Body::from(body.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_missing_signature_returns_401() {
        let (app, _rx) = setup_webhook_app("test-hook", "my-secret", "prompt").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hooks/test-hook")
                    .body(Body::from("body"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_unknown_webhook_returns_404() {
        let (app, _rx) = setup_webhook_app("test-hook", "my-secret", "prompt").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hooks/unknown")
                    .header("X-Signature-256", "anything")
                    .body(Body::from("body"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_webhook_body_interpolation() {
        let (app, mut rx) =
            setup_webhook_app("interp", "secret", "Handle event: $WEBHOOK_BODY done").await;

        let body = b"{\"event\":\"push\"}";
        let sig = make_signature("secret", body);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hooks/interp")
                    .header("X-Signature-256", &sig)
                    .body(Body::from(body.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let (job, _) = rx.try_recv().unwrap();
        assert_eq!(job.prompt, "Handle event: {\"event\":\"push\"} done");
    }

    #[test]
    fn test_verify_hmac_correct() {
        let secret = "test-secret";
        let body = b"test body";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let sig = hex::encode(mac.finalize().into_bytes());
        assert!(verify_hmac(secret, body, &sig));
    }

    #[test]
    fn test_verify_hmac_incorrect() {
        assert!(!verify_hmac("secret", b"body", "badhex"));
        assert!(!verify_hmac("secret", b"body", "deadbeefdeadbeef"));
    }
}
