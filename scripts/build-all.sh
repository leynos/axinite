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
# A release build takes none of the development standard in .cargo/config.toml.
# An assigned RUSTFLAGS, even an inherited empty one, displaces its `rustflags`
# tables; the release profile names no codegen backend, so nothing else is
# needed.
RUSTFLAGS="${RUSTFLAGS-}" cargo build --release

echo ""
echo "Done. Binary: target/release/axinite"
