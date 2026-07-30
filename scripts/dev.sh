#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PATH="${HOME}/.local/share/fnm:${PATH}"
if command -v fnm >/dev/null 2>&1; then
  eval "$(fnm env)"
fi

cargo run -p cuas_api -- --port 8080 &
API_PID=$!
trap 'kill $API_PID 2>/dev/null || true' EXIT

cd apps/console
npm run dev -- --host 127.0.0.1 --port 5173
