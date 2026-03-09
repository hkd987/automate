use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::Request;
use tokio::sync::Mutex;
use tower::ServiceExt;

use automate_daemon::api::{AppState, DaemonMeta, WhatsAppQrState};
use automate_daemon::log_stream::LogStreamManager;
use automate_daemon::store_sqlx::SqlxStore;

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

async fn setup_test_app(runtime: Arc<dyn automate_daemon::agent::AgentRuntime>) -> axum::Router {
    let store = SqlxStore::connect_in_memory().await.unwrap();

    let (job_tx, job_rx) = automate_daemon::job_queue::create_channel(100);

    let consumer_store = store.clone();
    let consumer_runtime = runtime.clone();
    tokio::spawn(async move {
        automate_daemon::job_queue::run_consumer(job_rx, consumer_store, consumer_runtime).await;
    });

    let state = AppState {
        store,
        job_tx,
        meta: DaemonMeta {
            start_time: Instant::now(),
            version: "0.1.0-test".to_string(),
            github_repo: "lumatthews/automate".to_string(),
        },
        whatsapp_qr: WhatsAppQrState(Arc::new(Mutex::new(None))),
        log_stream_mgr: Arc::new(LogStreamManager::new()),
        webhook_entries: Arc::new(Mutex::new(HashMap::new())),
    };

    automate_daemon::api::create_router(state)
}

fn make_automation_json(name: &str, prompt: &str) -> String {
    serde_json::json!({
        "name": name,
        "trigger": "manual",
        "prompt": prompt
    })
    .to_string()
}

async fn create_automation(app: &axum::Router, name: &str, prompt: &str) -> u16 {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/automations")
                .header("content-type", "application/json")
                .body(Body::from(make_automation_json(name, prompt)))
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status().as_u16()
}

async fn trigger_run(app: &axum::Router, name: &str) -> serde_json::Value {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/{}/run", name))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn list_runs(app: &axum::Router) -> Vec<serde_json::Value> {
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn list_automations(app: &axum::Router) -> Vec<serde_json::Value> {
    let resp = app
        .clone()
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
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
#[ignore]
async fn test_full_automation_lifecycle() {
    let mock_runtime = Arc::new(MockAgentRuntime {
        output: "lifecycle test done".to_string(),
        exit_code: 0,
    });
    let app = setup_test_app(mock_runtime).await;

    // Create automation
    let status = create_automation(&app, "lifecycle-test", "Run lifecycle test").await;
    assert_eq!(status, 201);

    // Trigger run
    let run_json = trigger_run(&app, "lifecycle-test").await;
    assert_eq!(run_json["status"], "pending");
    let run_id = run_json["run_id"].as_str().unwrap().to_string();

    // Wait for consumer to process
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Verify run completed
    let runs = list_runs(&app).await;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["id"], run_id);
    assert_eq!(runs[0]["automation_name"], "lifecycle-test");
    assert_eq!(runs[0]["status"], "completed");
    assert_eq!(runs[0]["output"], "lifecycle test done");
    assert!(runs[0]["finished_at"].is_string());
}

#[tokio::test]
#[ignore]
async fn test_multiple_automations() {
    let mock_runtime = Arc::new(MockAgentRuntime {
        output: "multi done".to_string(),
        exit_code: 0,
    });
    let app = setup_test_app(mock_runtime).await;

    // Create several automations
    for i in 0..3 {
        let name = format!("multi-auto-{}", i);
        let status = create_automation(&app, &name, &format!("Prompt {}", i)).await;
        assert_eq!(status, 201);
    }

    // Verify all created
    let automations = list_automations(&app).await;
    assert_eq!(automations.len(), 3);

    // Trigger runs for all
    for i in 0..3 {
        let name = format!("multi-auto-{}", i);
        let run_json = trigger_run(&app, &name).await;
        assert_eq!(run_json["status"], "pending");
    }

    // Wait for all to complete
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify all completed
    let runs = list_runs(&app).await;
    assert_eq!(runs.len(), 3);
    for run in &runs {
        assert_eq!(run["status"], "completed");
        assert_eq!(run["output"], "multi done");
    }
}

#[tokio::test]
#[ignore]
async fn test_automation_crud_cycle() {
    let mock_runtime = Arc::new(MockAgentRuntime {
        output: "crud".to_string(),
        exit_code: 0,
    });
    let app = setup_test_app(mock_runtime).await;

    // Create
    let status = create_automation(&app, "crud-test", "Original prompt").await;
    assert_eq!(status, 201);

    // Read (list)
    let automations = list_automations(&app).await;
    assert_eq!(automations.len(), 1);
    assert_eq!(automations[0]["name"], "crud-test");
    assert_eq!(automations[0]["prompt"], "Original prompt");

    // Delete
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/automations/crud-test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // Verify gone
    let automations = list_automations(&app).await;
    assert!(automations.is_empty());

    // Delete again returns 404
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/automations/crud-test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
#[ignore]
async fn test_concurrent_runs() {
    let mock_runtime = Arc::new(MockAgentRuntime {
        output: "concurrent done".to_string(),
        exit_code: 0,
    });
    let app = setup_test_app(mock_runtime).await;

    // Create automation
    let status = create_automation(&app, "concurrent-test", "Run concurrently").await;
    assert_eq!(status, 201);

    // Trigger multiple runs simultaneously
    let mut handles = Vec::new();
    for _ in 0..5 {
        let app_clone = app.clone();
        handles.push(tokio::spawn(async move {
            trigger_run(&app_clone, "concurrent-test").await
        }));
    }

    // Await all triggers
    for handle in handles {
        let run_json = handle.await.unwrap();
        assert_eq!(run_json["status"], "pending");
    }

    // Wait for all to process
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Verify all completed
    let runs = list_runs(&app).await;
    assert_eq!(runs.len(), 5);
    for run in &runs {
        assert_eq!(run["status"], "completed");
        assert_eq!(run["output"], "concurrent done");
    }
}
