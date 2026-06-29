#!/usr/bin/env bash
set -euo pipefail

PLUGIN_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "cursortab: building Rust binary..."
cd "$PLUGIN_DIR"
cargo build --release

mkdir -p "$PLUGIN_DIR/bin"
cp target/release/cursortab "$PLUGIN_DIR/bin/cursortab"
echo "cursortab: binary ready at bin/cursortab"
