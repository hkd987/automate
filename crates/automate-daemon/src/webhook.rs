use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::job_queue::{Job, JobSender};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct WebhookState {
    pub secrets: Arc<Mutex<HashMap<String, String>>>,
    pub prompts: Arc<Mutex<HashMap<String, String>>>,
    pub job_tx: JobSender,
}

pub fn create_webhook_router(state: WebhookState) -> Router {
    Router::new()
        .route("/hooks/:name", post(handle_webhook))
        .with_state(state)
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

async fn handle_webhook(
    State(state): State<WebhookState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let secrets = state.secrets.lock().await;
    let secret = match secrets.get(&name) {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "webhook not found"})),
            )
                .into_response();
        }
    };
    drop(secrets);

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

    if !verify_hmac(&secret, &body, sig_hex) {
        warn!(webhook = %name, "Invalid HMAC signature");
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid signature"})),
        )
            .into_response();
    }

    let body_str = String::from_utf8_lossy(&body).to_string();

    // Get the prompt for this webhook
    let prompts = state.prompts.lock().await;
    let prompt = prompts
        .get(&name)
        .cloned()
        .unwrap_or_default()
        .replace("$WEBHOOK_BODY", &body_str);
    drop(prompts);

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
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn make_signature(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }

    fn setup_webhook_app(name: &str, secret: &str, prompt: &str) -> Router {
        let mut secrets = HashMap::new();
        secrets.insert(name.to_string(), secret.to_string());
        let mut prompts = HashMap::new();
        prompts.insert(name.to_string(), prompt.to_string());

        let (tx, _rx) = crate::job_queue::create_channel(10);
        let state = WebhookState {
            secrets: Arc::new(Mutex::new(secrets)),
            prompts: Arc::new(Mutex::new(prompts)),
            job_tx: tx,
        };
        create_webhook_router(state)
    }

    #[tokio::test]
    async fn test_valid_hmac_enqueues_job() {
        let (tx, mut rx) = crate::job_queue::create_channel(10);
        let mut secrets = HashMap::new();
        secrets.insert("test-hook".to_string(), "my-secret".to_string());
        let mut prompts = HashMap::new();
        prompts.insert(
            "test-hook".to_string(),
            "Process: $WEBHOOK_BODY".to_string(),
        );

        let state = WebhookState {
            secrets: Arc::new(Mutex::new(secrets)),
            prompts: Arc::new(Mutex::new(prompts)),
            job_tx: tx,
        };
        let app = create_webhook_router(state);

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
        let app = setup_webhook_app("test-hook", "my-secret", "prompt");

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
        let app = setup_webhook_app("test-hook", "my-secret", "prompt");

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
        let app = setup_webhook_app("test-hook", "my-secret", "prompt");

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
        let (tx, mut rx) = crate::job_queue::create_channel(10);
        let mut secrets = HashMap::new();
        secrets.insert("interp".to_string(), "secret".to_string());
        let mut prompts = HashMap::new();
        prompts.insert(
            "interp".to_string(),
            "Handle event: $WEBHOOK_BODY done".to_string(),
        );

        let state = WebhookState {
            secrets: Arc::new(Mutex::new(secrets)),
            prompts: Arc::new(Mutex::new(prompts)),
            job_tx: tx,
        };
        let app = create_webhook_router(state);

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
