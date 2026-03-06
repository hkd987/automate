use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::Request;
use rusqlite::Connection;
use tokio::sync::Mutex;
use tower::ServiceExt;

// We test the full pipeline: register automation -> trigger run -> agent runs -> verify result

struct MockAgentRuntime {
    output: String,
    exit_code: i32,
}

#[async_trait::async_trait]
impl automate_daemon::agent::AgentRuntime for MockAgentRuntime {
    async fn check_installed(&self) -> bool {
        true
    }

    async fn run(
        &self,
        _prompt: &str,
        _env: HashMap<String, String>,
    ) -> Result<automate_daemon::agent::RunOutput, automate_daemon::agent::AgentError> {
        Ok(automate_daemon::agent::RunOutput {
            stdout: self.output.clone(),
            stderr: String::new(),
            exit_code: self.exit_code,
            duration: Duration::from_millis(42),
        })
    }
}

fn setup_test_app(
    runtime: Arc<dyn automate_daemon::agent::AgentRuntime>,
) -> (axum::Router, Arc<Mutex<Connection>>) {
    let conn = automate_daemon::db::init_db_in_memory().unwrap();
    let db = Arc::new(Mutex::new(conn));

    let (job_tx, job_rx) = automate_daemon::job_queue::create_channel(100);

    let consumer_db = db.clone();
    let consumer_runtime = runtime.clone();
    tokio::spawn(async move {
        automate_daemon::job_queue::run_consumer(job_rx, consumer_db, consumer_runtime).await;
    });

    let state = automate_daemon::api::AppState {
        db: db.clone(),
        job_tx,
        start_time: Instant::now(),
        version: "0.1.0-test".to_string(),
        github_repo: "lumatthews/automate".to_string(),
        whatsapp_qr: Arc::new(tokio::sync::Mutex::new(None)),
    };

    let app = automate_daemon::api::create_router(state);
    (app, db)
}

#[tokio::test]
async fn e2e_register_trigger_and_verify_run() {
    let mock_runtime = Arc::new(MockAgentRuntime {
        output: "e2e test result".to_string(),
        exit_code: 0,
    });

    let (app, _db) = setup_test_app(mock_runtime);

    // Step 1: Register an automation
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/automations")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "e2e-test",
                        "trigger": "manual",
                        "prompt": "Run the e2e test"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);

    // Step 2: Trigger a manual run
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/automations/e2e-test/run")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 202);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let run_id = json["run_id"].as_str().unwrap().to_string();
    assert_eq!(json["status"], "pending");

    // Step 3: Wait for the consumer to process the job
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Step 4: Verify run history shows correct status and output
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let runs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(runs.len(), 1);

    let run = &runs[0];
    assert_eq!(run["id"], run_id);
    assert_eq!(run["automation_name"], "e2e-test");
    assert_eq!(run["status"], "completed");
    assert_eq!(run["output"], "e2e test result");
    assert!(run["finished_at"].is_string());
}

#[tokio::test]
async fn e2e_failing_agent_records_error() {
    struct FailingAgent;

    #[async_trait::async_trait]
    impl automate_daemon::agent::AgentRuntime for FailingAgent {
        async fn check_installed(&self) -> bool {
            true
        }

        async fn run(
            &self,
            _prompt: &str,
            _env: HashMap<String, String>,
        ) -> Result<automate_daemon::agent::RunOutput, automate_daemon::agent::AgentError> {
            Err(automate_daemon::agent::AgentError::ProcessFailed(
                "agent crashed".to_string(),
            ))
        }
    }

    let failing_runtime = Arc::new(FailingAgent);
    let (app, _db) = setup_test_app(failing_runtime);

    // Register
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/automations")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "fail-auto",
                        "trigger": "manual",
                        "prompt": "this will fail"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Trigger
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/automations/fail-auto/run")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify failure is recorded
    let resp = app
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let runs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["status"], "failed");
    assert!(runs[0]["error"].as_str().unwrap().contains("agent crashed"));
}
