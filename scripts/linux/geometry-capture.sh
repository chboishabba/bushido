#!/usr/bin/env bash
# Explicit operator capture only. No upload, no desktop-wide fallback.
set -euo pipefail
umask 077
if [[ $# != 1 ]]; then echo "Usage: bash $0 NEW_OUTPUT_DIRECTORY" >&2; exit 2; fi
command -v spectacle >/dev/null || { echo "KDE Spectacle required" >&2; exit 2; }
mkdir -m 700 -- "$1"
out=$(realpath -- "$1")
printf 'Focus the Bushido window now; capture in 5 seconds.\n' >&2
date -Ins > "$out/start.txt"
# Active window, background mode, explicit output; no clipboard or notification.
timeout 20s spectacle -b -a -n -o "$out/window.png" -d 5000
test -s "$out/window.png"
date -Ins > "$out/end.txt"
sha256sum "$out/window.png" > "$out/sha256.txt"
printf '%s\n' "$out/window.png"
