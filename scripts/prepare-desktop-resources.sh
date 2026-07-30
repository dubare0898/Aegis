#!/usr/bin/env bash
# Build cuas_api + console and stage them for the Tauri desktop app.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RES="$ROOT/apps/desktop/src-tauri/resources"
DEV_MODE=0
if [[ "${1:-}" == "--dev" ]]; then
  DEV_MODE=1
fi

export PATH="${HOME}/.local/share/fnm:${PATH}"
if command -v fnm >/dev/null 2>&1; then
  eval "$(fnm env --shell bash)"
fi

mkdir -p "$RES/console" "$RES/scenarios"

echo "[desktop] building console…"
(
  cd "$ROOT/apps/console"
  npm run build
)
rm -rf "$RES/console"
mkdir -p "$RES/console"
cp -a "$ROOT/apps/console/dist/." "$RES/console/"

echo "[desktop] building cuas_api…"
# Always use the workspace target dir (ignore ambient CARGO_TARGET_DIR).
if [[ "$DEV_MODE" -eq 1 ]]; then
  (cd "$ROOT" && CARGO_TARGET_DIR="$ROOT/target" cargo build -p cuas_api)
  API_BIN="$ROOT/target/debug/cuas_api"
else
  (cd "$ROOT" && CARGO_TARGET_DIR="$ROOT/target" cargo build -p cuas_api --release)
  API_BIN="$ROOT/target/release/cuas_api"
fi

if [[ ! -x "$API_BIN" ]]; then
  echo "error: cuas_api binary not found at $API_BIN" >&2
  exit 1
fi

cp -f "$API_BIN" "$RES/cuas_api"
chmod +x "$RES/cuas_api"

echo "[desktop] copying scenarios…"
rm -rf "$RES/scenarios"
mkdir -p "$RES/scenarios"
cp -a "$ROOT/scenarios/." "$RES/scenarios/"

# Ensure splash exists for Tauri frontendDist
mkdir -p "$RES/splash"
if [[ ! -f "$RES/splash/index.html" ]]; then
  cp -f "$ROOT/apps/desktop/src-tauri/resources/splash/index.html" "$RES/splash/" 2>/dev/null || true
fi

echo "[desktop] resources ready in $RES"
