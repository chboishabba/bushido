#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
if [[ "$(uname -s)" != Linux ]]; then
  echo "This debug-only WebKitGTK subset requires Linux" >&2
  exit 2
fi
python3 scripts/linux/compile_webkit_subset.py --test
RULE_FILE="$(mktemp "${TMPDIR:-/tmp}/bushido-webkit-rules.XXXXXXXX.json")"
trap 'rm -f "$RULE_FILE"' EXIT
python3 scripts/linux/compile_webkit_subset.py --max-rules 2048 \
  --output "$RULE_FILE" \
  src-tauri/adblock/easylist.txt src-tauri/adblock/easyprivacy.txt
export BUSHIDO_LINUX_FILTER_JSON="$RULE_FILE"
echo "Debug-only ABP host subset: $RULE_FILE"
echo "Unsupported rules are omitted; this is not production privacy parity."
npm run tauri -- dev
