# Linux package candidate (P5.1)

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
