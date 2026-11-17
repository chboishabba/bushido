# Linux port acceptance and evidence contract

**Status: no Linux acceptance receipts yet.** Contributor-reported `cargo build` for upstream PR #4 does not verify the current fork's branch.

## Build receipt format

Each test report must include: repository/branch/commit SHA, distro and version, kernel, architecture, display session (X11/Wayland), Rust/Cargo/Node/npm versions, WebKitGTK/GTK/libsoup versions, exact command, exit status, and stdout/stderr (or artifact link).

### Minimum build commands

```bash
git rev-parse HEAD
rustc --version
cargo --version
node --version
npm --version
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 libsoup-3.0
cd src-tauri
cargo check --locked
cargo test --locked
cd ..
npm ci
npm run build
npx tauri dev
```

Versions and package names may vary by distro: log failures rather than masking them. `npx tauri dev` requires an active desktop session and must be evaluated separately from a headless compile job.

## Security matrix (each requires an actual negative control)

| Target | Positive case | Negative control / observed evidence |
| --- | --- | --- |
| Adblock network | Allowed resource delivered | Blocked resource never requested at local HTTP server |
| Redirects | Allowed target loads | Disallowed redirected target never fetched |
| HTTPS-only | HTTPS loads | HTTP document and subresources denied; no downgrade |
| DoH strict/maximum | DNS query reaches intended encrypted resolver | Failing resolver must not silently fall back |
| Storage isolation | Same workspace retains sessions | Separate workspace has no cookies/localStorage/cache overlap |
| Clear workspace | Workspace data removed | Other workspace data preserved; no false-success response |
| Permissions | Allowed prompt can resolve | Denied origin cannot receive capability |
| Vault/sync secrets | Encrypted and recoverable with intended mechanism | Plaintext search of at-rest files finds no secret; unavailable keyring fails closed |
| Fingerprint hardening | Script runs before page code | Cross-realm/iframe/worker checks cannot access original sources where guarantee applies |
| Windows regression | Existing WebView2 behaviour passes | Linux-specific changes do not weaken Windows path |

**Do not mark a security gate complete when the feature is merely configured, a JS shim was installed, or the UI shows a green status.**

## Build failure triage

Classify actual failures as `dependency`, `source compile`, `link`, `startup`, `browser runtime`, `security parity`, or `packaging`. Record the first reproducible error and its owning source/dependency before making changes.

## Change control

- PR #4 commit `f83d133` is retained as an ancestor and attributed to AdithyaaSivamal.
- Implementation PR in `chboishabba/bushido` remains draft until an auditable milestone is accepted.
- Upstream handoff should normally be an invitation to the original contributor to pull/cherry-pick reviewed changes, or a clearly linked dependent PR.
