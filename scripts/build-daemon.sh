#!/bin/bash
set -euo pipefail

# Cross-compile daemon for linux targets
# Usage: ./scripts/build-daemon.sh [target]
# Targets: x86_64-unknown-linux-gnu (default), aarch64-unknown-linux-gnu

TARGET="${1:-x86_64-unknown-linux-gnu}"
DIST_DIR="dist"

echo "Building automate-daemon for ${TARGET}..."

cargo build --release --target "${TARGET}" -p automate-daemon

mkdir -p "${DIST_DIR}"

# Determine arch suffix from target
case "${TARGET}" in
    x86_64-*)
        SUFFIX="x86_64"
        ;;
    aarch64-*)
        SUFFIX="aarch64"
        ;;
    *)
        SUFFIX="${TARGET}"
        ;;
esac

BINARY="target/${TARGET}/release/automate-daemon"
OUTPUT="${DIST_DIR}/automate-daemon-${SUFFIX}"

cp "${BINARY}" "${OUTPUT}"
echo "Binary copied to ${OUTPUT}"

# Generate SHA256 checksum
shasum -a 256 "${OUTPUT}" | awk '{print $1}' > "${OUTPUT}.sha256"
echo "SHA256: $(cat "${OUTPUT}.sha256")"

echo "Done."
