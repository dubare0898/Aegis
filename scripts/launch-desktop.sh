#!/usr/bin/env bash
# Launchable desktop entry without requiring a Tauri rebuild.
# Starts aegis_api (sim idle) + opens an app-style window when possible.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${AEGIS_PORT:-8080}"
API_LOG="${TMPDIR:-/tmp}/aegis-api-desktop.log"

export PATH="${HOME}/.cargo/bin:${HOME}/.local/share/fnm:${PATH}"
if command -v fnm >/dev/null 2>&1; then
  eval "$(fnm env --shell bash)" 2>/dev/null || true
fi

cd "$ROOT"

if [[ ! -d apps/console/dist ]] || [[ ! -f apps/console/dist/index.html ]]; then
  echo "[desktop] building console…"
  (cd apps/console && npm run build)
fi

if [[ ! -x target/release/aegis_api && ! -x target/debug/aegis_api ]]; then
  echo "[desktop] building aegis_api…"
  CARGO_TARGET_DIR="$ROOT/target" cargo build -p aegis_api
fi

API_BIN="$ROOT/target/debug/aegis_api"
[[ -x "$ROOT/target/release/aegis_api" ]] && API_BIN="$ROOT/target/release/aegis_api"

# Free port if a stale API is listening
if command -v fuser >/dev/null 2>&1; then
  fuser -k "${PORT}/tcp" 2>/dev/null || true
fi

echo "[desktop] starting API (sim idle — press Start in the UI)…"
"$API_BIN" --port "$PORT" --console-dist "$ROOT/apps/console/dist" >"$API_LOG" 2>&1 &
API_PID=$!

cleanup() {
  kill "$API_PID" 2>/dev/null || true
  wait "$API_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

for _ in $(seq 1 50); do
  if curl -sf "http://127.0.0.1:${PORT}/health" >/dev/null; then
    break
  fi
  sleep 0.1
done

URL="http://127.0.0.1:${PORT}/"
echo "[desktop] opening $URL"

if command -v google-chrome >/dev/null 2>&1; then
  google-chrome --app="$URL" --new-window >/dev/null 2>&1 &
elif command -v chromium-browser >/dev/null 2>&1; then
  chromium-browser --app="$URL" --new-window >/dev/null 2>&1 &
elif command -v chromium >/dev/null 2>&1; then
  chromium --app="$URL" --new-window >/dev/null 2>&1 &
elif command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$URL" >/dev/null 2>&1 &
else
  echo "Open $URL in your browser. Press Ctrl+C to stop the API."
fi

echo "[desktop] API pid=$API_PID — sim remains idle until you press Start"
echo "[desktop] log: $API_LOG"
wait "$API_PID"
