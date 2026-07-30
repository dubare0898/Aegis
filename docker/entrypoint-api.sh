#!/bin/sh
# Default API entrypoint: serve baked console dist when present.
# Extra CLI args after the image name are forwarded to cuas_api.
set -eu
cd /app
PORT="${PORT:-8080}"
if [ -f /app/console/index.html ]; then
  exec cuas_api --port "$PORT" --console-dist /app/console "$@"
fi
exec cuas_api --port "$PORT" "$@"
