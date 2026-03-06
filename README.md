# Automate

A desktop app that deploys LLM-powered automations to any SSH-accessible VM.

## How It Works

- **Desktop app** (Tauri 2.x) manages VM connections, automation configs, and deployment over SSH
- **Daemon** (Rust binary) runs on your VM via systemd, handling cron jobs, webhooks, log watchers, and chat triggers autonomously
- **Agent runtimes** (Claude Code, Codex) are spawned by the daemon to execute prompts with minimum-scoped credentials
- **`.automate.yml`** checked into your repo defines automations declaratively -- no credentials, safe to commit

## Quick Start

### Prerequisites

- Rust toolchain (stable)
- Node.js 18+
- A Linux VM with SSH access (key-based auth)
- An agent runtime installed on the VM (`claude` or `codex`)

### Setup

```bash
git clone https://github.com/user/automate.git
cd automate
cargo build --workspace
cd frontend && npm install && cd ..
```

### Run

```bash
cd crates/automate-desktop
cargo tauri dev
```

### First Use

1. Add a VM in the Connection Manager (host, port, SSH key path)
2. Click "Test Connection" to verify access
3. Deploy the daemon -- the app handles binary download, systemd setup, and nginx config
4. Create an automation or point the app at a repo with `.automate.yml`

## `.automate.yml`

```yaml
automations:
  - name: daily-digest
    trigger: cron(0 9 * * *)
    auth_profile: personal-sub
    prompt: "Summarize open PRs and post to Slack #dev"

  - name: on-push
    trigger: webhook
    auth_profile: work-bedrock
    prompt: "Review the diff and open a PR with fixes. Payload: $WEBHOOK_BODY"

  - name: watch-errors
    trigger: log_pattern("ERROR")
    file: /var/log/app.log
    auth_profile: codex-sub
    prompt: "Diagnose this error and create a GitHub issue. Context: $LOG_MATCH"

  - name: run-on-demand
    trigger: manual
    auth_profile: personal-sub
    prompt: "Run the test suite and report failures"
```

**Trigger types:** `cron(...)`, `webhook`, `log_pattern(...)`, `manual`

**Interpolation variables:** `$WEBHOOK_BODY`, `$LOG_MATCH`, `$TIMESTAMP`

## Auth Profiles

Credentials live in `~/.automate/profiles.yml` on your machine (gitignored). `.automate.yml` references profiles by name only.

```yaml
# ~/.automate/profiles.yml
auth_profiles:
  personal-sub:
    runtime: claude-code
    mode: subscription
  work-bedrock:
    runtime: claude-code
    mode: bedrock
    aws_region: us-east-1
    aws_model: anthropic.claude-sonnet-4-6-20251001-v2:0
  codex-sub:
    runtime: codex
    mode: subscription
```

## Supported Runtimes

| Runtime | Auth Modes |
|---|---|
| `claude-code` | subscription, bedrock, api-key |
| `codex` | subscription, api-key |

## Channels

**Slack** -- Uses Socket Mode (outbound WebSocket, no public IP needed). Configure a Slack app with bot + app tokens, set allowed channels and user IDs. Messages from allowlisted users trigger ad-hoc jobs.

**WhatsApp** -- Uses a `whatsapp-web.js` Node.js bridge spawned by the daemon. Scan a QR code in the desktop app to link your number. Set allowlisted phone numbers. Suitable for personal/low-volume use. Requires Node.js on the VM. No Meta Business account required.

## Development

```bash
cargo build --workspace          # build all crates
cargo test --workspace           # run all tests
cargo fmt --all --check          # check formatting
cargo clippy --workspace         # lint Rust
cd frontend && npm run lint      # lint frontend
cargo tauri dev                  # full dev server (from crates/automate-desktop/)
```

Workspace has 3 crates: `automate-shared` (types), `automate-daemon` (VM binary), `automate-desktop` (Tauri app), plus `frontend/` (React/TypeScript). See `CLAUDE.md` for conventions.

## License

BSD 2-Clause. See [LICENSE](LICENSE).
