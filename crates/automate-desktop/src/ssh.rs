use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::client;
use russh::ChannelMsg;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum SshError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    #[error("command execution failed: {0}")]
    ExecFailed(String),
    #[error("file transfer failed: {0}")]
    TransferFailed(String),
    #[error("key loading failed: {0}")]
    KeyError(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("not connected")]
    NotConnected,
}

struct ClientHandler;

impl client::Handler for ClientHandler {
    type Error = russh::Error;
}

type HandleInner = Arc<Mutex<Option<client::Handle<ClientHandler>>>>;

pub struct SshConnection {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_path: PathBuf,
    handle: HandleInner,
}

impl SshConnection {
    pub fn new(host: String, port: u16, user: String, key_path: PathBuf) -> Self {
        Self {
            host,
            port,
            user,
            key_path,
            handle: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn connect(&self) -> Result<(), SshError> {
        let key_pair = russh_keys::load_secret_key(&self.key_path, None)
            .map_err(|e| SshError::KeyError(e.to_string()))?;

        let config = Arc::new(client::Config::default());
        let addr = (self.host.as_str(), self.port);
        let sh = ClientHandler;

        let mut h =
            tokio::time::timeout(Duration::from_secs(30), client::connect(config, addr, sh))
                .await
                .map_err(|_| SshError::Timeout("connection timed out".into()))?
                .map_err(|e| SshError::ConnectionFailed(e.to_string()))?;

        let auth_ok = h
            .authenticate_publickey(&self.user, Arc::new(key_pair))
            .await
            .map_err(|e| SshError::AuthFailed(e.to_string()))?;

        if !auth_ok {
            return Err(SshError::AuthFailed("server rejected public key".into()));
        }

        *self.handle.lock().await = Some(h);
        Ok(())
    }

    pub async fn exec_command(&self, command: &str) -> Result<String, SshError> {
        let mut guard = self.handle.lock().await;
        let h = guard.as_mut().ok_or(SshError::NotConnected)?;

        let mut channel = h
            .channel_open_session()
            .await
            .map_err(|e| SshError::ExecFailed(e.to_string()))?;

        // Drop the lock before channel operations
        drop(guard);

        channel
            .exec(true, command)
            .await
            .map_err(|e| SshError::ExecFailed(e.to_string()))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status: Option<u32> = None;

        loop {
            let msg = tokio::time::timeout(Duration::from_secs(120), channel.wait())
                .await
                .map_err(|_| SshError::Timeout("command timed out".into()))?;

            match msg {
                Some(ChannelMsg::Data { ref data }) => {
                    stdout.extend_from_slice(data);
                }
                Some(ChannelMsg::ExtendedData { ref data, ext }) => {
                    if ext == 1 {
                        stderr.extend_from_slice(data);
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status: s }) => {
                    exit_status = Some(s);
                }
                Some(ChannelMsg::Eof) | None => break,
                _ => {}
            }
        }

        if let Some(code) = exit_status {
            if code != 0 {
                let err_msg = String::from_utf8_lossy(&stderr);
                return Err(SshError::ExecFailed(format!(
                    "exit code {}: {}",
                    code,
                    err_msg.trim()
                )));
            }
        }

        Ok(String::from_utf8_lossy(&stdout).trim().to_string())
    }

    pub async fn detect_arch(&self) -> Result<String, SshError> {
        self.exec_command("uname -m").await
    }

    pub async fn scp_upload(
        &self,
        local_path: &PathBuf,
        remote_path: &str,
    ) -> Result<(), SshError> {
        let mut guard = self.handle.lock().await;
        let h = guard.as_mut().ok_or(SshError::NotConnected)?;

        let data = tokio::fs::read(local_path)
            .await
            .map_err(|e| SshError::TransferFailed(format!("failed to read local file: {}", e)))?;

        let mut channel = h
            .channel_open_session()
            .await
            .map_err(|e| SshError::TransferFailed(e.to_string()))?;

        drop(guard);

        let escaped = remote_path.replace("'", "'\\''");
        let cmd = format!("cat > '{}'", escaped);
        channel
            .exec(true, cmd.as_str())
            .await
            .map_err(|e| SshError::TransferFailed(e.to_string()))?;

        for chunk in data.chunks(32768) {
            channel
                .data(chunk)
                .await
                .map_err(|e| SshError::TransferFailed(e.to_string()))?;
        }

        channel
            .eof()
            .await
            .map_err(|e| SshError::TransferFailed(e.to_string()))?;

        loop {
            match channel.wait().await {
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    if exit_status != 0 {
                        return Err(SshError::TransferFailed(format!(
                            "remote write failed with exit code {}",
                            exit_status
                        )));
                    }
                }
                Some(ChannelMsg::Eof) | None => break,
                _ => {}
            }
        }

        Ok(())
    }

    pub async fn create_ssh_tunnel(
        &self,
        local_port: u16,
        remote_port: u16,
    ) -> Result<(), SshError> {
        // Verify we're connected
        {
            let guard = self.handle.lock().await;
            if guard.is_none() {
                return Err(SshError::NotConnected);
            }
        }

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", local_port))
            .await
            .map_err(|e| SshError::ConnectionFailed(format!("failed to bind local port: {}", e)))?;

        let handle = Arc::clone(&self.handle);

        tokio::spawn(async move {
            loop {
                let (mut local_stream, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };

                let handle = Arc::clone(&handle);
                tokio::spawn(async move {
                    let channel = {
                        let mut guard = handle.lock().await;
                        let h = match guard.as_mut() {
                            Some(h) => h,
                            None => return,
                        };
                        match h
                            .channel_open_direct_tcpip(
                                "127.0.0.1",
                                remote_port.into(),
                                "127.0.0.1",
                                0,
                            )
                            .await
                        {
                            Ok(c) => c,
                            Err(e) => {
                                tracing::error!("failed to open tunnel channel: {}", e);
                                return;
                            }
                        }
                    };

                    let mut stream = channel.into_stream();
                    if let Err(e) =
                        tokio::io::copy_bidirectional(&mut local_stream, &mut stream).await
                    {
                        tracing::debug!("tunnel connection closed: {}", e);
                    }
                });
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_error_variants() {
        let errors = vec![
            SshError::ConnectionFailed("test".into()),
            SshError::AuthFailed("test".into()),
            SshError::ExecFailed("test".into()),
            SshError::TransferFailed("test".into()),
            SshError::KeyError("test".into()),
            SshError::Timeout("test".into()),
            SshError::NotConnected,
        ];
        for err in &errors {
            let msg = err.to_string();
            assert!(!msg.is_empty());
        }
    }

    #[test]
    fn test_ssh_error_display() {
        assert_eq!(
            SshError::ConnectionFailed("refused".into()).to_string(),
            "connection failed: refused"
        );
        assert_eq!(SshError::Timeout("30s".into()).to_string(), "timeout: 30s");
        assert_eq!(SshError::NotConnected.to_string(), "not connected");
    }

    #[test]
    fn test_key_path_resolution() {
        let conn = SshConnection::new(
            "example.com".into(),
            22,
            "user".into(),
            PathBuf::from("/home/user/.ssh/id_ed25519"),
        );
        assert_eq!(conn.key_path, PathBuf::from("/home/user/.ssh/id_ed25519"));
        assert_eq!(conn.host, "example.com");
        assert_eq!(conn.port, 22);
    }

    #[test]
    fn test_ssh_connection_default_state() {
        let conn = SshConnection::new("host".into(), 22, "user".into(), PathBuf::from("/tmp/key"));
        let handle = conn.handle.try_lock().unwrap();
        assert!(handle.is_none());
    }

    #[tokio::test]
    #[ignore] // Requires a real SSH target
    async fn test_connect_nonexistent_host() {
        let conn = SshConnection::new(
            "192.0.2.1".into(),
            22,
            "user".into(),
            PathBuf::from("/tmp/nonexistent_key"),
        );
        let result = conn.connect().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exec_without_connect() {
        let conn = SshConnection::new("host".into(), 22, "user".into(), PathBuf::from("/tmp/key"));
        let result = conn.exec_command("echo hi").await;
        assert!(matches!(result, Err(SshError::NotConnected)));
    }

    #[tokio::test]
    async fn test_scp_without_connect() {
        let conn = SshConnection::new("host".into(), 22, "user".into(), PathBuf::from("/tmp/key"));
        let result = conn
            .scp_upload(&PathBuf::from("/tmp/test"), "/tmp/dest")
            .await;
        assert!(matches!(result, Err(SshError::NotConnected)));
    }

    #[tokio::test]
    async fn test_tunnel_without_connect() {
        let conn = SshConnection::new("host".into(), 22, "user".into(), PathBuf::from("/tmp/key"));
        let result = conn.create_ssh_tunnel(8080, 4111).await;
        assert!(matches!(result, Err(SshError::NotConnected)));
    }
}
