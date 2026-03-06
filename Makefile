.PHONY: dev build test lint fmt

dev:
	cd crates/automate-desktop && cargo tauri dev

build:
	cargo build --workspace
	cd frontend && npm run build

test:
	cargo test --workspace

lint:
	cargo fmt --all --check
	cargo clippy --workspace -- -D warnings
	cd frontend && npm run lint

fmt:
	cargo fmt --all
