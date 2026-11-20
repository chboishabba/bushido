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

node --test scripts/linux/test-pip-policy.cjs scripts/linux/test-vault-fill.cjs scripts/linux/test-vault-save.cjs
python3 scripts/linux/check-package.py --self-test
python3 scripts/linux/test-source-candidate.py
python3 scripts/linux/witness-receipt.py --self-test
python3 scripts/linux/itir-evidence-check.py --self-test
python3 scripts/linux/network_fixture.py test
python3 scripts/linux/compile_webkit_subset.py --test
npm ci
npm run build
npm test
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked

echo "P0 compile/tests passed (does not prove native GUI or privacy parity)."
