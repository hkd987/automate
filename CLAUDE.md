# Automate

Tauri 2.x desktop app + Rust daemon for LLM automation on SSH VMs.

## Workspace

3 crates — `automate-shared` (types), `automate-daemon` (VM binary), `automate-desktop` (Tauri app), plus `frontend/` (React/TS/Tailwind).

## Build

- `cargo build --workspace` — build all Rust crates
- `cargo tauri dev` — desktop + frontend dev server (run from `crates/automate-desktop/`)
- `cd frontend && npm run dev` — frontend only

## Test

- `cargo test --workspace` — all Rust tests
- Unit tests in `#[cfg(test)]` modules
- Integration tests in `crates/*/tests/`
- E2E fixtures in `crates/*/tests/fixtures/`

## Lint

- `cargo fmt --all --check` (always run first)
- `cargo clippy --workspace`
- `cd frontend && npm run lint`

## Rust Conventions

- `thiserror` for library error types, `anyhow` for binary error handling
- Shared types always in `automate-shared`
- serde derive on all API/config types
- All async code uses tokio

## Frontend Conventions

- Tailwind CSS (no CSS modules)
- `@tauri-apps/api` invoke for IPC
- TypeScript types in `frontend/src/types/` mirror Rust shared types
- React Router for navigation

## Key Crates

- axum (daemon HTTP API)
- russh (SSH from desktop)
- rusqlite (SQLite, bundled feature)
- tokio (async runtime)
- serde + serde_yaml (config parsing)
- notify (file watching)
- tokio-cron-scheduler (cron)

## Architecture

Desktop app communicates with daemon via HTTP-over-SSH-tunnel. Daemon runs autonomously on VM via systemd. All credentials stored encrypted on VM only.
