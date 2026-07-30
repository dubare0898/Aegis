#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
bash "$ROOT/scripts/prepare-desktop-resources.sh"
cd "$ROOT/apps/desktop/src-tauri"
cargo tauri build
echo "[desktop] build artifacts under apps/desktop/src-tauri/target/release/bundle/"
