# Product Requirements Document
## Automate — LLM Automation Desktop App
**Version:** 0.1 (MVP)  
**Status:** Draft  
**Date:** March 2026

---

## 1. Executive Summary

Automate is a desktop application that connects to any SSH-accessible VM and materializes LLM-powered automations on it. Users define automations via a `.automate.yml` config file dropped in a project repo, or create ad-hoc automations directly in the app. A lightweight Rust daemon is deployed to the target VM and manages all automation lifecycles — cron jobs, webhook triggers, log watchers, and inbound chat jobs from Slack and WhatsApp — without requiring any managed infrastructure from us.

The go-to-market is bring-your-own-VM, pick-your-own-agent-runtime, pick-your-own-LLM. No lock-in. Pure software margin.

---

## 2. Problem Statement

Developers using LLM coding agents (Claude Code, Codex, Aider, Goose) are doing so manually and interactively. There is no simple, infrastructure-agnostic way to:

- Schedule an agent to run a task on a recurring basis
- Trigger an agent via a webhook from an external event
- Send an ad-hoc job to an agent running on a remote VM from Slack or WhatsApp
- Define these automations as code, checked into a repo, portable across environments

Existing tools either require cloud lock-in (Cursor Background Agents), significant DevOps overhead (GitHub Actions), or don't support agent-native primitives (Zapier, Make). Nothing owns the space of "drop a config file, get LLM automations on your own VM."

---

## 3. Target User

**Primary:** Solo developers and small teams actively using Claude Code, Codex, or similar agent runtimes who want to extend their workflows beyond interactive sessions.

**Secondary:** Technical founders and indie hackers who want an always-on AI assistant reachable from their phone via WhatsApp or Slack, running on their own infrastructure.

**Wedge persona:** Someone who has already left Claude Code running overnight on a VM and thought "I wish this just happened automatically."

---

## 4. Business Case

### Go-to-Market Sequencing

```
Phase 1 (MVP)       Phase 2                  Phase 3
Bring your own VM   +team sharing/collab      +hosted VMs (our infra)
Free / open core    Paid tiers                SaaS tier
```

Starting asset-light means zero infrastructure cost, zero ops burden, and full focus on validating the config format and UX. The hosted VM tier only makes sense once we know what users actually run.

### Viral Loop

The `.automate.yml` config file is the viral mechanism. When dropped in a public GitHub repo, other developers see it, wonder what it does, and find the tool. Same organic spread pattern as `Dockerfile` and `.github/workflows`.

### Competitive Moat

Three axes of compatibility accumulate over time: VM providers × Agent runtimes × LLM endpoints. Cursor is locked to their stack. We work with everything.

### Revenue Path

- Open core desktop app (free)
- Team features (VM sharing, shared automation library, run history sync) — paid
- Hosted VM tier (we provision and manage VMs, user just points the app) — SaaS

---

## 5. Product Overview

### Two Modes

**Standalone Mode**  
Open the app, connect a VM, define automations directly in the UI. No repo required. Lives in local app state.

**Project Mode**  
Point the app at a local repo containing a `.automate.yml`. The file is the source of truth. The app reads it, displays defined automations, and deploys them to the target VM. Team members clone the repo, open the app, point it at their VM — everything works.

Project mode is standalone mode with the config pre-populated. Same underlying engine.

### Upgrade Path

```
Standalone (ad-hoc, no repo)
    → Project mode (.automate.yml in repo)
        → Hosted tier (our VMs instead of yours)
```

Each step is additive. Users migrate naturally as they get more serious.

---

## 6. The `.automate.yml` Format

```yaml
# .automate.yml — safe to commit, no credentials
automations:
  - name: daily-digest
    trigger: cron(0 9 * * *)
    auth_profile: personal-sub
    prompt: "Summarize open PRs and post to Slack #dev"

  - name: on-push
    trigger: webhook
    auth_profile: work-bedrock
    prompt: "Review the diff and open a PR with suggested fixes. Webhook payload: $WEBHOOK_BODY"

  - name: watch-errors
    trigger: log_pattern("ERROR")
    file: /var/log/app.log
    auth_profile: codex-sub
    prompt: "Diagnose this error and create a GitHub issue. Log context: $LOG_MATCH"
```

Auth profiles are defined in `~/.automate/profiles.yml` (local only, gitignored) and referenced by name in `.automate.yml`. See Section 10 for the full profiles format.

**Supported trigger types (MVP):**
- `cron(...)` — standard cron expression
- `webhook` — HTTP POST, nginx-fronted, HMAC-validated
- `log_pattern(...)` — watches a file for a regex match
- `manual` — triggered from desktop app or chat channels

**Prompt interpolation variables:**
- `$WEBHOOK_BODY` — raw JSON body of the incoming webhook request
- `$LOG_MATCH` — the log line(s) that matched the pattern trigger
- `$TIMESTAMP` — ISO timestamp of when the job was triggered

**Auth profile field** references a named profile in `profiles.yml` (see Section 10).  
**Runtime** is derived from the auth profile — no separate runtime field needed in the config.

---

## 7. Desktop Application (Tauri)

### Tech Stack

- **Framework:** Tauri 2.x (Rust backend, React/TypeScript frontend)
- **Local DB:** SQLite via `rusqlite` (VM profiles, standalone automations, run history)
- **SSH:** `russh` crate (async, persistent connections with auto-reconnect)
- **Styling:** Tailwind CSS

### Core Screens

**1. VM Connection Manager**
- Add/edit/remove VM profiles (host, port, SSH key path, sudo capability)
- Test connection button
- Architecture detection on connect (`uname -m`) for daemon binary selection
- First-connect flow: installs daemon, checks/installs nginx

**2. Automation Dashboard**
- List of all automations (standalone + project)
- Per-automation: name, trigger type, last run time, last run status (success/fail)
- Quick actions: run now, pause, delete
- Link to run history log

**3. Config Editor / Automation Builder**
- Visual UI for building automations (writes `.automate.yml` under the hood)
- Raw YAML editor toggle for power users
- Trigger type selector (cron, webhook, log pattern, manual)
- Runtime selector (populated from what's installed on the VM)
- Auth mode selector per automation (see Section 10)
- Prompt editor

**4. Deploy View**
- Progress UI for VM first-connect setup
- Shows: daemon install, nginx check, systemd unit registration
- Per-automation deploy status

**5. Channel Setup**
- Slack: bot token + app token input, channel selector, allowlist user IDs
- WhatsApp: QR code display for scan-to-link, allowlist phone numbers

**6. Run History**
- Per-automation log viewer
- Timestamp, trigger source, status, truncated output
- Link to full log file on VM (`~/.automate/logs/`)

### Local State (Desktop App SQLite Schema — simplified)

```
vm_profiles      (id, name, host, port, key_path, arch, last_connected)
automations      (id, vm_id, name, trigger, runtime, auth_profile, prompt, enabled)
run_history      (id, automation_id, triggered_at, source, status, output_preview)
channel_configs  (id, vm_id, channel_type, config_json, allowlist_json)
```

Note: Credentials (API keys, AWS creds) are stored encrypted in `credentials.db` **on the VM** — not in the desktop app's local SQLite. The desktop app holds VM connection profiles and automation configs only. On first setup the desktop app transmits credentials to the VM over SSH and they are stored there for autonomous operation.

---

## 8. The Daemon

The daemon is a single Rust binary deployed to `~/.automate/daemon` on the target VM. It is the only moving part on the VM after setup. All automation logic runs through it.

### Responsibilities

```
~/.automate/daemon
├── scheduler          cron trigger management (tokio-cron-scheduler)
├── webhook server     local HTTP listener, nginx-fronted for public access
├── log watcher        file pattern triggers (notify crate)
├── slack listener     socket mode WebSocket connection (no nginx needed)
├── whatsapp client    whatsapp-rust crate, QR auth, persistent session
├── job queue          all triggers feed here (tokio mpsc channel)
├── agent runner       spawns agent runtime processes, captures output
└── reply router       sends output back to origin channel
```

### First-Connect Install Sequence (orchestrated by desktop app over SSH)

```
1. SSH in, detect arch (uname -m)
2. Download correct daemon binary from GitHub Releases
   x86_64-unknown-linux-gnu  — covers ~90% of VMs
   aarch64-unknown-linux-gnu — covers ARM (Graviton, Oracle free tier)
3. chmod +x, move to ~/.automate/daemon
4. Write systemd unit file (see below)
5. systemctl enable --now automate-daemon
6. Check nginx installed → if not, prompt user → apt install nginx
7. If user has webhook triggers → offer Let's Encrypt setup:
   a. Validate VM has a publicly resolvable domain name
   b. apt install certbot python3-certbot-nginx
   c. certbot --nginx -d {domain} --non-interactive --agree-tos -m {email}
   d. Verify systemd certbot renewal timer is active
8. Report "VM ready" back to desktop app
```

### Systemd Unit

```ini
[Unit]
Description=Automate Daemon
After=network.target

[Service]
ExecStart=/home/%u/.automate/daemon
Restart=always
RestartSec=5
Environment=AUTOMATE_HOME=/home/%u/.automate

[Install]
WantedBy=multi-user.target
```

`Restart=always` ensures the daemon survives agent runtime crashes. Agent processes are children of the daemon, not of systemd.

### Nginx Integration (Webhook Triggers)

When a webhook automation is deployed, the daemon:

1. Generates an nginx server block pointing to its local listener port
2. Writes to `/etc/nginx/conf.d/automate-{name}.conf`
3. Calls `nginx -s reload`
4. Reports public URL back to desktop app (`https://{vm-hostname}/hooks/{name}`)

HMAC secret is auto-generated per webhook and stored in `~/.automate/config.db`.

Slack does **not** use nginx — it opens an outbound WebSocket via socket mode.

### Desktop App ↔ Daemon IPC

After initial deploy, the desktop app communicates with the running daemon via a local HTTP API on `localhost:{port}` tunneled over SSH. HTTP-over-SSH is preferred over a Unix socket — it is easier to set up consistently across platforms, debuggable with curl during development, and works the same regardless of how the SSH tunnel is established.

This allows:
- Fetching run history
- Pushing new automation configs
- Triggering manual runs
- Streaming log output

### Daemon Auto-Update

The daemon self-updates using the `self_update` crate. On startup and every 24 hours, it checks the GitHub Releases API for a newer version:

```
1. Query GitHub Releases API for latest tag
2. Compare against current version (embedded at compile time)
3. If newer:
   a. Download new binary to ~/.automate/daemon.new
   b. Verify SHA256 checksum against release manifest
   c. Replace ~/.automate/daemon atomically
   d. Run: systemctl restart automate-daemon
4. Log update result to ~/.automate/logs/updates.log
```

The desktop app displays the current daemon version per VM and surfaces a "Update available" badge when a newer release exists. Users can also trigger a manual update check from the VM Connection Manager screen.

---

## 9. Agent Runtime Adapter

Runtimes are pluggable. The daemon defines an internal interface:

```rust
trait AgentRuntime {
    fn name(&self) -> &str;
    fn is_installed(&self, vm: &VM) -> bool;
    fn install(&self, vm: &VM) -> Result<()>;
    fn run(&self, job: &Job) -> Result<Output>;
}
```

**MVP runtimes:**
- `claude-code` (Anthropic Claude Code CLI)
- `codex` (OpenAI Codex CLI)

**Planned:**
- `aider`
- `goose`
- Custom (user-defined binary path)

New runtimes are added as impl blocks. The desktop app detects what's installed on the VM and surfaces only those options.

**Agent filesystem access:** Agents run with the same permissions as the VM user — full access to the home directory and anything else the user owns. There is no additional sandboxing of the agent's filesystem scope. This is consistent with how Claude Code and Codex operate interactively. Users should treat the VM as a dedicated automation environment and not store sensitive personal data on it outside of `~/.automate/` (which agents do not receive access to directly).

---

## 10. Runtime Auth Configuration

Auth and LLM endpoint are two distinct concerns. **Auth** controls how the agent CLI is credentialed and licensed. **LLM endpoint** is implicit in the auth mode — subscription auth uses the account's default model, Bedrock uses the configured AWS model, API key auth passes the key directly to the runtime.

### Auth Modes

**Claude Code — Subscription**
- User is logged into Claude Code via their Anthropic subscription
- Daemon runs `claude login` on first setup → OAuth flow result tunneled back to desktop app for browser completion
- Session token persisted on VM at `~/.claude/`
- No API key needed; billed through Anthropic subscription

**Claude Code — AWS Bedrock**
- No Anthropic subscription required
- The desktop app writes Bedrock configuration directly into `~/.claude/settings.json` on the VM over SSH during setup — this is how Claude Code natively reads Bedrock config
- `settings.json` is written once at setup time and persists on the VM; the daemon does not need to inject env vars at runtime
- AWS credentials (key ID, secret, region, model) are stored in `credentials.db` on the VM and written to `settings.json` during the setup flow
- Billed through AWS account

Example `~/.claude/settings.json` written by the desktop app:
```json
{
  "env": {
    "ANTHROPIC_API_KEY": "",
    "AWS_ACCESS_KEY_ID": "AKIA...",
    "AWS_SECRET_ACCESS_KEY": "...",
    "AWS_DEFAULT_REGION": "us-east-1"
  },
  "model": "anthropic.claude-sonnet-4-6-20251001-v2:0"
}
```

**Claude Code — API Key**
- Daemon injects `ANTHROPIC_API_KEY` into the agent process environment at job execution time
- Billed per-token via Anthropic API

**Codex — Subscription**
- User is logged into Codex via their OpenAI subscription
- Daemon runs `codex login` on first setup → OAuth flow tunneled back to desktop app
- Session token persisted on VM at `~/.codex/`

**Codex — API Key**
- Daemon injects `OPENAI_API_KEY` into the agent process environment at job execution time

### Auth Mode Decision Matrix

| Runtime | Auth Mode | What the daemon does |
|---|---|---|
| claude-code | subscription | `claude login` OAuth on setup, session reused per run |
| claude-code | bedrock | Write AWS creds + model to `~/.claude/settings.json` at setup time |
| claude-code | api-key | Inject `ANTHROPIC_API_KEY` at run time |
| codex | subscription | `codex login` OAuth on setup, session reused per run |
| codex | api-key | Inject `OPENAI_API_KEY` at run time |

### Desktop App — Auth Setup Screen

Per VM profile, users configure one or more auth profiles:

```
Auth Profile Name:    "work-bedrock"
Runtime:              claude-code
Mode:                 bedrock
AWS Access Key ID:    [••••••••••]
AWS Secret Key:       [••••••••••]
AWS Region:           us-east-1
Model:                anthropic.claude-sonnet-4-6-20251001-v2:0

Auth Profile Name:    "personal-sub"
Runtime:              claude-code
Mode:                 subscription
Status:               ✓ Logged in (expires never)
```

Automations reference an auth profile by name. Multiple automations can share the same auth profile. Credentials are stored encrypted on the VM and written to the appropriate config location (e.g. `~/.claude/settings.json` for Bedrock) during the setup flow.

### `.automate.yml` Auth Profile Reference

```yaml
# ~/.automate/profiles.yml (local only, gitignored)
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

# .automate.yml (in repo, safe to commit — no credentials)
automations:
  - name: daily-digest
    trigger: cron(0 9 * * *)
    auth_profile: personal-sub
    prompt: "Summarize open PRs and post to Slack #dev"
```

Credentials live in `profiles.yml` locally. The repo-committed `.automate.yml` only references profile names — safe to commit, no secrets exposed.

---

## 11. Inbound Chat Channels

### Slack

- Uses Socket Mode — no public webhook or nginx required
- Daemon opens outbound WebSocket to Slack using bot token + app token
- User configures: bot token, app token, allowed channel, allowlisted Slack user IDs
- Any message in the configured channel from an allowlisted user triggers an ad-hoc job

**Setup flow:**
1. User creates Slack app in their workspace (link to docs provided in desktop app)
2. Pastes bot token + app token into desktop app
3. Desktop app pushes config to daemon → daemon opens socket connection
4. Confirmation message posted to Slack channel: "Automate connected ✓"

### WhatsApp

- Uses `whatsapp-rust` crate (pure Rust, MIT license, Tokio-native)
- QR code or pair-code authentication (no Meta business account required)
- Session persisted to `~/.automate/whatsapp.db` (SQLite via whatsapp-rust)
- Auto-reconnect built into the crate

**Setup flow:**
1. User clicks "Connect WhatsApp" in desktop app
2. Daemon generates QR code → tunneled back to desktop app → displayed in UI
3. User scans QR with phone → session established
4. Desktop app shows "WhatsApp linked ✓" + linked number
5. User sets allowlisted phone numbers in desktop app

**Important:** whatsapp-rust implements the WhatsApp Web multi-device protocol. This is not the official WhatsApp Business API. It is suitable for personal/low-volume use. Users should be aware of WhatsApp's ToS and use a dedicated number if running higher volume automations.

### Job Handling (Both Channels)

Every inbound message from an allowlisted sender becomes a job:

```rust
struct AdhocJob {
    message: String,
    source_channel: Channel,       // Slack | WhatsApp
    source_user: String,
    injected_context: Vec<Context>,
    created_at: DateTime<Utc>,
}
```

**Context injection** (automatic, before handing to agent):
- Current project directory structure (if project mode)
- Recent automation run history (last 5 runs)
- Contents of `.automate.yml` if present
- Last 50 lines of relevant log file if message mentions logs

Output is routed back to the originating channel. Cron/webhook outputs go to a configured default channel (set in desktop app).

---

## 12. MVP Scope

### In Scope

- VM connection manager (SSH key auth, sudo support)
- Daemon install/update flow (GitHub Releases binary distribution)
- Three trigger types: cron, webhook, manual
- One file trigger: log pattern watcher
- Two agent runtimes: claude-code, codex
- Auth modes: claude-code subscription, claude-code Bedrock, claude-code API key, codex subscription, codex API key
- `.automate.yml` reader + visual config builder
- Deploy automations to VM
- Basic run history (last run, status, output preview)
- Slack inbound channel (socket mode)
- WhatsApp inbound channel (whatsapp-rust, QR auth)
- Allowlisting for both channels
- Nginx integration for webhook triggers
- Local SQLite state (VM profiles, automations, history, channel configs)
- macOS + Linux desktop builds; Windows stretch goal

### Explicitly Out of Scope for MVP

- Real-time log streaming UI (v2)
- Multi-turn conversation history across chat messages (v2)
- Team/multi-user features (v2)
- Hosted VM tier (v3)
- Mobile app (future)
- Windows build (stretch)

---

## 13. Technical Architecture Summary

```
┌─────────────────────────────────┐
│     Desktop App (Tauri)         │
│  React/TS UI + Rust backend     │
│  Local SQLite (rusqlite)        │
│  SSH client (russh)             │
└────────────┬────────────────────┘
             │ SSH tunnel
             ▼
┌─────────────────────────────────┐
│     Target VM (any SSH VM)      │
│                                 │
│  ~/.automate/daemon (Rust)      │
│  ├── scheduler (cron)           │
│  ├── webhook server             │
│  ├── log watcher (notify)       │
│  ├── slack (socket mode)        │
│  ├── whatsapp (whatsapp-rust)   │
│  ├── job queue (tokio mpsc)     │
│  ├── agent runner               │
│  └── reply router               │
│                                 │
│  nginx (/etc/nginx/conf.d/)     │
│  systemd (automate-daemon)      │
│  agent runtimes (claude, codex) │
└─────────────────────────────────┘
         ▲              ▲
    webhooks        Slack/WA
    (nginx)         messages
```

---

## 14. Rust Crate Reference

| Concern | Crate |
|---|---|
| SSH client | `russh` |
| File watching | `notify` |
| Cron scheduling | `tokio-cron-scheduler` |
| WhatsApp client | `whatsapp-rust` (v0.2.0, MIT) |
| Async runtime | `tokio` |
| Local DB | `rusqlite` |
| HTTP server (daemon API) | `axum` |
| Config parsing | `serde` + `serde_yaml` |
| CLI arg parsing | `clap` |
| Daemon self-update | `self_update` |
| Desktop framework | Tauri 2.x |

---

## 15. Key Design Decisions & Rationale

**Why Tauri over Electron?** ~5MB binary vs ~200MB. No Node.js runtime dependency. Rust backend gives us the same language as the daemon — shared types, shared SSH logic.

**Why a daemon instead of SSH-per-run?** Automations need to run when the desktop app is closed. The daemon is the persistence layer. The desktop app is a config/monitoring interface, not a runtime dependency.

**Why whatsapp-rust over Baileys?** Pure Rust — no Node.js on the VM, no separate runtime to manage. Tokio-native, persistent sessions, 356 stars, actively maintained, MIT licensed. Same protocol, better fit for our stack.

**Why Slack socket mode?** No nginx required for Slack. Outbound WebSocket means it works on VMs with no public IP and no firewall changes. Zero infrastructure overhead.

**Why separate auth profiles from `.automate.yml`?** Security and portability. The config file is safe to commit to a repo — it only references profile names, never credentials. Auth profiles live in a local-only `profiles.yml` that is gitignored. This mirrors how `.env` and `.env.example` work in most projects.

**Why encrypted credentials at rest on the VM rather than injected from the desktop app?** The daemon must operate autonomously — cron jobs fire at 3am, webhooks arrive when the desktop app is closed, WhatsApp messages come in at any time. Credentials must be available on the VM without the user present. The correct constraint is not "no credentials on the VM" but "no credentials stored insecurely on the VM." Credentials are encrypted at rest, protected by filesystem permissions, and injected into agent processes with minimum required scope per job.

**Why `.automate.yml` as the config format?** Portability and virality. The file travels with the repo. Other developers see it, want to use it. Mirrors the spread of Dockerfile and GitHub Actions workflows.

---

## 16. Security Model

### Threat Model

| Threat | Status | Mitigation |
|---|---|---|
| Credentials exposed over network | ✓ Mitigated | Credentials never transmitted after initial setup |
| Credentials in logs or process list | ✓ Mitigated | Daemon explicitly excludes creds from logs; not passed as CLI args |
| Credentials readable by other users on VM | ✓ Mitigated | `chmod 600` + user-owned files only |
| Webhook endpoint abused externally | ✓ Mitigated | HMAC validation + nginx rate limiting on all webhook endpoints |
| Unauthorized Slack/WhatsApp job submission | ✓ Mitigated | Strict allowlist enforcement — non-allowlisted senders silently ignored |
| Unauthenticated daemon access | ✓ Mitigated | Daemon API only accessible via SSH tunnel — never exposed publicly |
| Root on VM reads credentials | ✗ Not mitigated | Inherent to the Linux model — same as Claude Code, SSH keys, etc. |
| VM compromised externally | ✗ Not mitigated | VM hardening is the user's responsibility |
| Prompt injection via untrusted content | ⚠ Partial | Agent scope limiting reduces blast radius; fully unfixable at the LLM layer |

### What OpenClaw Got Wrong (And What We Do Differently)

OpenClaw's security crisis stemmed from three things we structurally avoid:

**1. Unauthenticated HTTP gateway on the user's machine.** OpenClaw ran a local HTTP server that trusted all localhost connections by default. A misconfigured reverse proxy or a malicious webpage could reach it without any auth. We have no such gateway. The daemon API is only reachable over an SSH tunnel — it is never publicly exposed.

**2. Public skills marketplace (ClawHub).** Over 800 of 10,700 ClawHub skills were confirmed malicious. We have no skills marketplace. Users bring their own agent runtimes. This entire attack surface does not exist.

**3. Running on the user's personal machine.** OpenClaw ran locally with access to the user's browser sessions, SSH keys, email, and corporate credentials. Our daemon runs on a dedicated VM the user controls. Even a worst-case prompt injection scenario is scoped to that VM — not the user's laptop.

### Credential Storage on the VM

The daemon runs autonomously — crons fire, webhooks arrive, and WhatsApp messages come in regardless of whether the desktop app is open. Credentials must live on the VM. The security goal is **encrypted at rest with minimum required scope**, not "no credentials on the VM."

**Storage layout:**

```
~/.automate/                         # chmod 700
├── .env                             # chmod 600 — decryption key + master creds
├── credentials.db                   # chmod 600 — encrypted SQLite
├── whatsapp.db                      # chmod 600 — whatsapp-rust session
└── logs/                            # chmod 700 — run history, never contains creds
```

**Systemd unit — credential injection:**

```ini
[Service]
ExecStart=/home/%u/.automate/daemon
EnvironmentFile=/home/%u/.automate/.env   # chmod 600, owned by user
```

The `.env` file holds the encryption key used to unlock `credentials.db` at daemon startup. It is `chmod 600`, owned by the user, and never transmitted after initial setup from the desktop app.

**Per-job credential injection:**

The daemon injects the minimum required credentials per job into the agent process environment — not a full credential dump. Claude Code only receives `ANTHROPIC_API_KEY` or AWS credentials. It never receives the daemon's encryption key, the WhatsApp session token, or any other credential it doesn't need for that specific job.

```rust
fn build_agent_env(job: &Job, creds: &Credentials) -> HashMap<String, String> {
    let mut env = HashMap::new();
    match job.auth_mode {
        AuthMode::AnthropicApiKey => {
            env.insert("ANTHROPIC_API_KEY", creds.anthropic_api_key.expose());
        }
        AuthMode::Bedrock => {
            env.insert("AWS_ACCESS_KEY_ID", creds.aws_key_id.expose());
            env.insert("AWS_SECRET_ACCESS_KEY", creds.aws_secret.expose());
            env.insert("AWS_REGION", creds.aws_region.clone());
        }
        // etc.
    }
    env  // daemon encryption key, WhatsApp session, etc. never included
}
```

### Webhook Security

All webhook endpoints are nginx-fronted with:
- HMAC-SHA256 signature validation (secret auto-generated per webhook at deploy time)
- Rate limiting (`limit_req_zone` in nginx config — 10 req/min per IP default)
- HTTPS only (user's VM TLS setup, or Let's Encrypt via certbot on first-connect if desired)

Unsigned requests are rejected with 401 before reaching the daemon.

### Allowlist Enforcement

Slack and WhatsApp allowlists are enforced at the daemon level — not just the UI. Messages from non-allowlisted senders are silently dropped and logged locally. The daemon never passes non-allowlisted input to an agent runtime under any circumstances, including edge cases like sender spoofing.

### Prompt Injection — Honest Assessment

Prompt injection is an industry-wide unsolved problem. Any LLM agent that processes external content (webhook payloads, log files, Slack messages) can potentially be manipulated by maliciously crafted input. We cannot fully prevent this. What we can do:

- **Scope the blast radius** — agent runs on a dedicated VM, not the user's personal machine
- **Minimum credential injection** — a compromised agent session can only access what it was given for that job
- **No auto-install of external code** — agents can write and run code on the VM, but there is no equivalent of OpenClaw's ClawHub auto-install mechanism
- **Be transparent** — document this risk clearly so users make informed decisions about what content their automations process

The docs will include an explicit section on prompt injection risk. Users running automations that ingest untrusted external content (public webhooks, unfiltered log streams) should understand this trade-off.

---

## 17. Open Questions for V1

1. **Nginx assumption** — handle the case where nginx is not installable (rootless VMs, containers). Fallback options: caddy, or daemon handles TLS directly via `rustls`.
2. **Allowlist UX** — how does a user add themselves to the WhatsApp allowlist before they've sent a message? Pre-populate from the QR setup flow using the linked number.
3. **Credential encryption key UX** — how does the user set/reset the passphrase used to derive the encryption key for `credentials.db`? What happens if they lose it? Likely: passphrase set at VM setup time, stored in the desktop app's local keychain. Reset requires re-running the setup flow.
4. **Let's Encrypt auto-setup** — the desktop app will offer optional certbot setup during first-connect for any VM that will use webhook triggers. If the user accepts, the setup flow runs `certbot --nginx` and configures auto-renewal via a systemd timer. If declined, TLS is left to the user. Requires the VM to have a publicly resolvable domain name — the setup flow validates this before attempting cert issuance.
5. **App name** — TBD.

---

*This document reflects the product design discussion as of March 2026. It should be treated as a living document — update it as architecture decisions are finalized.*