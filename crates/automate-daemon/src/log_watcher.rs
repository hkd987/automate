use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use tokio::sync::Mutex;
use tracing::{error, info};
use uuid::Uuid;

use crate::job_queue::{Job, JobSender};

struct WatcherEntry {
    _watcher: RecommendedWatcher,
    cancel_tx: tokio::sync::oneshot::Sender<()>,
}

struct FileState {
    offset: u64,
    #[cfg(unix)]
    inode: Option<u64>,
}

pub struct LogWatcherManager {
    watchers: Arc<Mutex<HashMap<String, WatcherEntry>>>,
}

impl Default for LogWatcherManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LogWatcherManager {
    pub fn new() -> Self {
        Self {
            watchers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register_watcher(
        &self,
        name: String,
        file_path: PathBuf,
        pattern: &str,
        prompt: String,
        job_sender: JobSender,
    ) -> anyhow::Result<()> {
        let regex = Regex::new(pattern)?;
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        // Track file state (offset + inode for rotation detection)
        let file_state = Arc::new(Mutex::new(FileState {
            offset: Self::file_size(&file_path),
            #[cfg(unix)]
            inode: Self::file_inode(&file_path),
        }));

        let watch_state = file_state.clone();

        // Create an async channel for notify events
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<()>(100);

        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    let _ = event_tx.blocking_send(());
                }
            }
        })?;

        // Watch the parent directory (file may not exist yet / may be recreated on rotation)
        let watch_dir = file_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;

        // Spawn the processing task
        let task_path = file_path.clone();
        let task_name = name.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => {
                        info!(watcher = %task_name, "Log watcher cancelled");
                        break;
                    }
                    event = event_rx.recv() => {
                        if event.is_none() {
                            break;
                        }
                        Self::process_new_lines(
                            &task_path,
                            &watch_state,
                            &regex,
                            &task_name,
                            &prompt,
                            &job_sender,
                        )
                        .await;
                    }
                }
            }
        });

        let entry = WatcherEntry {
            _watcher: watcher,
            cancel_tx,
        };
        self.watchers.lock().await.insert(name.clone(), entry);
        info!(watcher = %name, path = %file_path.display(), pattern = %pattern, "Log watcher registered");
        Ok(())
    }

    pub async fn remove_watcher(&self, name: &str) -> bool {
        let mut watchers = self.watchers.lock().await;
        if let Some(entry) = watchers.remove(name) {
            let _ = entry.cancel_tx.send(());
            info!(watcher = %name, "Log watcher removed");
            true
        } else {
            false
        }
    }

    fn file_size(path: &PathBuf) -> u64 {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }

    #[cfg(unix)]
    fn file_inode(path: &PathBuf) -> Option<u64> {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| m.ino())
    }

    async fn process_new_lines(
        path: &Path,
        state: &Arc<Mutex<FileState>>,
        regex: &Regex,
        name: &str,
        prompt: &str,
        job_sender: &JobSender,
    ) {
        let mut file_state = state.lock().await;

        let path_clone = path.to_path_buf();
        let start_offset = file_state.offset;
        #[cfg(unix)]
        let prev_inode = file_state.inode;
        let regex_clone = regex.clone();
        let read_result = tokio::task::spawn_blocking(move || {
            let file = match std::fs::File::open(&path_clone) {
                Ok(f) => f,
                Err(_) => return (start_offset, Vec::new(), None),
            };

            let metadata = file.metadata().ok();
            let file_len = metadata.as_ref().map(|m| m.len()).unwrap_or(0);

            #[cfg(unix)]
            let current_inode = {
                use std::os::unix::fs::MetadataExt;
                metadata.as_ref().map(|m| m.ino())
            };
            #[cfg(not(unix))]
            let current_inode: Option<u64> = None;

            let mut offset = start_offset;

            // Handle log rotation: reset if file is smaller than offset
            // or if the inode changed (file was replaced)
            if file_len < offset {
                offset = 0;
            }

            #[cfg(unix)]
            if let (Some(prev), Some(curr)) = (prev_inode, current_inode) {
                if prev != curr {
                    offset = 0;
                }
            }

            if file_len <= offset {
                return (offset, Vec::new(), current_inode);
            }

            let mut reader = BufReader::new(file);
            if reader.seek(SeekFrom::Start(offset)).is_err() {
                return (offset, Vec::new(), current_inode);
            }

            let mut matches = Vec::new();
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(n) => {
                        offset += n as u64;
                        let trimmed = line.trim().to_string();
                        if regex_clone.is_match(&trimmed) {
                            matches.push(trimmed);
                        }
                    }
                    Err(_) => break,
                }
            }
            (offset, matches, current_inode)
        })
        .await;

        let (new_offset, matches, _new_inode) = match read_result {
            Ok(result) => result,
            Err(_) => return,
        };

        file_state.offset = new_offset;
        #[cfg(unix)]
        {
            file_state.inode = _new_inode;
        }

        for matched_line in matches {
            let run_id = Uuid::new_v4();
            let interpolated_prompt = prompt.replace("$LOG_MATCH", &matched_line);
            let mut env = HashMap::new();
            env.insert("LOG_MATCH".to_string(), matched_line.clone());

            let job = Job {
                automation_name: name.to_string(),
                trigger_source: "log_watcher".to_string(),
                prompt: interpolated_prompt,
                env,
                max_retries: crate::job_queue::DEFAULT_MAX_RETRIES,
                retry_count: 0,
            };

            if let Err(e) = job_sender.send((job, run_id)).await {
                error!(watcher = %name, error = %e, "Failed to enqueue log watcher job");
            } else {
                info!(watcher = %name, run_id = %run_id, matched_line = %matched_line, "Log watcher job enqueued");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn test_log_watcher_pattern_match() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        // Create the file first
        std::fs::write(&log_path, "").unwrap();

        let manager = LogWatcherManager::new();
        let (tx, mut rx) = crate::job_queue::create_channel(10);

        manager
            .register_watcher(
                "test-watcher".to_string(),
                log_path.clone(),
                "ERROR",
                "Handle: $LOG_MATCH".to_string(),
                tx,
            )
            .await
            .unwrap();

        // Give watcher time to initialize
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Append matching line
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log_path)
                .unwrap();
            writeln!(f, "ERROR: something broke").unwrap();
        }

        // Wait for the job
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await;
        assert!(result.is_ok(), "Should receive a job for matching line");
        let (job, _) = result.unwrap().unwrap();
        assert_eq!(job.automation_name, "test-watcher");
        assert_eq!(job.trigger_source, "log_watcher");
        assert_eq!(job.prompt, "Handle: ERROR: something broke");
        assert_eq!(
            job.env.get("LOG_MATCH"),
            Some(&"ERROR: something broke".to_string())
        );
    }

    #[tokio::test]
    async fn test_log_watcher_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        std::fs::write(&log_path, "").unwrap();

        let manager = LogWatcherManager::new();
        let (tx, mut rx) = crate::job_queue::create_channel(10);

        manager
            .register_watcher(
                "no-match".to_string(),
                log_path.clone(),
                "ERROR",
                "prompt".to_string(),
                tx,
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Append non-matching line
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log_path)
                .unwrap();
            writeln!(f, "INFO: all good").unwrap();
        }

        // Should not receive any job
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await;
        assert!(
            result.is_err(),
            "Should not receive a job for non-matching line"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_log_watcher_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("rotating.log");
        std::fs::write(&log_path, "initial content\n").unwrap();

        let manager = LogWatcherManager::new();
        let (tx, mut rx) = crate::job_queue::create_channel(10);

        manager
            .register_watcher(
                "rotation-test".to_string(),
                log_path.clone(),
                "ERROR",
                "prompt: $LOG_MATCH".to_string(),
                tx,
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Simulate log rotation: remove and recreate the file.
        // This changes the inode, which the watcher uses to detect rotation
        // and reset its read offset. Retry a few times to handle environments
        // where notify events may be delayed.
        for attempt in 0..5 {
            std::fs::remove_file(&log_path).ok();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            std::fs::write(&log_path, "ERROR: after rotation\n").unwrap();

            match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
                Ok(Some((job, _))) => {
                    assert!(job.prompt.contains("after rotation"));
                    return;
                }
                _ if attempt < 4 => {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    continue;
                }
                _ => panic!("Should detect content after rotation after multiple attempts"),
            }
        }
    }

    #[tokio::test]
    async fn test_remove_watcher() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("remove.log");
        std::fs::write(&log_path, "").unwrap();

        let manager = LogWatcherManager::new();
        let (tx, _rx) = crate::job_queue::create_channel(10);

        manager
            .register_watcher(
                "to-remove".to_string(),
                log_path,
                "ERROR",
                "prompt".to_string(),
                tx,
            )
            .await
            .unwrap();

        assert!(manager.remove_watcher("to-remove").await);
        assert!(!manager.remove_watcher("to-remove").await);
    }
}
