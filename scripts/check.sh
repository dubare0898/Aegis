#!/usr/bin/env bash
# Workspace reliability bar: unit tests + smoke + golden + baseline compare.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

echo "[check] cargo test --workspace"
cargo test --workspace

echo "[check] aegis_harness --suite smoke --compare-baseline"
cargo run -p aegis_harness -- --suite smoke --compare-baseline

echo "[check] aegis_harness --assert-golden"
cargo run -p aegis_harness -- --assert-golden --no-auto-engage --no-log

echo "[check] ok"
