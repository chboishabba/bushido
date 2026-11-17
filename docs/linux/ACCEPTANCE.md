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

## Shared policy versus enforcement (P2.1/P2.2)

The pure `blocker::evaluate_request` consumer should produce the same block/rewrite/filter decision for document and subresource requests on both backends. This is **not** equivalent to actual Linux interception. Acceptance for P2.2 requires proof that WebKitGTK never transmits a blocked resource to the local test server and that it honors redirects in subframes/workers.

## Native WebKitGTK permission boundary (P3.3 partial)

Run separate test origins using local fixtures that request geolocation, notifications, camera/microphone, device information, pointer lock and clipboard. Verify each request receives denial *without* a prompt or allowed capability, and that nested frames, popup views, and separate tabs cannot bypass the boundary. Test other WebKit permission subclasses (including website-data-access) and document any intentional functionality lost. Treat source inspection as a preflight check, **not** runtime evidence. A future origin-scoped permission UI must replace the unconditional deny only after these negative controls pass.

## Linux per-workspace WebKitGTK data directories (P3.1 partial)

The Wry `profile_name` extension is ignored on Linux. `create_tab` and `open_glance` now use `WebviewBuilder::data_directory` with the same deterministic workspace key. Source-level tests cover distinct/path-safe directory mapping. Acceptance requires two concurrent workspaces, independently set cookies/localStorage/service-worker/cache entries, same-workspace Glance access, persistence after restart and complete selective clear of one workspace without erasing another. Inspect both underlying WebKit data locations and browser-facing APIs. No P3.1 acceptance without native run receipts.

## P3.1 selective clear completion tests (not yet executed)

1. Start Linux on X11 or XWayland with two distinct workspace profiles; write cookies/localStorage/cache entries in both, and create two tabs in workspace A.
2. Invoke `clear_workspace_data` using A's tab ID, await the Tauri command's result; verify the callback completed (not just dispatched).
3. Refresh both A tabs: A must lose all browser data, including HTTP-only cookies, while B retains every fixture.
4. Test a missing tab ID, missing WebKitGTK context, failure callback and forced timeout: the command must return `Err` rather than `Ok`.
5. Repeat while switching tabs and during application shutdown. The 15-second GTK nested event pump is a potential reentrancy hazard and must be inspected before release.

## P3.2 Linux encrypted credentials (partial)

- Install `libsecret-tools`, start an unlocked Secret Service session, generate a sync identity, and assert `keys.dat` starts with `BSH-LINUX-V1\0` and never contains the secret key bytes.
- Save and reload multiple vault entries across application restarts. Assert decryption only with the same desktop keyring item, and verify wrong-key and ciphertext corruption rejection.
- Lock the keyring or remove `secret-tool` from PATH: saving and decrypting must error without plaintext fallback. Test service-unavailable and corrupted keyring-entry results.
- Run concurrent first-use processes and confirm they agree on one stored key, without inconsistent new envelopes.
- Confirm no key appears in `ps` arguments, logs, exported crash reports, or plaintext files. Confirm file formats reject unversioned previous Linux plaintext.
- Exercise keyring item deletion and upgrade/migration policies, ensuring previously encrypted entries never silently become unrecoverable.
- Unit tests for pure AEAD functions alone **do not close** P3.2.

## P3.2 atomic persistence / fail-closed regression

On a Linux desktop with `secret-tool`, confirm behavior for missing item, present item, locked keyring, missing D-Bus service, and invalid output. A **nonzero** lookup status must never trigger key generation. Verify fresh-install provisioning separately: if `secret-tool lookup` reports missing by exit code 1, installation must fail closed rather than silently bootstrap until a verified missing-item mechanism is implemented. Unit test atomic keys.dat replacement: its final mode must be owner-only (0600), only a complete ciphertext may be observed, and no temporary secret file may remain. Inject a write failure and confirm the prior ciphertext survives.

## P2.2 observable local HTTP fixture

Run `python3 scripts/linux/network_fixture.py test` to validate the **harness** (not the browser). Run `python3 scripts/linux/network_fixture.py serve`, then open `http://127.0.0.1:8765/` from a disposable Linux WebKitGTK profile with HTTPS-only disabled for localhost. Configure equivalent network-block rules for `/blocked.js`, `/blocked-pixel`, `/blocked-fetch`, `/blocked-frame`, `/blocked-worker.js` and `/blocked-redirect`. Inspect `http://127.0.0.1:8765/__assert`; status 200 requires the document, allowed script and allowed image all arrive **and** no prohibited request reaches the server. HTTP 409 lists either missing allowed controls or blocked requests that leaked. `/__events` dumps actual server-side requests. Local-fixture success alone does not cover HTTPS, DNS, real-world filter semantics or service workers; those have separate gates.

## P2.2 debug-native filter probe

1. On Linux install the pinned WebKitGTK build dependencies, start `python3 scripts/linux/network_fixture.py serve`.
2. In a separate shell and desktop session run `scripts/linux/run-filter-probe.sh`. This enables the developer-only WebKit content-extension JSON for every WebView before it loads a page.
3. Disable HTTPS-only for localhost testing and open `http://127.0.0.1:8765/`. Inspect `/__events` and `/__assert`.
4. Test with no filter, invalid JSON, and a rule set allowing everything. These are negative controls: missing allowed resources must not pass, invalid JSON must fail WebView creation, and disabled rules must permit prohibited requests to reach the server.
5. Verify filter installation before navigation in child tabs, Glance and popup views. Check first-load races, GTK nested-event-loop responsiveness and filter cache permissions.
6. Re-run with actual third-party hosts, redirects and worker requests; do not treat the fixture as full blocker parity or P2.2 completion.
