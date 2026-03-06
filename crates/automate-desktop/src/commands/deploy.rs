use std::path::PathBuf;

use uuid::Uuid;

use crate::db;
use crate::ssh::SshConnection;
use crate::state::AppState;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeployStep {
    pub step: u8,
    pub label: String,
    pub status: StepStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Done,
    Error,
}

const SYSTEMD_UNIT: &str = r#"[Unit]
Description=Automate Daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/automate-daemon --host 127.0.0.1 --port 4111
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"#;

#[tauri::command]
pub async fn deploy_daemon(
    state: tauri::State<'_, AppState>,
    vm_id: String,
) -> Result<Vec<DeployStep>, String> {
    let uuid: Uuid = vm_id.parse().map_err(|e: uuid::Error| e.to_string())?;

    // Step 1: Get VM profile from DB
    let profile = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        db::get_vm(&conn, &uuid)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "VM not found".to_string())?
    };

    let ssh = SshConnection::new(
        profile.host.clone(),
        profile.port,
        profile.user.clone(),
        PathBuf::from(&profile.key_path),
    );

    let mut steps = vec![
        DeployStep {
            step: 1,
            label: "Connect to VM".to_string(),
            status: StepStatus::Pending,
            detail: None,
        },
        DeployStep {
            step: 2,
            label: "Detect architecture".to_string(),
            status: StepStatus::Pending,
            detail: None,
        },
        DeployStep {
            step: 3,
            label: "Upload daemon binary".to_string(),
            status: StepStatus::Pending,
            detail: None,
        },
        DeployStep {
            step: 4,
            label: "Set permissions".to_string(),
            status: StepStatus::Pending,
            detail: None,
        },
        DeployStep {
            step: 5,
            label: "Write systemd unit".to_string(),
            status: StepStatus::Pending,
            detail: None,
        },
        DeployStep {
            step: 6,
            label: "Enable and start service".to_string(),
            status: StepStatus::Pending,
            detail: None,
        },
        DeployStep {
            step: 7,
            label: "Setup WhatsApp bridge".to_string(),
            status: StepStatus::Pending,
            detail: None,
        },
        DeployStep {
            step: 8,
            label: "Verify health endpoint".to_string(),
            status: StepStatus::Pending,
            detail: None,
        },
    ];

    // Step 1: Connect
    steps[0].status = StepStatus::InProgress;
    match ssh.connect().await {
        Ok(()) => steps[0].status = StepStatus::Done,
        Err(e) => {
            steps[0].status = StepStatus::Error;
            steps[0].detail = Some(e.to_string());
            return Ok(steps);
        }
    }

    // Step 2: Detect arch
    steps[1].status = StepStatus::InProgress;
    let arch = match ssh.detect_arch().await {
        Ok(arch) => {
            steps[1].status = StepStatus::Done;
            steps[1].detail = Some(arch.clone());
            arch
        }
        Err(e) => {
            steps[1].status = StepStatus::Error;
            steps[1].detail = Some(e.to_string());
            return Ok(steps);
        }
    };

    // Step 3: SCP binary
    steps[2].status = StepStatus::InProgress;
    let binary_name = format!("automate-daemon-{}", arch);
    let local_path = PathBuf::from(&binary_name);
    match ssh
        .scp_upload(&local_path, "/usr/local/bin/automate-daemon")
        .await
    {
        Ok(()) => steps[2].status = StepStatus::Done,
        Err(e) => {
            steps[2].status = StepStatus::Error;
            steps[2].detail = Some(e.to_string());
            return Ok(steps);
        }
    }

    // Step 4: chmod +x
    steps[3].status = StepStatus::InProgress;
    match ssh
        .exec_command("chmod +x /usr/local/bin/automate-daemon")
        .await
    {
        Ok(_) => steps[3].status = StepStatus::Done,
        Err(e) => {
            steps[3].status = StepStatus::Error;
            steps[3].detail = Some(e.to_string());
            return Ok(steps);
        }
    }

    // Step 5: Write systemd unit
    steps[4].status = StepStatus::InProgress;
    let write_cmd = format!(
        "cat > /etc/systemd/system/automate-daemon.service << 'UNIT_EOF'\n{}UNIT_EOF",
        SYSTEMD_UNIT
    );
    match ssh.exec_command(&write_cmd).await {
        Ok(_) => steps[4].status = StepStatus::Done,
        Err(e) => {
            steps[4].status = StepStatus::Error;
            steps[4].detail = Some(e.to_string());
            return Ok(steps);
        }
    }

    // Step 6: Enable and start
    steps[5].status = StepStatus::InProgress;
    match ssh
        .exec_command("systemctl daemon-reload && systemctl enable automate-daemon && systemctl restart automate-daemon")
        .await
    {
        Ok(_) => steps[5].status = StepStatus::Done,
        Err(e) => {
            steps[5].status = StepStatus::Error;
            steps[5].detail = Some(e.to_string());
            return Ok(steps);
        }
    }

    // Step 7: Setup WhatsApp bridge
    steps[6].status = StepStatus::InProgress;
    match setup_whatsapp_bridge(&ssh).await {
        Ok(detail) => {
            steps[6].status = StepStatus::Done;
            steps[6].detail = Some(detail);
        }
        Err(e) => {
            // Non-fatal: WhatsApp bridge setup failure shouldn't block the deploy
            steps[6].status = StepStatus::Done;
            steps[6].detail = Some(format!("Skipped ({})", e));
        }
    }

    // Step 8: Verify health
    steps[7].status = StepStatus::InProgress;
    match ssh
        .exec_command("curl -sf http://127.0.0.1:4111/health")
        .await
    {
        Ok(output) => {
            steps[7].status = StepStatus::Done;
            steps[7].detail = Some(output);
        }
        Err(e) => {
            steps[7].status = StepStatus::Error;
            steps[7].detail = Some(e.to_string());
            return Ok(steps);
        }
    }

    Ok(steps)
}

/// Upload the WhatsApp bridge files to the VM and run npm install.
async fn setup_whatsapp_bridge(ssh: &SshConnection) -> Result<String, String> {
    // Check if node is available
    ssh.exec_command("which node")
        .await
        .map_err(|_| "Node.js not installed on VM".to_string())?;

    // Create directory
    ssh.exec_command("mkdir -p ~/.automate/whatsapp-bridge")
        .await
        .map_err(|e| e.to_string())?;

    // Write package.json
    let package_json = include_str!("../../../../whatsapp-bridge/package.json");
    let cmd = format!(
        "cat > ~/.automate/whatsapp-bridge/package.json << 'BRIDGE_EOF'\n{}BRIDGE_EOF",
        package_json
    );
    ssh.exec_command(&cmd).await.map_err(|e| e.to_string())?;

    // Write index.js
    let index_js = include_str!("../../../../whatsapp-bridge/index.js");
    let cmd = format!(
        "cat > ~/.automate/whatsapp-bridge/index.js << 'BRIDGE_EOF'\n{}BRIDGE_EOF",
        index_js
    );
    ssh.exec_command(&cmd).await.map_err(|e| e.to_string())?;

    // Run npm install
    ssh.exec_command("cd ~/.automate/whatsapp-bridge && npm install --production")
        .await
        .map_err(|e| format!("npm install failed: {}", e))?;

    Ok("WhatsApp bridge installed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_systemd_unit_content() {
        assert!(SYSTEMD_UNIT.contains("ExecStart=/usr/local/bin/automate-daemon"));
        assert!(SYSTEMD_UNIT.contains("Restart=on-failure"));
        assert!(SYSTEMD_UNIT.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn test_deploy_step_serialization() {
        let step = DeployStep {
            step: 1,
            label: "Connect to VM".to_string(),
            status: StepStatus::Done,
            detail: Some("connected".to_string()),
        };
        let json = serde_json::to_string(&step).unwrap();
        assert!(json.contains("\"status\":\"done\""));
        assert!(json.contains("\"label\":\"Connect to VM\""));
    }

    #[test]
    fn test_step_status_variants() {
        for (status, expected) in [
            (StepStatus::Pending, "\"pending\""),
            (StepStatus::InProgress, "\"in_progress\""),
            (StepStatus::Done, "\"done\""),
            (StepStatus::Error, "\"error\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
        }
    }
}
