#!/bin/bash
set -euo pipefail

# Start development environment
# Runs frontend dev server + cargo tauri dev

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "Starting development environment..."

# Install frontend dependencies if needed
if [ ! -d "${ROOT_DIR}/frontend/node_modules" ]; then
    echo "Installing frontend dependencies..."
    (cd "${ROOT_DIR}/frontend" && npm install)
fi

# Run cargo tauri dev (which starts both frontend and desktop app)
cd "${ROOT_DIR}/crates/automate-desktop"
cargo tauri dev
