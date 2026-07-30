#!/usr/bin/env bash
# Workspace reliability bar: unit tests + demo smoke + golden assert.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

echo "[check] cargo test --workspace"
cargo test --workspace

echo "[check] demo_harness --suite smoke"
cargo run -p demo_harness -- --suite smoke

echo "[check] demo_harness --assert-golden"
cargo run -p demo_harness -- --assert-golden

echo "[check] ok"
