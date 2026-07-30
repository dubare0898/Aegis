#!/usr/bin/env bash
# Write a user applications entry with absolute paths for this checkout.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${XDG_DATA_HOME:-$HOME/.local/share}/applications/aegis.desktop"
ICON="$ROOT/apps/desktop/src-tauri/icons/128x128.png"
mkdir -p "$(dirname "$DEST")"
cat >"$DEST" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=Aegis
Comment=Simulation-first C-UAS decision-support console (operator-in-the-loop)
Exec=$ROOT/scripts/launch-desktop.sh
Path=$ROOT
Icon=$ICON
Terminal=false
Categories=Utility;Science;
StartupNotify=true
EOF
chmod +x "$ROOT/scripts/launch-desktop.sh" || true
echo "Wrote $DEST"
