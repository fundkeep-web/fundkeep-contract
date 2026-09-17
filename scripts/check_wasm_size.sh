#!/usr/bin/env bash
# Builds the optimized contract WASM artifact and verifies its size against
# a documented maximum size threshold.
#
# Usage:
#   ./scripts/check_wasm_size.sh [MAX_SIZE_BYTES]
#
# Default threshold: 65,536 bytes (64 KiB), well within Soroban limits while
# giving headroom for contract expansion.

set -euo pipefail

# Threshold in bytes (default: 64 KiB = 65536 bytes)
MAX_SIZE="${1:-65536}"
TARGET="wasm32v1-none"
PACKAGE="fundkeep-contract"
WASM_PATH="target/${TARGET}/release/fundkeep_contract.wasm"

echo "==> Building optimized contract WASM (${TARGET}, release)..."
cargo build --target "${TARGET}" --release --package "${PACKAGE}"

if [ ! -f "${WASM_PATH}" ]; then
  echo "Error: WASM artifact not found at ${WASM_PATH}" >&2
  exit 1
fi

# Determine file size in bytes
if stat -c %s "${WASM_PATH}" >/dev/null 2>&1; then
  SIZE=$(stat -c %s "${WASM_PATH}")
else
  SIZE=$(stat -f %z "${WASM_PATH}")
fi

SIZE_KB=$(awk -v s="${SIZE}" 'BEGIN { printf "%.2f", s / 1024 }')
MAX_KB=$(awk -v m="${MAX_SIZE}" 'BEGIN { printf "%.2f", m / 1024 }')

echo "=================================================="
echo "  WASM Artifact Size Report"
echo "=================================================="
echo "  Target:    ${TARGET}"
echo "  Artifact:  ${WASM_PATH}"
echo "  Size:      ${SIZE} bytes (${SIZE_KB} KiB)"
echo "  Threshold: ${MAX_SIZE} bytes (${MAX_KB} KiB)"
echo "=================================================="

if [ "${SIZE}" -gt "${MAX_SIZE}" ]; then
  echo "FAILURE: WASM size (${SIZE} bytes) exceeds limit of ${MAX_SIZE} bytes!" >&2
  exit 1
fi

echo "SUCCESS: WASM size is within acceptable threshold."
