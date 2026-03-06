use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::{broadcast, RwLock};

const BROADCAST_CAPACITY: usize = 1024;

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub timestamp: DateTime<Utc>,
    pub stream: &'static str, // "stdout" or "stderr"
    pub content: String,
}

pub struct RunLogBroadcaster {
    sender: broadcast::Sender<LogLine>,
    history: Arc<RwLock<Vec<LogLine>>>,
}

impl RunLogBroadcaster {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            sender,
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn send(&self, line: LogLine) {
        {
            let history = self.history.clone();
            let line_clone = line.clone();
            tokio::spawn(async move {
                history.write().await.push(line_clone);
            });
        }
        // Ignore send error (no active receivers)
        let _ = self.sender.send(line);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogLine> {
        self.sender.subscribe()
    }

    pub async fn get_history(&self) -> Vec<LogLine> {
        self.history.read().await.clone()
    }
}

pub struct LogStreamManager {
    streams: Arc<RwLock<HashMap<String, Arc<RunLogBroadcaster>>>>,
}

impl LogStreamManager {
    pub fn new() -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_stream(&self, run_id: &str) -> Arc<RunLogBroadcaster> {
        let broadcaster = Arc::new(RunLogBroadcaster::new());
        self.streams
            .write()
            .await
            .insert(run_id.to_string(), broadcaster.clone());
        broadcaster
    }

    pub async fn get_broadcaster(&self, run_id: &str) -> Option<Arc<RunLogBroadcaster>> {
        self.streams.read().await.get(run_id).cloned()
    }

    pub async fn subscribe(
        &self,
        run_id: &str,
    ) -> Option<(Vec<LogLine>, broadcast::Receiver<LogLine>)> {
        let streams = self.streams.read().await;
        let broadcaster = streams.get(run_id)?;
        let history = broadcaster.get_history().await;
        let receiver = broadcaster.subscribe();
        Some((history, receiver))
    }

    pub async fn close_stream(&self, run_id: &str) {
        self.streams.write().await.remove(run_id);
    }

    pub async fn get_history(&self, run_id: &str) -> Vec<LogLine> {
        let streams = self.streams.read().await;
        match streams.get(run_id) {
            Some(broadcaster) => broadcaster.get_history().await,
            None => Vec::new(),
        }
    }
}

impl Default for LogStreamManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_send() {
        let mgr = LogStreamManager::new();
        let broadcaster = mgr.create_stream("run-1").await;
        let mut rx = broadcaster.subscribe();

        let line = LogLine {
            timestamp: Utc::now(),
            stream: "stdout",
            content: "hello world".to_string(),
        };
        broadcaster.send(line.clone());

        let received = rx.recv().await.unwrap();
        assert_eq!(received.content, "hello world");
        assert_eq!(received.stream, "stdout");
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let mgr = LogStreamManager::new();
        let broadcaster = mgr.create_stream("run-2").await;
        let mut rx1 = broadcaster.subscribe();
        let mut rx2 = broadcaster.subscribe();

        broadcaster.send(LogLine {
            timestamp: Utc::now(),
            stream: "stdout",
            content: "line 1".to_string(),
        });

        let r1 = rx1.recv().await.unwrap();
        let r2 = rx2.recv().await.unwrap();
        assert_eq!(r1.content, "line 1");
        assert_eq!(r2.content, "line 1");
    }

    #[tokio::test]
    async fn test_subscribe_late_gets_history() {
        let mgr = LogStreamManager::new();
        let broadcaster = mgr.create_stream("run-3").await;

        broadcaster.send(LogLine {
            timestamp: Utc::now(),
            stream: "stdout",
            content: "early line".to_string(),
        });

        // Allow the spawn to complete
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let (history, _rx) = mgr.subscribe("run-3").await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "early line");
    }

    #[tokio::test]
    async fn test_close_stream() {
        let mgr = LogStreamManager::new();
        let broadcaster = mgr.create_stream("run-4").await;
        let mut rx = broadcaster.subscribe();

        mgr.close_stream("run-4").await;

        // After closing, subscribe should return None
        assert!(mgr.subscribe("run-4").await.is_none());

        // Existing receiver should get a closed/lagged error on next send attempt
        // since the broadcaster is dropped (no more senders)
        // But the receiver itself still exists - it just won't get new messages
        // The sender is still alive in the Arc, so let's verify the stream is removed
        assert!(mgr.get_broadcaster("run-4").await.is_none());
    }

    #[tokio::test]
    async fn test_get_history() {
        let mgr = LogStreamManager::new();
        let broadcaster = mgr.create_stream("run-5").await;

        broadcaster.send(LogLine {
            timestamp: Utc::now(),
            stream: "stdout",
            content: "line 1".to_string(),
        });
        broadcaster.send(LogLine {
            timestamp: Utc::now(),
            stream: "stderr",
            content: "err 1".to_string(),
        });

        // Allow spawns to complete
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let history = mgr.get_history("run-5").await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].stream, "stdout");
        assert_eq!(history[1].stream, "stderr");
    }

    #[tokio::test]
    async fn test_get_history_nonexistent() {
        let mgr = LogStreamManager::new();
        let history = mgr.get_history("no-such-run").await;
        assert!(history.is_empty());
    }
}
