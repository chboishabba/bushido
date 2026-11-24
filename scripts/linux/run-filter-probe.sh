#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
if [[ "$(uname -s)" != Linux ]]; then
  echo "This native WebKitGTK probe requires Linux" >&2
  exit 2
fi
export BUSHIDO_LINUX_FILTER_JSON="$ROOT/scripts/linux/fixture-filter-webkit.json"
echo "Developer-only WebKitGTK filter: $BUSHIDO_LINUX_FILTER_JSON"
echo "Start the fixture separately: python3 scripts/linux/network_fixture.py serve"
echo "Run this command in an X11/XWayland desktop session with HTTPS-only disabled for localhost."
exec npm run tauri -- dev
