use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use automate_shared::config::AutomateConfig;
use automate_shared::store::Store;
use automate_shared::triggers::TriggerDef;

use crate::job_queue::JobSender;
use crate::scheduler::Scheduler;

pub struct ConfigWatchState {
    pub active: bool,
    pub path: Option<PathBuf>,
    pub last_reload: Option<String>,
}

pub struct ConfigWatcher {
    state: Arc<Mutex<ConfigWatchState>>,
    cancel_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    _watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
}

impl Default for ConfigWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigWatcher {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ConfigWatchState {
                active: false,
                path: None,
                last_reload: None,
            })),
            cancel_tx: Arc::new(Mutex::new(None)),
            _watcher: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn get_state(&self) -> (bool, Option<PathBuf>, Option<String>) {
        let state = self.state.lock().await;
        (state.active, state.path.clone(), state.last_reload.clone())
    }

    pub async fn start_watching<S: Store>(
        &self,
        path: PathBuf,
        store: S,
        job_tx: JobSender,
        scheduler: Arc<Scheduler>,
    ) -> anyhow::Result<()> {
        // Stop any existing watcher first
        self.stop_watching().await;

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<PathBuf>(100);

        let watch_path = if path.is_file() {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        } else {
            path.clone()
        };

        let config_path = path.clone();
        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    for p in &event.paths {
                        let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if file_name == ".automate.yml" || file_name == ".automate.yaml" {
                            let _ = event_tx.blocking_send(p.clone());
                        }
                    }
                }
            }
        })?;

        watcher.watch(&watch_path, RecursiveMode::Recursive)?;

        let state = self.state.clone();

        // Do an initial load
        let initial_config_file = find_config_file(&config_path);
        if let Some(ref cf) = initial_config_file {
            if let Err(e) = reload_config(cf, &store, &job_tx, &scheduler).await {
                warn!(error = %e, "Initial config load failed");
            } else {
                let mut s = state.lock().await;
                s.last_reload = Some(chrono::Utc::now().to_rfc3339());
            }
        }

        let task_state = state.clone();
        tokio::spawn(async move {
            let debounce_ms = 500u64;
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => {
                        info!("Config watcher cancelled");
                        break;
                    }
                    event = event_rx.recv() => {
                        match event {
                            Some(changed_path) => {
                                // Debounce: drain any additional events within the window
                                tokio::time::sleep(tokio::time::Duration::from_millis(debounce_ms)).await;
                                while event_rx.try_recv().is_ok() {}

                                match reload_config(&changed_path, &store, &job_tx, &scheduler).await {
                                    Ok(()) => {
                                        let mut s = task_state.lock().await;
                                        s.last_reload = Some(chrono::Utc::now().to_rfc3339());
                                        info!(path = %changed_path.display(), "Config reloaded successfully");
                                    }
                                    Err(e) => {
                                        error!(path = %changed_path.display(), error = %e, "Config reload failed, keeping previous config");
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        // Update state
        {
            let mut s = self.state.lock().await;
            s.active = true;
            s.path = Some(path);
        }
        *self.cancel_tx.lock().await = Some(cancel_tx);
        *self._watcher.lock().await = Some(watcher);

        Ok(())
    }

    pub async fn stop_watching(&self) {
        if let Some(tx) = self.cancel_tx.lock().await.take() {
            let _ = tx.send(());
        }
        *self._watcher.lock().await = None;
        let mut s = self.state.lock().await;
        s.active = false;
        s.path = None;
    }
}

fn find_config_file(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let yml = path.join(".automate.yml");
    if yml.exists() {
        return Some(yml);
    }
    let yaml = path.join(".automate.yaml");
    if yaml.exists() {
        return Some(yaml);
    }
    None
}

enum ConfigChange {
    Removed(String),
    Updated(String, automate_shared::config::AutomationDef),
    Added(String, automate_shared::config::AutomationDef),
}

async fn reload_config<S: Store>(
    config_path: &Path,
    store: &S,
    job_tx: &JobSender,
    scheduler: &Arc<Scheduler>,
) -> anyhow::Result<()> {
    let content = tokio::fs::read_to_string(config_path).await?;
    let new_config: AutomateConfig = serde_yaml::from_str(&content)?;

    let current = store.list_automations().await?;
    let current_map: HashMap<String, _> =
        current.into_iter().map(|a| (a.name.clone(), a)).collect();

    let new_map: HashMap<String, _> = new_config
        .automations
        .iter()
        .map(|a| (a.name.clone(), a.clone()))
        .collect();

    let mut changes = Vec::new();

    for name in current_map.keys() {
        if !new_map.contains_key(name) {
            store.delete_automation(name).await?;
            changes.push(ConfigChange::Removed(name.clone()));
        }
    }

    for (name, new_def) in &new_map {
        match current_map.get(name) {
            Some(existing) if existing == new_def => {}
            Some(_) => {
                store.delete_automation(name).await?;
                store.insert_automation(new_def).await?;
                changes.push(ConfigChange::Updated(name.clone(), new_def.clone()));
            }
            None => {
                store.insert_automation(new_def).await?;
                changes.push(ConfigChange::Added(name.clone(), new_def.clone()));
            }
        }
    }

    for change in changes {
        match change {
            ConfigChange::Removed(name) => {
                let _ = scheduler.remove_cron(&name).await;
                info!(automation = %name, "Removed automation via config reload");
            }
            ConfigChange::Updated(name, new_def) => {
                let _ = scheduler.remove_cron(&name).await;
                if let TriggerDef::Cron(ref expr) = new_def.trigger {
                    if let Err(e) = scheduler
                        .register_cron(name.clone(), expr, new_def.prompt.clone(), job_tx.clone())
                        .await
                    {
                        error!(automation = %name, error = %e, "Failed to register updated cron");
                    }
                }
                info!(automation = %name, "Updated automation via config reload");
            }
            ConfigChange::Added(name, new_def) => {
                if let TriggerDef::Cron(ref expr) = new_def.trigger {
                    if let Err(e) = scheduler
                        .register_cron(name.clone(), expr, new_def.prompt.clone(), job_tx.clone())
                        .await
                    {
                        error!(automation = %name, error = %e, "Failed to register new cron");
                    }
                }
                info!(automation = %name, "Added automation via config reload");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store_sqlx::SqlxStore;
    use std::io::Write;

    async fn setup() -> (
        SqlxStore,
        JobSender,
        crate::job_queue::JobReceiver,
        Arc<Scheduler>,
    ) {
        let store = SqlxStore::connect_in_memory().await.unwrap();
        let (tx, rx) = crate::job_queue::create_channel(100);
        let scheduler = Arc::new(Scheduler::new().await.unwrap());
        (store, tx, rx, scheduler)
    }

    fn write_config(dir: &Path, yaml: &str) {
        let path = dir.join(".automate.yml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(yaml.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }

    fn config_with_automation(name: &str, trigger: &str, prompt: &str) -> String {
        format!(
            "automations:\n  - name: {}\n    trigger: {}\n    prompt: {}\n",
            name, trigger, prompt
        )
    }

    fn config_with_automations(entries: &[(&str, &str, &str)]) -> String {
        let mut yaml = "automations:\n".to_string();
        for (name, trigger, prompt) in entries {
            yaml.push_str(&format!(
                "  - name: {}\n    trigger: {}\n    prompt: {}\n",
                name, trigger, prompt
            ));
        }
        yaml
    }

    #[tokio::test]
    async fn test_add_automation_on_file_change() {
        let dir = tempfile::tempdir().unwrap();
        let (store, tx, _rx, scheduler) = setup().await;

        write_config(dir.path(), "automations: []\n");

        let watcher = ConfigWatcher::new();
        watcher
            .start_watching(dir.path().to_path_buf(), store.clone(), tx, scheduler)
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        write_config(
            dir.path(),
            &config_with_automation("new-auto", "manual", "do things"),
        );

        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let automations = store.list_automations().await.unwrap();
        assert_eq!(automations.len(), 1);
        assert_eq!(automations[0].name, "new-auto");
        assert_eq!(automations[0].prompt, "do things");

        watcher.stop_watching().await;
    }

    #[tokio::test]
    async fn test_update_automation_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let (store, tx, _rx, scheduler) = setup().await;

        write_config(
            dir.path(),
            &config_with_automation("my-auto", "manual", "original prompt"),
        );

        let watcher = ConfigWatcher::new();
        watcher
            .start_watching(dir.path().to_path_buf(), store.clone(), tx, scheduler)
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        {
            let automations = store.list_automations().await.unwrap();
            assert_eq!(automations.len(), 1);
            assert_eq!(automations[0].prompt, "original prompt");
        }

        write_config(
            dir.path(),
            &config_with_automation("my-auto", "webhook", "updated prompt"),
        );

        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let automations = store.list_automations().await.unwrap();
        assert_eq!(automations.len(), 1);
        assert_eq!(automations[0].name, "my-auto");
        assert_eq!(automations[0].prompt, "updated prompt");
        assert_eq!(
            automations[0].trigger,
            automate_shared::triggers::TriggerDef::Webhook
        );

        watcher.stop_watching().await;
    }

    #[tokio::test]
    async fn test_remove_automation() {
        let dir = tempfile::tempdir().unwrap();
        let (store, tx, _rx, scheduler) = setup().await;

        write_config(
            dir.path(),
            &config_with_automations(&[
                ("auto-1", "manual", "prompt 1"),
                ("auto-2", "manual", "prompt 2"),
            ]),
        );

        let watcher = ConfigWatcher::new();
        watcher
            .start_watching(dir.path().to_path_buf(), store.clone(), tx, scheduler)
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        {
            let automations = store.list_automations().await.unwrap();
            assert_eq!(automations.len(), 2);
        }

        write_config(
            dir.path(),
            &config_with_automation("auto-1", "manual", "prompt 1"),
        );

        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let automations = store.list_automations().await.unwrap();
        assert_eq!(automations.len(), 1);
        assert_eq!(automations[0].name, "auto-1");

        watcher.stop_watching().await;
    }

    #[tokio::test]
    async fn test_debounce_multiple_writes() {
        let dir = tempfile::tempdir().unwrap();
        let (store, tx, _rx, scheduler) = setup().await;

        write_config(dir.path(), "automations: []\n");

        let watcher = ConfigWatcher::new();
        watcher
            .start_watching(dir.path().to_path_buf(), store.clone(), tx, scheduler)
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        for i in 0..5 {
            write_config(
                dir.path(),
                &config_with_automation(&format!("auto-{}", i), "manual", &format!("prompt-{}", i)),
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let automations = store.list_automations().await.unwrap();
        assert_eq!(automations.len(), 1);
        assert_eq!(automations[0].name, "auto-4");

        watcher.stop_watching().await;
    }

    #[tokio::test]
    async fn test_invalid_yaml_preserves_config() {
        let dir = tempfile::tempdir().unwrap();
        let (store, tx, _rx, scheduler) = setup().await;

        write_config(
            dir.path(),
            &config_with_automation("valid-auto", "manual", "valid prompt"),
        );

        let watcher = ConfigWatcher::new();
        watcher
            .start_watching(dir.path().to_path_buf(), store.clone(), tx, scheduler)
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        {
            let automations = store.list_automations().await.unwrap();
            assert_eq!(automations.len(), 1);
        }

        let path = dir.path().join(".automate.yml");
        std::fs::write(&path, "this is not: [valid: yaml: {{\n").unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let automations = store.list_automations().await.unwrap();
        assert_eq!(automations.len(), 1);
        assert_eq!(automations[0].name, "valid-auto");

        watcher.stop_watching().await;
    }

    #[tokio::test]
    async fn test_watch_state() {
        let dir = tempfile::tempdir().unwrap();
        let (store, tx, _rx, scheduler) = setup().await;

        write_config(dir.path(), "automations: []\n");

        let watcher = ConfigWatcher::new();

        let (active, path, _) = watcher.get_state().await;
        assert!(!active);
        assert!(path.is_none());

        watcher
            .start_watching(dir.path().to_path_buf(), store, tx, scheduler)
            .await
            .unwrap();

        let (active, path, _) = watcher.get_state().await;
        assert!(active);
        assert_eq!(path.unwrap(), dir.path().to_path_buf());

        watcher.stop_watching().await;

        let (active, path, _) = watcher.get_state().await;
        assert!(!active);
        assert!(path.is_none());
    }
}
