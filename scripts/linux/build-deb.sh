#!/usr/bin/env bash
# Local candidate build. No installation, publication, signing or hosted CI.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
[[ "$(uname -s)" == Linux ]] || { echo "Linux build host required" >&2; exit 2; }
for cmd in git node npm rustc cargo pkg-config python3 dpkg-deb sha256sum readelf; do
  command -v "$cmd" >/dev/null || { echo "Missing build dependency: $cmd" >&2; exit 2; }
done
[[ -z "$(git status --porcelain --untracked-files=normal)" ]] || {
  echo "Build from a clean committed checkout" >&2; exit 2;
}
pkg-config --atleast-version=2.40 webkit2gtk-4.1
pkg-config --exists gtk+-3.0 libsoup-3.0 openssl
node -e 'const major=Number(process.versions.node.split(".")[0]); if(major!==20 && major!==22 && major<24) process.exit(1)'
python3 scripts/linux/check-package.py --self-test
export TZ=UTC LC_ALL=C.UTF-8
export SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
mkdir -p src-tauri/target/linux-package
export CARGO_TARGET_DIR="$(mktemp -d "$ROOT/src-tauri/target/linux-package/candidate.XXXXXXXX")"
receipt="$CARGO_TARGET_DIR/build-receipt.txt"
{
  git rev-parse HEAD
  printf 'SOURCE_DATE_EPOCH=%s\n' "$SOURCE_DATE_EPOCH"
  cat /etc/os-release
  rustc -Vv
  cargo --version
  node --version
  npm --version
  pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 libsoup-3.0 openssl
  sha256sum package-lock.json src-tauri/Cargo.lock src-tauri/tauri.conf.json src-tauri/tauri.linux.conf.json
} > "$receipt"
npm ci
# The pinned Tauri CLI recognizes the platform overlay and forwards --locked
# to Cargo. --ci only disables local interactive bundler prompts.
npm run tauri -- build --bundles deb --ci -- --locked
[[ -z "$(git status --porcelain --untracked-files=normal)" ]] || {
  echo "Build changed tracked or unignored source; candidate rejected" >&2; exit 1;
}
mapfile -d '' packages < <(find "$CARGO_TARGET_DIR" -type f -name '*.deb' -print0)
[[ "${#packages[@]}" == 1 ]] || { echo "Expected exactly one new Debian candidate" >&2; exit 1; }
candidate="${packages[0]}"
python3 scripts/linux/check-package.py "$candidate"
expected="$(node -p 'require("./package.json").version')"
[[ "$(dpkg-deb -f "$candidate" Version)" == "$expected" ]] || { echo "Package version mismatch" >&2; exit 1; }
{
  dpkg-deb -f "$candidate"
  sha256sum "$candidate"
} >> "$receipt"
# This binary comes from our isolated local build, not a downloaded package.
binary="$CARGO_TARGET_DIR/release/bushido-browser"
[[ -f "$binary" ]] || { echo "Expected local binary missing" >&2; exit 1; }
readelf -d "$binary" >> "$receipt"
printf 'Candidate: %s\nReceipt: %s\n' "$candidate" "$receipt"
