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

## P2.2 restricted ABP subset compiler

Run `python3 scripts/linux/compile_webkit_subset.py --test` and inspect the subset counts. Then execute `scripts/linux/run-filter-subset.sh` in an X11/XWayland Linux debug session and observe actual requests from the local server. Ensure exception hosts, unsupported ABP patterns, and truncated rules never receive an unearned "fully protected" status. This dev-only compiler does **not** close network privacy parity.

## P2.2 exception-overlap safety regression

Test `||example.net^` with `@@||login.example.net^$script`: the unconditional native parent block must be omitted because it cannot represent the exception. Test the opposite parent/child direction and unrelated host exceptions. An unscoped `@@/regex/` must refuse the entire conversion. This safety restriction may prevent generation on existing full lists; treat it as an explicit OPEN coverage gate, not a parity solution.

## P2.2 Rust native-source bootstrap

In a Linux debug session, `BUSHIDO_LINUX_NATIVE_SOURCE_SUBSET=peter-lowe npm run tauri -- dev` generates `<app-data>/native-webkit/rules.json` and `receipt.json`, then uses the same vendored Wry WebKit content-filter store before navigation. Verify the receipt, native compile success, and denied traffic against a host from the source list with an independent server-side observer. The bare-host parser intentionally rejects conditional rules; the external Python subset producer and fixture filter remain separate developer probes. Prove parity to the Rust engine and per-tab toggles before considering production promotion.

## Per-WebView native filter selection

Start a Linux debug session with the original-source native filter enabled. Create four distinct WebViews: adblock on/not whitelisted, adblock off, adblock on/whitelisted, and Glance. Record server-side resource requests and verify that only the first receives the native block list. Change the whitelist and adblock preference while a tab remains open: this **must not** be considered complete until the existing view updates or is recreated without a stale filter. Also confirm that a default app WebView is not filtered by the probe. The implementation currently handles initial creation only.

## Dynamic native filter update (P2.2)

With a Linux debug WebKitGTK session and explicit native rule source, load a known blocked resource and observe it at the test server. Toggle the current site's whitelist; await the Rust `set_tab_native_filter` acknowledgement before reload and verify the allowed/blocked paths change correspondingly. Force missing filter-store ID and missing content manager; the operation must return `Err` rather than `Ok`. Cover repeated toggles, two tabs with conflicting whitelists, navigation between hosts, and preference changes. The current source implementation only handles explicit whitelist toggles, and other transitions remain OPEN until wired.

## Live whitelist-state and dependency lock acceptance

With live Linux navigation, test whitelist toggles, cross-domain changes, cosmetic scriptlet suppression, and correct native filter decisions before first subresource. The new URL-event handler is post-navigation and is **not** a pre-network privacy guarantee. Check the locked Rust dependency graph for direct WebKitGTK compatibility. None of these acceptance gates is closed by source inspection alone.

## First-enable filter test

Create a fresh Linux profile with no compiled content-filter store and with adblocking disabled. Turn filtering on after first page load, verify source compilation, acknowledgement, and a blocked request never reaching the server on subsequent reload. Invalid source must fail without removing previously installed filters. Repeat rapid toggles to detect out-of-order async completion; ordering is not yet guaranteed.

## Global adblock toggles on existing WebViews

After loading persisted settings, change `adBlocker` from on to off and back with multiple live Linux WebViews and different site whitelists. Each view should acknowledge the changed native filter state before a reload; verify server-observed requests and absence of rule-source errors when the probe is deliberately disabled. Repeat rapid toggles to detect out-of-order saves. The implementation remains debug-only and does not establish Windows or original Rust adblock parity.

## P2.2 stale asynchronous completion rejection

With a real Linux debug WebKitGTK instance, induce two native filter updates for the same tab and force the first compilation callback to finish last; it must not alter the newer state. Closing a tab must invalidate pending callbacks, even when the same ID is reused. Test enable→disable and disable→enable, including a 15-second failure path. A successful disabled update must be acknowledged and trigger the existing reload. The generation-state unit tests exercise ordering logic but **do not** prove a GTK runtime race has been eliminated.

## P2.2 pre-navigation deny-and-retry

In a Linux debug build with native rule source configured, navigate from an unwhitelisted origin to a whitelisted destination and back. Instrument the WebKit navigation policy callback and local server: the initial request must be denied when the desired and applied filter modes differ, and only a successfully acknowledged native update may retry it. Force filter compilation failure: the original request must not appear server-side. Check redirect chains, repeated clicks, frames and simultaneous navigations for out-of-order retries or leaked initial requests. **No runtime acceptance yet.**

## Pending navigation supersession

Start a navigation from site A to B that requires a filter transition; before compilation finishes navigate to C. Verify B cannot be retried after the newer navigation. Repeat where C already matches the applied filter, and after closing/recreating the tab. Identical URL retries and redirect chains remain separately open.

## P2.2 same-URL and denied navigation ordering

Exercise two rapid identical URL navigations requiring a filter switch, and assert only the second sequence can resume. Attempt a denied scheme or cancelled destination between a pending filter update and its callback: the old request must not reappear. Toggle the site whitelist during compilation and ensure a stale desired mode cannot be retried. Separate network-level tests remain necessary for HTTP redirects and frames.

## Glance native filtering

Open a Glance view from a whitelisted source tab into an unwhitelisted destination; the Glance filter must follow the destination rather than the source. Repeat with global adblock disabled and an explicitly whitelisted Glance destination. Observe blocked and allowed subresource requests at the fixture server from the **first** page load. Close and recreate Glance with the same ID and ensure no prior native-filter generation is reused. Test failed creation cleanup. Glance cross-site navigation/promotion and dynamic settings remain open pending a dedicated policy callback weld.

## Glance cross-origin navigation

Create Glance on an unwhitelisted origin, follow a link to a whitelisted destination and back, and observe the original requests at the server. The navigation attempt must not pass with the old applied native filter. The second navigation must not revive a superseded first attempt. Confirm `tab-url-changed` updates the Glance URL without adding an ordinary tab history entry unexpectedly. Inspect actual HTTP redirects and frames separately; source-level callbacks are not proof of a network-wide barrier.
