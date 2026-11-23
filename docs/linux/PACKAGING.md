# Linux packaging candidates (P5.1)

Status: source configuration and local static package-check tooling only. No Bushido Linux package has been built or installed by this packet.

## Build host

Start with a clean checkout on a native amd64 Debian 12 or compatible newer Debian/Ubuntu environment (arm64 is a candidate, not accepted). System WebKitGTK 4.1 API **2.40 or later** is required by the pinned Tauri fork. GTK 3, libsoup 3, OpenSSL development headers, an active desktop session for later smoke tests, Rust and Node 22 are required. Use a security-supported distro. The oldest actually supporting all pinned crates remains to be verified.

Example Debian dependencies (resolve distro package-name differences):

```sh
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libssl-dev libxdo-dev librsvg2-dev libayatana-appindicator3-dev libsecret-tools xdg-utils ca-certificates desktop-file-utils python3 binutils file curl libxkbcommon-dev
```

The runtime Secret Service must be available for vault/sync key wrapping. Installing secret-tool is not equivalent to provisioning or unlocking the keyring.

## Build locally

```sh
scripts/linux/build-deb.sh
```

The script fails on missing prerequisites or dirty tree, uses npm ci and Cargo --locked with the existing lockfiles and vendored path patches, records versions, commit, lock hashes and SOURCE_DATE_EPOCH, and builds into a fresh ignored target directory. It accepts exactly one new .deb, checks its static contents and version, and writes the artifact digest and ELF dynamic section into a receipt. It never installs, signs, publishes, uses an upstream binary, or changes hosted CI. --ci is the local Tauri non-interactive flag, not hosted CI.

This is a lockfile-controlled candidate process, not a proof of reproducible bytes. The Rust/Node/system dependency versions and package repository snapshot must be pinned for that gate; compare two builds from isolated directories on the same pinned host, then a second independent host. Tauri bundling can introduce nondeterminism.

## Artifact and desktop contract

- Linux automatic platform overlay selects Debian only. Windows NSIS and upstream updater config are in the Windows path; the Linux Rust builder does not register the binary updater and React checks the compiled platform capability before making a request.
- Native runtime dependencies include WebKitGTK and GTK (Tauri defaults), explicitly versioned WebKitGTK minimum, libsecret-tools, xdg-utils, CA trust and OpenSSL 3. Verify real DT_NEEDED dependencies and distribution package resolution; this is not full closure yet.
- Desktop file launches the binary without URI/file arguments and has browser categories, icon and no terminal. No default-browser claim, MIME or scheme registration until a safe external URL entry point, single-instance routing and workspace selection are implemented.
- GPL license included in the package. Before public distribution produce the corresponding-source offer/archive for this exact commit including local forks, lockfiles, build scripts and license notices; audit transitive licenses and filter-list redistribution. Preserve upstream attribution.
- AppImage/RPM/Nix packaging remain optional follow-up targets, not generated here. Do not mask security differences with a packaging success claim.

Run the static checker independently:

```sh
python3 scripts/linux/check-package.py --self-test
python3 scripts/linux/check-package.py /path/to/candidate.deb
```

The self-test builds tiny synthetic Debian archives using /bin/true only as an ELF fixture. It never executes them. The checker verifies a bounded desktop entry, package name, architecture header, declared dependencies and launcher/icon/license presence. It is not a malicious archive scanner, icon decoding, installability, complete dependency closure, binary authenticity or native application smoke test.

## Native acceptance sequence

Use a disposable clean Linux VM or test profile. Inspect artifact digest, control metadata, full package contents and dependency resolution before installation. Install with the distribution package manager; do not install as an upgrade over real user data. Verify launcher, initial window, remote navigation, workspace cookies, vault/keyring behavior, native download persistence, permissions, X11 and Wayland behavior. Confirm no Linux upstream binary-updater request. Uninstall and check desktop entries/binary removed while user data remain intact. Verify no default-handler or MIME change. Repeat the Windows NSIS build and updater regression separately. Only a reviewed source archive and successful native acceptance may advance the release gate.

References: Tauri Debian distribution documentation https://v2.tauri.app/distribute/debian/; pinned CLI 2.10.0 build.rs and tauri-utils 2.8.2 DebConfig. The local branch retains original upstream source provenance.

## Local execution receipt

Debian 13.3 container, Node 22.16.0: static synthetic archive checks 9/9 PASS; build script `bash -n` PASS; invocation stopped on missing `rustc` (exit 2, as designed). Cargo and the GTK/WebKitGTK/libsoup development pkg-config packages are also absent. Neither actual Bushido compilation nor a .deb was produced. Exact tested checker blob `97694456e37fc024a9b686144a080767efd3b3c6`, build script blob `0f2e8fb2986c6e56e003851bb73dab3a47cd9d2f`.

### External activation prerequisite (partial source)
Linux has an early private instance lock and bounded same-user socket queue. Explicit CLI URLs are limited to HTTP(S) without embedded credentials; main browser chrome asks for the target current workspace before opening. This is not yet native-accepted; desktop launcher still advertises no URI arguments or MIME registration until cold/warm activation, focus, user consent and singleton lifecycle are verified. Pending queue is volatile, and paths exceeding Unix socket limits fail closed. See roadmap and acceptance.

Local independent Linux process/socket reference checks: 11/11 PASS. This is not application integration acceptance. Explicit manual cold/warm CLI smoke after compiling the package: run `bushido-browser https://example.test/`, then from another terminal `bushido-browser https://example.org/`; observe one process owning the profile, named-workspace consent, and no second initialization. Also test with no URL, decline the prompt, switch workspaces before confirmation, and send an invalid scheme. Desktop MIME association remains intentionally absent.

### Source distribution candidate
See [SOURCE_DISTRIBUTION.md](SOURCE_DISTRIBUTION.md) for the exact-commit candidate generator, dependency/notice inventory, receipt and outstanding review. It does not authorize redistribution or substitute for the build-specific corresponding-source package.


## Arch/CachyOS first — local native makepkg candidate (P5.2)

Arch/CachyOS is the first native packaging target. Debian remains an independent cross-distro package contract; never install a Debian archive on Arch. This Arch source candidate is not an AUR submission or a release.

From a **clean, committed** Linux checkout on a non-root Arch host with `base-devel`, `pacman`, `libarchive`, `nodejs`, `npm`, `rust`, `cargo`, `webkit2gtk-4.1` (2.40+), GTK3, libsoup3, OpenSSL and the listed `PKGBUILD` dependencies:

```sh
scripts/linux/build-arch.sh
```

The wrapper creates a private temporary build directory, `git archive` of exact HEAD, and a local SHA256-pinned copy of `packaging/arch/PKGBUILD`. It invokes `makepkg --noconfirm --cleanbuild` **without** `--syncdeps`, `--install`, sudo, AUR submission or publication. Build uses `npm ci`, locked Cargo release, and source-tree local forks; `check()` runs Cargo and frontend tests. The source checksum placeholder in the committed template is intentionally **not** a runnable bypass: only the wrapper resolves it against the actual staged source tarball. A Git commit alone does not make a signed/reproducible source distribution. This candidate depends on configured npm/Cargo sources unless caches are present.

Static validation checks source checksum, safe archive layout, package identity, launcher/icon/license, desktop policy, package file list. Output includes package and local build receipt in a newly created private directory, printed by the wrapper. The installed path is `/usr/bin/bushido-browser`; no implicit default-browser registration, URL/MIME handlers or auto-updater. The original desktop template icon placeholder is replaced only in the package copy. No post-install hooks.

Manual acceptance after reviewing the artifact: first inspect `pacman -Qip /path/to/*.pkg.tar.zst` and `pacman -Qlp ...`; use a **disposable VM/profile** for `sudo pacman -U`, verify Wayland and X11 launches, storage/keyring, browser tab behavior and permissions, then `sudo pacman -R bushido-browser` and verify neither user data nor default-browser associations changed. Live package installation/removal, library closure review, source/legal notices, reproducibility and production security remain OPEN. AUR submission only after those gates.

Sources: ArchWiki PKGBUILD and official makepkg/pacman documentation. No proprietary downloaded runtime is used.
