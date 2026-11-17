#!/usr/bin/env bash
set -Eeuo pipefail

# Run from any directory in a real Linux desktop or CI environment.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" != Linux ]]; then
  echo "Linux acceptance requires a Linux host" >&2
  exit 2
fi

echo "Repository: $ROOT"
git rev-parse HEAD 2>/dev/null || true
uname -a
cat /etc/os-release
rustc --version
cargo --version
node --version
npm --version
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 libsoup-3.0

cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm ci
npm run build
npm test

echo "P0 compile/tests passed (does not prove native GUI or privacy parity)."
