#!/usr/bin/env bash
# Build Axinite and all bundled channels.
#
# Run this before release or when channel sources have changed.

set -euo pipefail

cd "$(dirname "$0")/.."

echo "Building bundled channels..."
./scripts/build-wasm-extensions.sh --channels

echo ""
echo "Building Axinite..."
cargo build --release

echo ""
echo "Done. Binary: target/release/axinite"
