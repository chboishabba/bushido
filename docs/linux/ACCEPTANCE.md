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

## Redirect, iframe and worker network acceptance

With `fixture-filter-webkit.json` installed before first navigation, load the fixture. An *allowed* `/allowed-redirect-to-blocked` must reach the HTTP server, but its `/blocked-redirect-destination` target must not. An allowed `/allowed-nested-frame` must reach the server but `/blocked-nested-script.js` must not. An allowed `/allowed-worker.js` must arrive but `/blocked-worker-import.js` must not. Check `/__assert` and negative-control runs without filtering; a green Python self-test alone is not proof of WebKitGTK enforcement.

## Glance global preference and document-policy consistency

With Glance open, change global adblock on→off→on and verify both the native WebKit filter and original Rust document policy follow the changed value. Compare ordinary tabs and Glance on the same destination; neither should retain creation-time document policy. Confirm the fallback without experimental rules does not pretend that native filtering was updated. On-page-load cosmetic/scriptlet capture is a separate OPEN parity gate.

## Cosmetic load policy toggle

Create a tab with adblock initially on, change it off, reload, then toggle on again; the page-load cosmetic injection should consult the current native debug preference rather than a captured initial boolean. Repeat with initial adblock off. Inspect pre-registered document-start initialization scripts separately; this source change does not remove those scripts and cannot be marked complete cosmetic parity.

## Native filter registration rollback

Force a Linux `add_child` failure after native state registration and confirm the per-tab preference, applied mode and generation ticket are cleared. Reuse the same ID to create an unfiltered tab and check that no stale state or callback takes effect. Run the associated pure Rust tests and exercise both normal tab and Glance constructor errors before considering this accepted.

## Document-start privacy/cosmetic separation

Create an unwhitelisted tab with adblock enabled, then turn adblock off and reload: the new document must not install the cosmetic observer or inherit pending CSS/scriptlets from the previous origin. Reverse the toggle, including a tab originally created with adblocking disabled: conditional cosmetic injection should become available. Verify `content_blocker.js` WebRTC restriction is present at document start under either toggle and on whitelisted origins. Check same-document navigation, page-load timing and native WebKitGTK IPC separately before claiming parity.

## Linux cosmetic IPC

Exercise the dedicated channel with valid class/ID batches, invalid JSON, oversized messages, whitelisted pages and generic-hide exceptions. Verify unrelated messages cannot enter ordinary Tauri dispatch. Confirm subsequent DOM mutations and first-load timing on WebKitGTK; no runtime acceptance yet.

## Cosmetic batch acceptance

Exercise more than 256 distinct DOM classes/IDs, escaped Unicode/control characters, a late bridge, and stale URL messages. Batches must fit the native receipt limits and eventually drain when the channel is available. URL equality is not a frame-authentication mechanism.

## Linux media telemetry

Exercise playing/paused/ended and metadata updates against the existing frontend event consumer. Reject invalid kinds, oversized payloads and mismatched URLs; verify tab ID cannot be supplied by page content. Test late channel registration and rapid transitions. URL equality is not authenticated frame provenance. No native runtime pass claimed.

## Linux native shortcut acceptance

Test Ctrl+T, Ctrl+W, Ctrl+Shift+T, Ctrl+= and F11 in main, tab and Glance views; verify one frontend action per key sequence, unbound keys reach the page, rebind uses the live map, focus-out clears held keys, and synthetic JS keyboard events cannot reach the native GTK handler. Test Wayland and X11 independently; global plugin initialization remains an open compatibility check. Pure mapping tests added but not executed.

## Video and PiP acceptance

Exercise initial/lazy video, removal, background tabs, active-tab switches and navigation reset. Confirm unsupported PiP is reported, disabled video is respected, synthetic overlay clicks cannot activate PiP, and confirmed enter/leave states—not requested toggles—drive the toolbar. Test real WebKitGTK standard API availability; no native detached PiP fallback yet. Frame provenance and URL-level stale messages remain open.

## Linux FindBar acceptance

Search text, quoted/escaped strings and absent terms; test forward/backward and query clearing; issue rapid different queries and switch tabs while a result is outstanding. Linux callback returns the count to the initiating trusted command; do not accept unsolicited page messages as Linux search results. Count is a bounded visible-text estimate, not an exact native find-controller count. Validate runtime on WebKitGTK and Windows.

## Linux Glance link context menu

Right-click an HTTP(S) anchor in an ordinary tab and select Open in Glance. Verify the existing preview opens in the originating workspace and follows the existing filter policy. Check non-link menu, disallowed schemes and URL credentials, stale menu after navigation, closed source tab, normal Alt-click behavior without the Windows interceptor, and synthetic JavaScript messages. Native shortcut/pinned-tab automatic preview parity is still OPEN. No runtime acceptance yet.

## Linux native downloads

Exercise simultaneous same-name downloads in one shared workspace and another workspace, authenticated and POST/blob downloads, a failure followed by a separate success, cancellation before/after destination selection, tab destruction, MIME routing and collision protection. Verify no cookie export or replay GET, exactly one record, native-only controls hidden and completed files preserved on record removal. Verify actual WebKit data counts and callback ordering; no runtime pass claimed.

- Race cancellation against an already-successful finish: a completed destination must survive. Verify long multibyte suggested filenames and cancellation dispatch timeout versus eventual terminal result.

### P4.1 flat-directory publication controls
On Linux run the source tests and native downloads into the chosen root and MIME route. Verify report.pdf, report (1).pdf etc. with concurrent transfers; pre-existing files and dangling symlinks must remain unchanged. No partial bytes may be published in the flat root. Verify failed hard-link publication (e.g. a filesystem without hard links) retains the original completed bytes in staging, with visible warning and working Open File/Open Folder actions. Verify collision exhaustion, cancellation before destination, cancellation racing successful finish, failure cleanup and cleanup errors do not remove a published file. Verify configured non-UTF-8 paths fail explicitly. Local kernel primitive probes are not native acceptance. Ask-location UI and restart recovery remain OPEN.

### P4.1 native ask-location controls
With askDownloadLocation enabled on WebKitGTK >= 2.40, Save a browser download to a different local folder and filename; verify the chosen name and bytes, original authenticated request, no file/staging before acceptance, and the existing no-overwrite suffix policy on collision. Cancel from the dialog, remove from the download panel during a pending dialog, inject a native failure/finish while pending, and attempt repeated/stale responses. Confirm no destination is committed after cancellation and no orphan chooser survives terminal completion. Test concurrent dialogs and ordinary/Glance sources, actual GTK/portal local-only behavior and Wayland/X11. On older WebKitGTK verify explicit failure for ask-location and unchanged automatic downloads. Invalid/non-UTF-8 paths must fail rather than be rewritten. Run pure Rust regressions; this is source-only pending native acceptance.

### P4 Glance Alt-click policy controls
On native Linux click credential-free HTTP(S) links with Alt+primary button: source should remain unchanged and only Glance destination should load in its workspace. Test internal/external hosts, redirects, script-triggered navigation, keyboard activation, ordinary/middle/Ctrl/Shift clicks, non-HTTP and credential URLs, focus loss, stale source URL, close/source workspace changes, failed/late preview creation, reused tab IDs and native-filter retry overlap. Confirm no privileged remote-page IPC. Test target=_blank, iframe and script-cancelled anchors separately; no exact identity with Windows DOM click interception is claimed. Run gesture and URL Rust regressions and validate changed Wry dependency graph with Cargo --locked. Pinned link previews, promoted views and GTK/Wayland/X11 acceptance remain OPEN.

### P4 native pinned automatic preview controls
Pin/unpin a live ordinary tab and click direct primary links to same hostname, other ports/schemes on same hostname, subdomain and different hostname. Only pinned cross-host links (or valid explicit Alt-click) should preview; Ctrl/Shift/middle/keyboard/redirect/script-triggered navigation must retain their prior behavior. Page writes to __bushidoPinned must not affect the Linux decision. Test live suspension/resume, destroyed recreation, crash recreation, current/legacy session restart, pinned internal-page navigation, settings recreation, and stale IDs after close; no pin may leak between views. Check rapid pin toggles during creation/close and inactive workspaces separately; these are open native ordering gates. Verify ordinary panels are not hooked. Promoted/preloaded views and new-window/frame cases remain OPEN. Run both new Rust regressions and native fixtures before acceptance.

### P4 promoted Glance lifecycle and feature inheritance
Open Glance from workspace A with a distinct authenticated session from workspace B. Verify expansion reuses the exact WebView/context and current URL/title, and workspace mismatch is rejected before promotion. Test double expand, expand before open acknowledgment, close before open completes, close racing promotion, source close, explicit workspace switch, missing view/panel, duplicate ID, stale close_glance after successful promotion and native failure paths. Confirm late-promoted views are closed through close_tab, not close_glance. Observe exactly one download/media/shortcut handler per view; verify video and media events, title/load changes, Alt-click/context menu and pin auto-preview after promotion. No new remote Tauri permissions. Run actual WebKitGTK and Windows regression; full ordinary-tab security/cosmetic parity and preloaded views remain OPEN.

### P4 Native FindController controls
Run Linux native find on pages with visible/hidden text, Unicode, frames and repeated terms. Compare WebKit engine's found/failed results, forward/backward wrap and highlighting; confirm zero matches and overflow display (100,000+ not a fabricated exact count). Verify no page JavaScript count on Linux. Exercise fast repeated queries, clear during in-flight find, close/unmount/tab switch, navigation during find, destruction/recreated IDs, duplicate and out-of-order request IDs, delayed or absent signals. Confirm at most one outstanding native operation and no stale result assigned to a new request; an undrained timeout must quarantine the view until recreation. Verify Windows unchanged search-event transport and any frontend cleanup interaction. Native current-match ordinal is not exposed by this API; UI displays total only. Run new Rust tests and native fixtures; no runtime acceptance yet.

Add a page with at least three matches and exercise Next/Previous repeatedly on engines affected by WebKit bug 325421. Verify the displayed total comes from counted-matches, not traversal found-text value 1. Clear during each of the two phases and force late/wrong-phase signals, missing counted-matches and expired GUI dispatch. Confirm slot ownership persists until the matching terminal signal; no stale signal can be reinterpreted as a different phase. Native engine signal guarantees still require acceptance.

### P4 Linux PiP capability/automation controls
Run node --test scripts/linux/test-pip-policy.cjs for mocked policy. On native WebKitGTK test document API false/absent, disabled/not-loaded videos, eligible media, trusted/untrusted activation, rejected request, active exit, timeout and navigation. Require observable detached compositor window and same original media session to accept detached PiP; neither a prompt nor a document boolean alone proves it. Verify unsupported engine surfaces a truthful error, no duplicate request or replay, and Windows regression. See MCP_AUTOMATION.md for separate driver/fixture acceptance. No hosted CI changes.

PiP mock suite local receipt: Node v22.16.0, 9/9 pass, blobs 1923ffcd772640a5b5aadcc58827cfd870e2fbd5 / d1cbd1491d3f4397de25c12c4a98c0aa4a45357e. Additional case delays the native request promise, toggles repeatedly and fires prompt expiry; requires one entry call while pending. Actual WebKit promise/window behavior is unverified. Local preflight includes the mock suite.

### P3/P4 explicit native saved-login fill
Use a test-only vault entry keyed by canonical HTTPS origin (including nondefault port). Verify native context action -> native account chooser -> single isolated-world fill into the same empty visible POST form; no auto-submit and no secret in page messages or app event. Negative fixtures: HTTP, invalid TLS, distinct port/subdomain, legacy hostname key, multiple/hidden/new-password fields, cross-origin action or submit override, form replacement, same-URL reload, navigation while chooser open, lock/disable while selecting, deleted entry, panel/Glance, synthetic page messages, duplicate activation. Existing global vault needs explicit workspace-partition acceptance separately. Verify actual WebKit isolated world and GTK chooser, settings persistence race and secret lifetime; source is not security acceptance. Rust origin test queued; no hosted CI.

Manual provisioning: from trusted Passwords settings, add HTTPS URL including nondefault port and verify exact canonical key on Linux. Windows uses legacy hostname. Verify native status message contains no secret. Local Node v22.16.0 mock receipt: 6/6 pass, 164.52 ms, source 8bc8a5acfd4fb93b6635118caa47540cbbc9c983, test 0a5cf55dbd2757cdccdb00b24544a3c19f247086. Does not prove real isolated world or native security. Add negative lock/reunlock race, stale destroyed view, hidden/offscreen workspace and hostile form mutation cases to native suite. No hosted CI.

### Vault lifetime and metadata listing
Verify locked metadata requests fail, list output never contains password/ciphertext, selected reveal/copy and invalid IDs, reveal timer/lock/close, stale async results and Settings password list after lock/reunlock. Using synthetic entries, capture fill authorization, lock and unlock with the same key, then attempt old authorization: must reject before engine dispatch. Race lock against synchronous admission; distinguish already-queued engine dispatch from cancellation (not guaranteed). Verify setup refuses existing/partial metadata and rolls back both initial inserts together. Test old Windows behavior separately; no bulk-decrypt Settings response on either platform. Run session_tests and actual native test fixtures; unexecuted Rust tests and remaining workspace/AAD gates are not acceptance.

Lock versus unlock/setup: delay Argon2 after initial generation snapshot, lock, resume; stale transition must fail without installing a key or committing initial metadata. Force setup commit failure and duplicate installation. Check global App auto-lock when unlocked through Settings, reject wrong modal password without continuing save/retry, epoch-tagged event reorder and initial snapshot race. Local SQLite smoke 8/8 PASS as documented; Rust tests not executed.

### P3/P4 scoped vault v2
Use same HTTPS origin and separate usernames in workspaces A/B; verify native chooser and Settings list/reveal/copy/delete are isolated and nondefault ports remain distinct. Attempt missing workspace and cross-scope entry ID. Modify synthetic DB row workspace, origin, username or ID and transplant ciphertext/nonce between rows: selected decryption must reject; list metadata is not authenticated until decrypt. Verify selected username change before decrypt rejects. Legacy rows preserved and explicitly recoverable in Settings but never native-fill candidates. Create/delete/restart workspace and confirm new UUID never reuses old numeric ID; restore existing session IDs unchanged. Test interrupted schema creation, incorrect key, invalid nonce, failed wrapping, lock during save and stale UI replies after changing workspace. Rust tests pending native toolchain; no claim of complete isolation for trusted chrome or rollback resistance.

Local independent Python cryptography/SQLite reference smoke 16/16 PASS (see roadmap). Native Rust integration and keyring tests remain unexecuted. Verify old Linux write/update commands reject, legacy recovery remains accessible, and delayed list/reveal/delete/manual-save replies cannot repaint a different scope or an earlier instance after switching away/back. Keep historical workspace IDs when restoring sessions; explicitly review old backup/orphan recovery separately.

### P3/P4 submission-triggered save consent
Run node --test scripts/linux/test-vault-save.cjs (local 8/8 PASS, matching source/test blobs documented). Native fixture must prove isolated-world message handler inaccessible from the default world/other frames, unique view ownership, metadata-only notice and one selected credential read only after real GTK approval. Verify disabled password saving independently of autofill, locked vault, expiry, duplicate/no consent, synthetic events, cross-origin/action overrides, TLS error, same-URL reload, form/value mutation, view destruction, lock/reunlock during prompt/read/wrapping, wrong workspace and same account insert race. Check password never appears in application events/logs or native prompt text; no overwrite and no login-success claim. Full-page/redirect flows deliberately cancel; no automatic capture parity acceptance. Test actual native compiler and GTK/WebKit, platform wrapping, post-dispatch races and memory lifetime separately.

Superseding mocked save policy receipt 10/10 PASS (deadline without timer and exact discard added). Actual insert SQL executed local SQLite 3.46.1, 7/7 PASS; concurrent writers and platform key wrapping still native gates. Validate actual GTK cancellation and locked-vault discard with synthetic credentials; note same-document save is not verification of a successful login. No hosted CI.

Exercise save dialog expiry, isolated-world read timeout and fill callback timeout on the actual GLib main context without Tokio runtime entry; no panic, no secret persistence after expiry, no fabricated success. Rust compilation/runtime still pending.

### P4 Glance security inheritance
Verify initial HTTP upgrade and later HTTP rejection when HTTPS-only, unsafe schemes and embedded userinfo rejected, native filter pending retry invalidated by denied navigation; correct destination whitelist, cosmetic and cookie policy plus service worker/font hardening at document start, original workspace context. Before promotion prove no Linux vault isolated save handler or fill context action installed; after successful same-workspace promotion, same native view, exactly one credential hook installation, scoped fill/save and policy disable controls, no replay/reload; repeat promotion and stale close, navigation during transition, missing view, wrong workspace. Verify callbacks actually installed and native failures reported. Cosmetic timing, live settings changes, older Windows vault, preload and actual native acceptance remain open.

Add Glance initial/redirect URL tests and shared hardening flag tests; verify failed creation restores actual current tab/split layout, including switching workspace before failure. Reused save JS policy suite locally 10/10 PASS (180.70 ms), no actual Glance or GTK integration execution. Three new Rust unit tests pending compilation.

### P4 native history / restart
Test process kill before destination, during transfer, after completed-stage receipt, after hard link before final receipt, after final receipt before cleanup, and after cleanup. Record actual persisted state; never issue GET or resume at restart. Check interrupted/failed and missing/changed completed file, dangling symlink, private history modes, redacted URL/error/request data, write failure, user clear/remove preserves files and remains cleared after restart, old non-native manifest unaffected. Validate power-loss directory/file sync ordering and orphan behavior separately; no native execution claimed. Three Rust tests pending toolchain.

Superseding completion order: private staged receipt only, directory sync before final receipt, staging unlink only after durable final receipt. Exercise clear while finishing and startup snapshot race. Local Python SQLite/OS reference 11/11 PASS, including abrupt subprocess exit before transaction commit; not WebKit or power-loss acceptance. Measure synchronous durable write latency on GTK thread.

### P4 speculative preload containment / workspace authority
Verify hover alone creates no hidden native WebView and sends no speculative request on Linux or Windows. Verify ordinary link and form navigation retain their original request/method/history; legacy preload messages have no handler and URLs cannot promote hidden views. Verify Glance refuses stale/missing source, panel source, wrong workspace, changed committed URL and reused source ID even with identical workspace; native profile remains source-owned. Promote valid preview once, close/recreate source and reject old preview, check fill/save denied if native workspace differs. Source tests pending Rust/GTK; speculative optimization intentionally disabled and not parity. Future preload requires separate request/ownership/side-effect acceptance.

Add delayed native Glance event/menu activation after source close/recreate with identical URL/workspace; reject old generation before constructing new view. Verify promoted view receives one generation and its hooks are inert after removal, including old fill/save consent and worker admission. Verify lifecycle serialization with concurrent create/close/promote; native engine destruction acknowledgement and Windows producer provenance still OPEN.

### P5.1 Linux candidate package
Follow docs/linux/PACKAGING.md. Local static package-check self-test 9/9 synthetic archives PASS; actual Bushido compile/build remains OPEN. Build on clean pinned native host, inspect exact candidate and receipt, resolve DT_NEEDED and package dependencies, check signed upstream update channel absent Linux, desktop icon/name/category/exec and no MIME/default browser association, license/source offer, install/launch/restart/uninstall in disposable profile and Windows NSIS regression. Two independent builds must demonstrate reproducibility before marking it. No production release approval implied.

P5.1 local build script syntax PASS and missing-tool fail-fast exit 2 verified on exact blob `0f2e8fb2986c6e56e003851bb73dab3a47cd9d2f`; native dependency/compiler gate remains OPEN. Static checker now runs from local preflight. No installer exists from this run.

### P5.1 external activation and instance guard
On actual Linux, start cold with HTTP(S) URL, then send second launch, empty launch, multiple and malformed links, oversized batch, hostile schemes/userinfo, queue-full and stale socket. Verify one process holds lock prior to DNS/cache writes, secondary never initializes services, peer UID/private mode, crash releases lock, stale socket only removed by owner, no deletion of unrelated file, no fallback on timeout. Confirm actual main-WebView-only command, explicit named-workspace prompt, onboarding delay, rejection, changed workspace cancellation, original URL goes through normal tab policy. Test real GTK window focus/confirm and same-UID threat boundary. Do not register desktop MIME handlers until native cold/warm route and instance acceptance; Windows unaffected. Three Rust tests pending.

Local independent kernel primitive reference 11/11 PASS (separate processes, SIGKILL, O_NOFOLLOW, SO_PEERCRED, framed socket/ack/stale inode); not compiled Rust. Verify explicit authority, backslash/userinfo rejection, same primary/secondary bounds, private socket 0600, poll disabled on Windows, same-workspace updates do not cancel and genuine switch does. No MIME registration until native acceptance.

### Local consolidated P0 receipt
Run from a clean committed checkout with Python 3.11+:
```sh
python3 scripts/linux/accept-local.py --self-test
python3 scripts/linux/accept-local.py
```
The runner creates a private /tmp/bushido-acceptance-* directory; retain it before reboot, review logs for sensitive information before sharing, and report receipt.json plus failing stage logs. Exit 0 means the listed local P0 stages completed, not native GUI/security acceptance; exit 1 indicates a failed stage or changed checkout; exit 2 indicates blocked prerequisites or setup failure. Incomplete receipts must never be considered accepted. The runner invokes npm ci/build/test and locked Cargo check/test on the original checkout; it does not install OS packages, launch Bushido, change defaults, or access its profile. Continue with manual native cases above or external driver once P0 passes. Existing preflight remains available.

### P5.1 source candidate
Run python3 scripts/linux/test-source-candidate.py; generate two fresh directories from the same clean commit, compare receipt and archive hashes, inspect notice paths and inventory. Negative controls: export-ignore, export-subst, dirty checkout, symlink and existing destination. No native build needed. Verify actual binary/source association and independent license/source review before distribution. Tests written, not yet executed in this packet.

### P4.2 Linux screenshot and clipboard
Use synthetic colored quadrants, scrolling page, split views, multiple workspaces, zoom and fractional/2x display scale. Verify visible PNG contains only requested live tab, area crop agrees with frozen image, stale/closed/recreated or navigated tab results discarded, failure leaves layout visible, full-page option disabled. Check empty/oversized viewport and PNG limits, invalid base64/IHDR, native snapshot timeout, PNG alpha, paste into another GTK and non-GTK application on X11/Wayland while browser remains open, ownership after other clipboard writes and app exit. Save to test Downloads, concurrent collision, traversal, dangling symlink and permissions; partial I/O failure is explicit (may leave partial file). Two Rust regressions pending user execution. Native GTK/WebKit binding and integration unverified; no hosted CI.

Negative controls: direct IPC from a remote child view must reject visible capture/save/image copy even when target ID exists. Switch workspace/tab or navigate while screenshot or annotation is open; old image must close and restore current layout. Same-URL reload and rapid away/back require separate native checks. Clipboard dispatch reports submission, not verified cross-application paste.

### P4.2 print and inspector
Use a page replacing window.print, local print CSS and PDF printer; explicit chrome action must invoke native dialog, not replaced page JS. Cancel and accept, inspect actual print output separately; no success claim from dialog return. Try duplicate action during nested GTK loop, hidden/loading/stale/panel/remote child IPC, close/navigate while dialog visible. Verify inspector opens only debug normal enabled views, disabled preference after reload, panel/Glance/promoted Glance denied until eligible recreate, release denied, frontend errors visible from menu/quick action/shortcuts. Test actual Wry settings and native callback lifetime; live revocation and document-originated print requests remain open. Two Rust tests pending user-side native run.

Check native print response cancelled vs submitted; verify actual print output separately. Non-Linux response requested means evaluation dispatch only. Change URL between command and GTK callback and require rejection. Navigation during modal and same-URL reload remain distinct acceptance cases.

### zkSEC/zkperf local evidence
See ZKSEC_ZKPERF.md. Run witness-receipt fixture from consolidated preflight. On isolated debug app, set BUSHIDO_LINUX_PERMISSION_OBSERVATION=1; capture trusted main command baseline and after native permission fixture, compare count and independent actual deny behavior; remote child access and disabled/release must reject. No origin attribution or grant. Wry standalone saturation test needs its own locked fork test invocation; root cargo test does not run dependency unit tests. Collect real binary closure, matching source/debug, raw traces, derived models and build events locally; no proof/acceptance from index alone. Tests written, not executed.

### P2.4 resolver control
Use isolated test_dns binary and dns_test.py with randomized transaction IDs; confirm mismatches reject, /level unavailable, main-only native control and policy generation ABA, startup policy, no proxy/redirect and pinned upstream packet destinations; failure injection in maximum vs strict, no plain DNS bootstrap. Verify Linux default starts no resolver, debug opt-in only, preconnect denied. Cache disabled: report latency as proposal observations, not acceptance thresholds. Actual WebKit process coverage and origin-scoped network enforcement remain OPEN. See DNS_CONTROL.md.
