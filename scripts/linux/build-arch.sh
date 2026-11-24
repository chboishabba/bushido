#!/usr/bin/env bash
# Offline source staging + local native Arch makepkg candidate.
# No sudo, install, publication, signing, AUR or CI.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
[[ "$(uname -s)" == Linux ]] || { echo 'Linux only' >&2; exit 2; }
for cmd in git python3 node npm rustc cargo makepkg sha256sum tar gzip pkg-config bsdtar pacman; do
  command -v "$cmd" >/dev/null || { echo "Missing prerequisite: $cmd" >&2; exit 2; }
done
[[ "${EUID}" -ne 0 ]] || { echo 'Never build as root' >&2; exit 2; }
[[ -z "$(git status --porcelain --untracked-files=normal)" ]] ||
  { echo 'Require a clean committed checkout' >&2; exit 2; }
pkg-config --atleast-version=2.40 webkit2gtk-4.1
pkg-config --exists gtk+-3.0 libsoup-3.0 openssl
version="$(node -p 'require("./package.json").version')"
[[ "$version" == 0.14.0 ]] || { echo 'PKGBUILD version needs update' >&2; exit 2; }
sha="$(git rev-parse HEAD)"
export TZ=UTC LC_ALL=C
export SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
out="$(mktemp -d "${TMPDIR:-/tmp}/bushido-arch-candidate.XXXXXXXX")"
chmod 700 "$out"
archive="$out/bushido-source.tar.gz"
git archive --format=tar --prefix=bushido-source/ "$sha" | gzip -n > "$archive"
digest="$(sha256sum "$archive" | awk '{print $1}')"
sed "s/__LOCAL_SOURCE_SHA256__/$digest/" packaging/arch/PKGBUILD > "$out/PKGBUILD"
bash -n "$out/PKGBUILD"
python3 scripts/linux/check-arch-package.py --source "$out/PKGBUILD" --archive "$archive"
{
  echo "source_commit=$sha"
  echo "source_date_epoch=$SOURCE_DATE_EPOCH"
  echo "archive_sha256=$digest"
  sha256sum package-lock.json src-tauri/Cargo.lock
  rustc -Vv
  cargo --version
  node --version
  npm --version
  pkg-config --modversion webkit2gtk-4.1 gtk+-3.0 libsoup-3.0 openssl
  pacman -Q webkit2gtk-4.1 gtk3 libsoup3 openssl libsecret nodejs rust cargo
} > "$out/source-build-receipt.txt"
(
  cd "$out"
  # No --syncdeps or --install; report missing packages without mutating OS.
  makepkg --noconfirm --cleanbuild
)
mapfile -d '' packages < <(find "$out" -maxdepth 1 -type f -name 'bushido-browser-*.pkg.tar.*' -print0)
[[ "${#packages[@]}" == 1 ]] || { echo 'Expected one package candidate' >&2; exit 1; }
python3 "$ROOT/scripts/linux/check-arch-package.py" --package "${packages[0]}"
sha256sum "${packages[0]}" >> "$out/source-build-receipt.txt"
[[ "$sha" == "$(git rev-parse HEAD)" && -z "$(git status --porcelain --untracked-files=normal)" ]] ||
  { echo 'Source changed while building' >&2; exit 1; }
printf 'Arch candidate: %s\nReceipt: %s\n' "${packages[0]}" "$out/source-build-receipt.txt"
