use std::collections::HashMap;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::Mutex;
use tracing::info;

use automate_daemon::{
    agent, api, channels, job_queue, log_stream, log_watcher, scheduler, store_sqlx::SqlxStore,
    updater, webhook,
};
use automate_shared::store::Store;
use automate_shared::triggers::TriggerDef;

const GITHUB_REPO: &str = "lumatthews/automate";

const VERSION: &str = env!("BUILD_VERSION");

#[derive(Parser)]
#[command(name = "automate-daemon", version = VERSION)]
struct Cli {
    #[arg(short, long, default_value = "127.0.0.1")]
    host: String,
    #[arg(short, long, default_value_t = 4111)]
    port: u16,
    #[arg(short, long)]
    db_path: Option<String>,
    #[arg(long, help = "Check for updates and exit")]
    check_update: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    if cli.check_update {
        match updater::check_for_update(VERSION, GITHUB_REPO).await {
            Ok(Some(info)) => {
                println!(
                    "Update available: {} -> {} ({})",
                    info.current_version, info.latest_version, info.download_url
                );
                if let Some(notes) = &info.release_notes {
                    println!("Release notes:\n{notes}");
                }
                std::process::exit(0);
            }
            Ok(None) => {
                println!("Already up to date (v{VERSION})");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Failed to check for updates: {e}");
                std::process::exit(1);
            }
        }
    }

    let db_url = cli.db_path.map_or_else(
        || {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let dir = std::path::Path::new(&home).join(".automate");
            std::fs::create_dir_all(&dir).ok();
            let db_file = dir.join("config.db").to_string_lossy().to_string();
            format!("sqlite://{}", db_file)
        },
        |path| {
            if path.starts_with("postgres://") || path.starts_with("sqlite://") {
                path
            } else {
                format!("sqlite://{}", path)
            }
        },
    );

    info!(version = VERSION, db_url = %db_url, "Starting automate-daemon");

    let store = SqlxStore::connect(&db_url).await?;

    let (job_tx, job_rx) = job_queue::create_channel(100);

    // Default to ClaudeCode runtime
    let runtime: Arc<dyn agent::AgentRuntime> =
        Arc::new(agent::claude_code::ClaudeCodeRuntime::create());

    // Initialize log stream manager for real-time log streaming
    let log_stream_mgr = Arc::new(log_stream::LogStreamManager::new());

    let consumer_store = store.clone();
    let consumer_runtime = runtime.clone();
    let consumer_log_stream = log_stream_mgr.clone();
    tokio::spawn(async move {
        job_queue::run_consumer_with_log_stream(
            job_rx,
            consumer_store,
            consumer_runtime,
            consumer_log_stream,
        )
        .await;
    });

    // Load all automations once from DB for startup initialization
    let automations = store.list_automations().await?;

    // Initialize scheduler and restore cron jobs
    let sched = scheduler::Scheduler::new().await?;
    {
        let cron_jobs: Vec<(String, String, String)> = automations
            .iter()
            .filter_map(|a| {
                if let TriggerDef::Cron(expr) = &a.trigger {
                    Some((a.name.clone(), expr.clone(), a.prompt.clone()))
                } else {
                    None
                }
            })
            .collect();
        if !cron_jobs.is_empty() {
            info!(count = cron_jobs.len(), "Restoring cron jobs");
            sched.restore_all(cron_jobs, job_tx.clone()).await?;
        }
    }

    // Initialize webhook state from DB
    let webhook_entries = {
        let mut entries = HashMap::new();
        for a in &automations {
            if a.trigger == TriggerDef::Webhook {
                if let Ok(Some(secret)) = store
                    .get_credential(&format!("{}.webhook_secret", a.name))
                    .await
                {
                    entries.insert(
                        a.name.clone(),
                        webhook::WebhookEntry {
                            secret,
                            prompt: a.prompt.clone(),
                        },
                    );
                }
            }
        }
        entries
    };

    // Initialize log watcher manager
    let log_watcher_mgr = Arc::new(log_watcher::LogWatcherManager::new());
    {
        for a in &automations {
            if let TriggerDef::LogPattern(pattern) = &a.trigger {
                if let Some(file) = &a.file {
                    if let Err(e) = log_watcher_mgr
                        .register_watcher(
                            a.name.clone(),
                            file.clone(),
                            pattern,
                            a.prompt.clone(),
                            job_tx.clone(),
                        )
                        .await
                    {
                        tracing::error!(
                            automation = %a.name,
                            error = %e,
                            "Failed to restore log watcher"
                        );
                    }
                }
            }
        }
    }

    // Optionally start channel integrations
    let mut channel_mgr = channels::ChannelManager::new();

    // Load Slack config from credentials if available
    {
        if let (Ok(Some(bot_token)), Ok(Some(app_token))) = (
            store.get_credential("SLACK_BOT_TOKEN").await,
            store.get_credential("SLACK_APP_TOKEN").await,
        ) {
            let allowed_str = store
                .get_credential("SLACK_ALLOWED_USER_IDS")
                .await
                .unwrap_or(None)
                .unwrap_or_default();
            let allowed_user_ids: Vec<String> = allowed_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let slack_config = channels::SlackConfig {
                bot_token,
                app_token,
                allowed_user_ids,
                enabled: true,
            };
            channel_mgr.start_slack(slack_config, job_tx.clone());
        }
    }

    let state = api::AppState {
        store,
        job_tx,
        meta: api::DaemonMeta {
            start_time: std::time::Instant::now(),
            version: VERSION.to_string(),
            github_repo: GITHUB_REPO.to_string(),
        },
        whatsapp_qr: api::WhatsAppQrState(Arc::new(Mutex::new(None))),
        log_stream_mgr,
        webhook_entries: Arc::new(Mutex::new(webhook_entries)),
    };

    let app = api::create_router(state);

    let addr = format!("{}:{}", cli.host, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "Listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    channel_mgr.stop_all();
    info!("Daemon shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("Received Ctrl+C"),
        _ = terminate => info!("Received SIGTERM"),
    }
}
