#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Starting test VM..."
docker compose -f "$PROJECT_ROOT/docker/docker-compose.test.yml" up -d --build --wait

# Copy the SSH key from the container for SSH e2e tests
CONTAINER_ID=$(docker compose -f "$PROJECT_ROOT/docker/docker-compose.test.yml" ps -q test-vm)
docker cp "$CONTAINER_ID:/home/testuser/.ssh/id_ed25519" /tmp/test_vm_key
chmod 600 /tmp/test_vm_key

echo "Running E2E tests..."
cargo test --workspace -- --ignored 2>&1; status=$?

echo "Stopping test VM..."
docker compose -f "$PROJECT_ROOT/docker/docker-compose.test.yml" down

exit $status
