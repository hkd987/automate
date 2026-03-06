use std::path::PathBuf;

use automate_desktop_lib::ssh::SshConnection;

/// Path to the test VM's private key.
/// When the Docker test-vm container is built, the key is generated at this path
/// inside the container. We copy it out before running tests via:
///   docker cp <container>:/home/testuser/.ssh/id_ed25519 /tmp/test_vm_key
fn test_key_path() -> PathBuf {
    PathBuf::from("/tmp/test_vm_key")
}

fn make_test_connection() -> SshConnection {
    SshConnection::new(
        "127.0.0.1".to_string(),
        2222,
        "testuser".to_string(),
        test_key_path(),
    )
}

#[tokio::test]
#[ignore]
async fn test_ssh_connect_to_docker_vm() {
    let conn = make_test_connection();
    let result = conn.connect().await;
    assert!(result.is_ok(), "Failed to connect: {:?}", result.err());
}

#[tokio::test]
#[ignore]
async fn test_ssh_exec_command() {
    let conn = make_test_connection();
    conn.connect().await.expect("failed to connect");

    let output = conn
        .exec_command("echo hello")
        .await
        .expect("failed to exec");
    assert_eq!(output, "hello");
}

#[tokio::test]
#[ignore]
async fn test_ssh_detect_arch() {
    let conn = make_test_connection();
    conn.connect().await.expect("failed to connect");

    let arch = conn.detect_arch().await.expect("failed to detect arch");
    // Should be a valid architecture string (e.g. x86_64, aarch64)
    assert!(!arch.is_empty(), "arch should not be empty");
    assert!(
        arch == "x86_64" || arch == "aarch64" || arch == "armv7l",
        "unexpected arch: {}",
        arch
    );
}

#[tokio::test]
#[ignore]
async fn test_ssh_upload_file() {
    let conn = make_test_connection();
    conn.connect().await.expect("failed to connect");

    // Create a temp file locally
    let tmp_dir = std::env::temp_dir();
    let local_path = tmp_dir.join("automate_e2e_upload_test.txt");
    std::fs::write(&local_path, "e2e upload content").expect("failed to write local file");

    // Upload to remote
    let remote_path = "/tmp/e2e_upload_test.txt";
    conn.scp_upload(&local_path, remote_path)
        .await
        .expect("failed to upload");

    // Verify the file exists and has correct content
    let output = conn
        .exec_command(&format!("cat {}", remote_path))
        .await
        .expect("failed to read remote file");
    assert_eq!(output, "e2e upload content");

    // Cleanup
    let _ = std::fs::remove_file(&local_path);
    let _ = conn.exec_command(&format!("rm {}", remote_path)).await;
}
