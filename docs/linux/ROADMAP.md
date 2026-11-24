# Bushido native Linux port — execution roadmap

**Updated:** 2026-11-20  
**Working branch:** `linux-port-pr4-source`  
**Development PR:** [chboishabba/bushido#1](https://github.com/chboishabba/bushido/pull/1) (draft)  
**Original contribution:** [visualstudioblyat/bushido#4](https://github.com/visualstudioblyat/bushido/pull/4), AdithyaaSivamal, `f83d133`.

## Definition of done

Produce a native Linux application that browses reliably, enforces its advertised privacy guarantees, keeps credentials and workspace data isolated, and packages reproducibly. A compilation-only port does **not** satisfy this definition.

**Current baseline:** PR #4's Windows-only `controller()` guard, plus cross-platform clipboard dependency, child-pane layout adjustment, fail-explicit unsupported features and non-Windows plaintext key-wrap rejection. These changes have **not** been accepted by Linux compilation, runtime testing or a privacy audit.

## Execution order and gates

| Gate | Phase | Concrete deliverable | Acceptance evidence | State |
| --- | --- | --- | --- | --- |
| P0.1 | Build | Pinned Linux host prerequisites and WebKitGTK/GTK system deps | Installer or setup script succeeds on a clean Linux image | OPEN |
| P0.2 | Build | `cargo check --locked`, `cargo test --locked` (forked Tauri/Wry included) | Captured logs, exact toolchain and distro | OPEN |
| P0.3 | Build | `npm ci && npm run build` | Captured frontend build logs | OPEN |
| P0.4 | Build | Linux CI build with cold cache | Green workflow tied to exact commit SHA | OPEN |
| P1.1 | Runtime | Tauri window, GTK/WebKit child webviews, basic navigation | Screenshot, runtime logs and local HTTP fixture | OPEN |
| P1.2 | Runtime | Tab lifecycle, split panes, resizing, history, session restoration | Deterministic UI/integration scenario | OPEN |
| P1.3 | Runtime | Webview popup/new-window, error handling, URL scheme safety | Adversarial local navigation fixture | OPEN |
| P2.1 | Network | Shared Rust adblock decision for document and subresource requests; header rules still inline | Policy unit tests + Windows parity vectors | PARTIAL — policy source and tests added; runtime parity unverified |
| P2.2 | Network | Debug-only native WebKitGTK filter adapter with fixture JSON attached before navigation | Local HTTP fixture verifies blocked requests never reach server | PARTIAL — adapter source added; browser/network acceptance unverified |
| P2.3 | Network | HTTPS-only, redirects, third-party matching, scriptlets/cosmetic filters | Positive and negative network fixtures including iframes/workers | OPEN |
| P2.4 | Network | DoH reaches WebKit network stack, no fallback/leak at maximum privacy | Packet/DNS capture and failure-injection tests | OPEN |
| P3.1 | Security | Linux per-workspace data_directory plus acknowledged WebKitGTK clear | Two-workspace fixture; no cross-workspace leakage; clear actually erases | PARTIAL — completion propagation source added, Linux runtime unverified |
| P3.2 | Security | Linux Secret Service key + versioned ChaCha20-Poly1305 envelope for sync and vault | Rest/encryption, lock, migration and failure tests; no raw fallback | PARTIAL — crypto source/tests written, keyring/runtime acceptance open |
| P3.3 | Security | Native Linux WebKitGTK permission requests explicitly denied by default; origin-scoped prompt mediation still missing | Camera/location/notifications/clipboard fixtures | PARTIAL — deny callback committed, not build/runtime tested |
| P3.4 | Security | Fingerprint script timing and cross-realm parity on WebKit | Document-start, frame and worker tests | OPEN |
| P4.1 | Parity | Downloads, authentication cookies, pause/resume and error reporting | Download fixtures; compare explicit Linux fallback capabilities | OPEN |
| P4.2 | Parity | Screenshot, image clipboard, zoom, DevTools and print | Executable feature matrix | OPEN |
| P4.3 | Parity | Suspend/resume, media, CPU/RAM behavior and shortcuts | Lifecycle/stability tests | OPEN |
| P5.1 | Release | Linux Debian configuration separated from Windows NSIS; local candidate builder + static checker | Install/uninstall, dependency closure and smoke tests on target distro | PARTIAL — source configuration, synthetic package tests PASS; actual build OPEN |
| P5.2 | Release | No regression to Windows WebView2 implementation | Windows build and privacy regression suite | OPEN |
| P5.3 | Release | Attribution-preserving upstream handoff | PR description, source receipts and maintainer review | OPEN |

**Gate policy:** Mark a gate accepted only with artifacts/receipts linked to the exact commit and platform. Source edits alone are insufficient.

## Critical path

```
P0 clean build -> P1 running webview -> P2 actual interception and DNS
                             \-> P3 isolation/keyring/permissions
P2 + P3 -> P4 parity -> P5 release
```

P2 and P3 are independent in parts and should progress in parallel after the runtime is established. **Never advertise privacy parity before P2/P3 acceptance.**

## Deliverable boundaries

- **M0:** Developer Linux build: compile logs + frontend build + CI. No privacy claim.
- **M1:** Browser shell: opens local and remote URLs; tab/split/session tests. Still experimental.
- **M2:** Security candidate: request filtering, HTTPS, DoH, storage isolation and credential handling verified. Limited beta only with disclosed remaining gaps.
- **M3:** Release candidate: feature matrix, regression suite, packaging and maintainers' review.

## P2.2 document-start source cleanup

- Removed the obsolete page-load `inject_cosmetic` clone after moving WebRTC hardening to the unconditional document-start registration; kept the original privacy script source as the single installed producer.
- The cosmetic observer remains deliberately conditional at page load. This source cleanup does not prove first-frame timing or Linux IPC.

## P2.2 document-start privacy/cosmetic separation

- `content_blocker.js` implements WebRTC ICE/STUN/TURN hardening, not cosmetic ad blocking. It is now registered as an always-on WebView initialization script, independent of cosmetic whitelist and adblock setting.
- Removed permanent document-start registration of `cosmetic_observer.js`. The existing page-load path installs the observer only when adblocking is enabled, the current origin is not whitelisted, and the engine permits generic hiding.
- Cleared previous-origin pending cosmetic payload on **every** candidate navigation, including transitions with adblock turned off. The frontend now reloads open tabs on global preference changes even if the experimental native filter is unconfigured, rather than leaving obsolete cosmetic DOM state.
- Source-level correction only. Document-start vs post-commit timing, cosmetic observer IPC on Linux, cookie blocker persistence and full original Rust/WebKit parity remain OPEN.

## P2.2 normal-tab WebView construction rollback

- `create_tab` now cleans up its pre-registered Linux native filter state when Tauri's `window.add_child` fails, matching the earlier Glance cleanup path.
- Added pure Rust tests for live-preference vs applied-state separation and failed constructor rollback/tab-ID reuse. This prevents a later creation from inheriting a failed WebView's native filter intent.
- Only source inspected; native compilation and real WebKitGTK error paths are not yet verified.

## P2.2 live cosmetic page-load preference

- The ordinary tab's `on_page_load` path now checks the same current Linux native-debug adblock preference before applying cosmetic resources. Previously, a tab created with adblock on would inject cosmetic hiding after native filtering was turned off, and vice versa.
- The static initialization scripts attached at WebView creation, in-page observers and Windows-specific content injection are not all removable dynamically; full preference equivalence remains OPEN. The change only affects this Rust page-load decision.

## P2.2 global preference consistency for Glance and tab navigation

- The frontend's live `adBlocker` transition now updates the visible Glance WebView in addition to ordinary tabs; it awaits a native update acknowledgement before reloading each view.
- The native Linux debug document-navigation callbacks for ordinary tabs and Glance now consult `NativeFilterUpdateState.preference_for` rather than a preference captured at WebView creation. This aligns Rust document decisions with native filter updates for explicitly opted-in sessions.
- Remaining limitations: cosmetic/on-page-load injection still captures the original adblock toggle; global preference changes in non-debug/Linux production and Windows are not covered. Navigation/GTK runtime acceptance and full ABP parity remain open.

## P2.2 redirect/frame/worker acceptance fixture expansion

- The local HTTP request recorder now has an allowed `302` redirect whose **destination** is blocked, an allowed iframe that loads a blocked script, and an allowed worker that imports a blocked script. It requires the allowed controls to arrive and rejects server-observed blocked destinations/child resources.
- The existing fixture-native WebKit JSON rule blocks all `/blocked*` paths, including the newly added nested cases. Added Python negative controls for redirect destination and frame/worker leaks. These are executable harness assertions, **not a browser runtime pass**.

## P2.2 Glance navigation policy weld

- Glance's `on_navigation` now evaluates destination-specific whitelist state and the original Rust document adblock decision. Under an enabled Linux debug native filter source, it uses the shared per-tab navigation sequence and applied-filter state to deny mismatched navigation, compile/apply the correct native rule set, and retry only if still current.
- Successful Glance navigation emits `tab-url-changed` for the existing frontend Glance URL bookkeeping.
- The callback handles top-level navigation attempts and reuses the existing experimental rule source. Redirect chains, frame/resource semantics, live global adblock changes in Glance, and native GTK acceptance remain OPEN.

## P2.2 Glance native filter initial-state weld

- `open_glance` now takes the frontend's live global adblock preference, computes the Glance **destination** host's whitelist (not the source tab's), and supplies that decision to the same per-WebView native filter builder.
- Linux debug Glance registers applied-filter and adblock state before its first load, with cleanup on both failed construction and explicit close. The frontend now handles creation failures instead of silently leaving an orphaned overlay.
- This closes the previous unconditional native-filter bypass in the Glance **initial** creation path at source level. Subsequent Glance navigation, redirects, promotion and global preference changes still require dynamic coordination and native verification. Only experimental source rules are filtered; production ABP parity remains OPEN.

## P2.2 same-URL and failed-navigation retry hardening

- Every candidate top-level navigation now receives its own monotonically increasing sequence, including repeated clicks on the same URL and URLs subsequently rejected by URL-scheme or policy checks. Only the latest matching **sequence and URL** can retry after filter compilation.
- A pending retry is also rechecked against the live destination whitelist and adblock preference before navigation. This prevents an asynchronous result from retrying under an obsolete desired mode after a preference change.
- Added source-level unit regression cases for same-URL repeats, rejected navigation invalidation and tab close/ID reuse. **These tests have not been executed.** Redirect and subframe request boundaries and native GTK acceptance remain open.

## P2.2 latest-destination retry cancellation

- Track a per-tab pending navigation URL when the initial attempt is denied for native filter transition. On completion, only the current matching destination can be retried; a subsequent user navigation supersedes the old URL, including a navigation requiring no filter change.
- Tab closure clears pending destinations. This prevents a common stale-navigation retry, but repeated requests to the **same URL**, asynchronous timing, redirects, subframes and adblock preference transitions still require native WebKitGTK acceptance.

## P2.2 pre-navigation deny/update/retry (source implementation)

- Linux debug tabs with a configured native filter source now keep an **applied filter state** separate from both outstanding compilation tickets and the latest adblock preference.
- In the existing Rust document-navigation callback, destination whitelist state is inspected **before approval**. If it requires a different native filter, the callback denies that navigation, schedules the acknowledged native filter change and retries only on success. Compilation failure leaves the original request denied.
- Native update callbacks record applied state only for their current ticket. Tab close clears preference and applied state.
- This is a bounded **top-level navigation** interception strategy, not a fully verified WebKitGTK pre-network barrier: callback ordering, racing redirects, concurrent pending retries, frame navigations and first-created filter-store state remain unverified. Actual Linux server-observed testing is required before P2.2 acceptance.

## P2.2 asynchronous native-filter update ordering (source implementation)

- New `NativeFilterUpdateState` issues globally monotone, per-tab latest tickets. The WebKitGTK save callback checks the ticket **before modifying** native filters; superseded callbacks return explicit errors and cannot re-enable a stale rule set.
- Disabling native filtering now acknowledges removal as an applied change, so the frontend's global-toggle path reloads after both directions of a successful toggle.
- Timeout/dispatch errors invalidate the ticket. Closing a tab invalidates its outstanding callbacks, and a newly created tab reusing the same ID cannot accept an older ticket.
- Pure Rust tests cover out-of-order completion, tab close/ID reuse and independent tab updates. **Not yet run**; GLib main-thread and rapid-toggle runtime acceptance remain OPEN.
- This addresses update ordering, **not** the distinct pre-navigation first-request race or the original Rust ABP completeness.

## P2.2 global adblock preference updates (source implementation)

- React now notices actual `settings.adBlocker` transitions after saved settings are loaded and requests `set_tab_native_filter` for existing live non-internal tabs. It reloads only after the backend confirms a native filter was applied, and surfaces errors.
- In ordinary Linux sessions without developer rule source, the backend returns `false` (not a claimed successful native operation); this avoids routine navigation errors when the experimental probe is disabled.
- The Windows WebView2 interception policy and existing Rust creation-time adblock capture still need a separate shared live-preference model; this change is specifically for the Linux native subset.
- Multiple rapid preference changes can race; no native acceptance evidence yet.

## P2.2 first-enable native filter compilation fix

- Existing tabs that started with adblocking disabled might never have written WebKit's compiled filter store entry. The live command now reads the configured rule JSON and compiles/saves it before applying, preserving the currently installed filter if compilation fails.
- This reuses WebKitGTK's content-filter store; redundant saves on repeated toggles and concurrent completion ordering remain open performance/correctness issues. No native run yet.

## P2.2 live whitelist state and locked WebKit dependency

- Included the Linux direct `webkit2gtk` dependency in the root application's Cargo.lock package entry.
- Rust navigation checks current destination whitelist rather than the creation-time boolean, and clears pending cosmetic data when navigating to an exempt site; page-load injection also consults the live hostname.
- React now requests a best-effort native filter change on each tab URL event. This event can occur **after network requests begin**, so the first-request leakage race is OPEN.
- Existing Windows COM subresource interceptors and initial tab-wide adblock configuration still capture creation-time decisions; full parity is OPEN.

## P2.2 native filter updates on existing tabs (source implemented)

- Added `set_tab_native_filter(id, ad_blocker)` to compute the actual site whitelist from Bushido's Rust state and dispatch an update into the live WebKitGTK `UserContentManager`.
- When enabled, it loads the previously saved native filter, preserves the old filter if that load fails, then replaces native filters on success. When disabled, it removes native content filters. The operation returns a result only after WebKitGTK acknowledges it, with a bounded 15-second timeout.
- The frontend's explicit whitelist toggle now calls this operation before reloading the tab and reports failures. The command is Linux-debug-only and returns false on other platforms (not a false claim of native enforcement).
- **Still OPEN:** live changes to the global adblock toggle, navigation between whitelisted and unwhitelisted sites, concurrent update races, the original Rust document-navigation closure's captured whitelist value, and real native acceptance. `remove_all_filters` assumes the WebKit manager owns only Bushido's native subset; preserving unrelated filters is a future strengthening.
- No claim of full ABP/rust-engine parity.

## P2.2 per-WebView native filter opt-in (source implemented)

- Added `native_content_filter_enabled` to Tauri runtime attributes and Wry WebView attributes; the Tauri builder forwards it through `tauri-runtime-wry` to the WebKitGTK backend.
- Bushido's `create_tab` sets the flag only when `ad_blocker && !site_whitelisted`. The WebKit native filter is attached only when that flag is enabled. A disabled/whitelisted tab skips native filter installation, even when the experimental rule source exists globally.
- Glance deliberately sets the flag false until its own settings and whitelist handoff is defined. This is a disclosed gap, not equivalent protection.
- The flag defaults false for unrelated views; the native rule producer remains debug/opt-in and **incomplete**. Linux runtime and per-tab positive/negative tests remain OPEN. Whitelist changes after initial view creation require explicit refresh/recreate or dynamic filter management in a later gate.

## P2.2 compiled-filter reuse across WebViews

- Vendored Wry now caches the compiled `UserContentFilter` on the GTK thread keyed by **exact JSON bytes**, and attaches that filter to each new view's `UserContentManager` before navigation.
- Changing the rule file contents forces recompilation; identical files reuse the compiled WebKit object across tabs and Glance views. This avoids recompiling several thousand hostname patterns for every new tab.
- The cache is debug-Linux-only, matches the current dev-probe boundary and remains **unverified at runtime**. Production settings updates and per-view opt-out are not yet supported.

## P2.2 Rust source-to-WebKit bootstrap (implemented; unverified)

- New `blocker::native_subset_from_peter_lowe` and `write_native_webkit_subset` construct deterministic native WebKit JSON directly from Bushido's current Peter Lowe filter source (downloaded list preferred, bundled fallback), rather than requiring the separate Python converter as the only producer.
- Opt in with `BUSHIDO_LINUX_NATIVE_SOURCE_SUBSET=peter-lowe` in a **Linux debug build**, and the app writes a private JSON rule file and receipt to its data directory, then configures the existing Wry pre-navigation native content-filter loader. Python's fixture JSON remains a separate manual probe.
- The subset parser deliberately accepts only bare `||domain^` host rules, rejects regex/options/cosmetic syntax, reports discarded lines and duplicates and refuses an empty output. Linux Rust tests cover source selection and parser boundaries; the live WebKitGTK execution is **not yet verified**.
- This **does not integrate full Rust adblock engine decisions**, cross-list exceptions, per-tab enable/disable/whitelists, redirect handling or DNS. Opt-in rule generation is for implementation/evaluation only and cannot constitute release privacy parity.

## P2.2 exception-overlap safety correction

- An original `||host^` native rule matches host descendants, so an exact-only `@@||host^` exception matcher could overblock a narrower exempted hostname. The producer now suppresses overlapping parent/child blocks for host-scoped exceptions, including option-bearing exceptions.
- Unknown/unscoped `@@` expressions force complete conversion refusal rather than potentially overriding allow rules. This can make the development subset unusable with some real lists; source-level safety is prioritised over inflated coverage.
- Added tests for parent/child overlaps, option-bearing exceptions and unscoped refusal. **No Python test execution or WebKitGTK native proof yet.**

## P0 CI coverage extension

- The frontend CI job and local Linux preflight now run both stdlib Python harness self-tests: network request fixture and ABP subset translator. CI remains **not accepted** until a completed green run is attached to a head SHA.
- These checks validate local producers; they do **not** replace a Linux WebKitGTK compile, native fixture run or network security receipt.

## P2.2 audited ABP hostname subset (partial; dev only)

- `scripts/linux/compile_webkit_subset.py` converts only unconditional `||hostname^` blocks into WebKit content-extension JSON. Exact matching `@@||hostname^` exceptions across lists suppress that hostname. All other ABP rules are skipped and counted.
- The translator is deterministic, bounded to a documented rule cap, checks malformed hostname syntax, and prints omitted/exception/truncated counts. `run-filter-subset.sh` runs translator self-tests and supplies the generated JSON to the existing native Wry probe before first navigation.
- **No complete EasyList parity**: advanced exceptions, per-resource options, redirects, cosmetic filters, and all other original adblock-rust semantics remain unsupported. Native WebKit execution and CI acceptance still OPEN.

## P2.2 native content-filter probe (partial)

- Vendored Wry now reads `BUSHIDO_LINUX_FILTER_JSON` **only in Linux debug builds**, compiles the WebKit content-extension JSON using `UserContentFilterStore::save`, and adds the compiled filter to the webview's `UserContentManager` before the first load. Compilation errors, missing manager and 15-second timeout fail view creation, rather than continuing unfiltered when the explicit test is enabled.
- `scripts/linux/fixture-filter-webkit.json` blocks the six `/blocked*` paths of the existing local HTTP fixture and leaves the allowed controls reachable. `scripts/linux/run-filter-probe.sh` selects it for `tauri dev`; server-side `/__assert` is the acceptance signal.
- This is a **dev-only native enforcement test**. It does not compile EasyList/uBlock rules into WebKit JSON; production adblock parity, per-site whitelist, redirect processing, header rules, HTTPS-only and DNS remain OPEN.
- The compile helper runs a bounded GTK nested event loop before page load; compile/runtime and GUI reentrancy testing are required before promotion. **No Linux test receipt available yet.**

## P2.2 server-observed network acceptance fixture

- `scripts/linux/network_fixture.py` provides a stdlib-only local HTTP server with allowed and prohibited resource paths (script, image, fetch, iframe, worker, redirect). It records **what reached the server**, and `/__assert` rejects both observed forbidden resources and missing allowed controls.
- A Python harness self-test was added. **Neither a live WebKitGTK run nor network interception is demonstrated**; P2.2 remains OPEN. The fixture is an observable acceptance boundary for the upcoming native content-filter adapter.

## P3.2 persistence and keyring failure hardening

- Encrypted Linux identity writes now use a randomly named, owner-only (0600) temporary file, `sync_all`, then atomic rename. Existing keys.dat is not truncated if the write fails.
- Public peer/device metadata is written after successful secret wrapping and ciphertext persistence.
- Secret Service lookup failure (including empty stdout with failed exit status) now produces an error, **never the 'no key exists' result**. This trades compatibility with some `secret-tool` not-found exit conventions for a fail-closed guarantee; initial key creation may require an explicit provisioning path depending on distro and must be exercised before acceptance.
- Added a Linux regression test for atomic replacement, file permissions and temporary-file cleanup. Tests are still **unexecuted**.

## P3.2 Linux encrypted secret storage (partial; source-level)

- Linux `dpapi_encrypt`/`dpapi_decrypt` now use a versioned, AEAD-authenticated envelope (ChaCha20-Poly1305 + random 96-bit nonce) under a 256-bit wrapping key kept in the desktop Secret Service via `secret-tool`.
- Secret Service credentials are written via child stdin, not a process command argument or alongside encrypted data. The key is re-read after creation; absent, locked or malfunctioning keyrings return an error. Historical plaintext formats and tampered blobs are rejected.
- Deterministic Rust tests cover round-trip, wrong key, mutation, legacy plaintext rejection and independent nonces; **tests have not run**. Desktop keyring lock/unlock, concurrent initialization, real sync identity persistence and vault recovery remain unverified.
- Dependencies already include `chacha20poly1305`, `rand` and `zeroize`; install `libsecret-tools` as a Linux runtime requirement. No plaintext fallback is permitted.
- **Security limitation:** `secret-tool` is a subprocess and may require an unlocked interactive desktop keyring; an in-process libsecret backend is preferable if practical. Do not ship before an external security review and production runtime validation.

## P3.1 selective clear implementation (source-level)

- Wry's existing WebKitGTK `WebsiteDataManager::clear(ALL)` call previously discarded the asynchronous completion callback and returned immediate success.
- The Linux implementation now processes GTK events until the callback returns a result, with a 15-second timeout. This introduces a short bounded nested GTK event pump; runtime reentrancy and shutdown behaviour require a dedicated GUI regression test.
- `tauri-runtime-wry` now sends a response channel for `ClearAllBrowsingData` and the Rust command returns the reported result.
- **Do not mark P3.1 accepted until native tests prove completion, persistence, workspace separation, absence of UI reentrancy regressions, and error reporting.**

## Native Linux workspace storage isolation (P3.1, partial)

- Audit found the existing `with_profile_name()` is **Windows-only**: the vendored Tauri source explicitly documents Linux as unsupported/ignored. Consequently Linux tabs otherwise shared the same WebKitGTK context.
- `create_tab` and `open_glance` now pass a deterministic **relative per-workspace `data_directory`** on Linux, keyed by SHA-256 of the workspace profile name. Wry already keys its WebContext cache by the data-directory path; WebKitGTK configures an independent WebsiteDataManager and cookie store for each context.
- Unit tests define stable mapping, distinct workspaces and traversal-proof relative paths. **Not executed; real cookie/cache/localStorage isolation still requires two-workspace runtime fixtures.**
- `clear_workspace_data` now uses Tauri's existing `clear_all_browsing_data` on Linux, with a modified Wry WebKitGTK backend that observes the asynchronous completion callback. The Tauri runtime propagates the result to the caller instead of returning after the queued message. A bounded timeout reports failure.
- **Source implemented, not accepted:** real WebKitGTK clearing of cookies/cache/localStorage and sibling-workspace preservation is not yet verified on Linux; errors and timing need real tests.

## Native WebKitGTK permission boundary (P3.3, partial)

- The vendored Wry WebKitGTK backend now attaches `connect_permission_request` on Linux, explicitly calls `PermissionRequestExt::deny()` and returns `true` to consume the signal. This applies to app and child views built via the backend, including popups created through the same path.
- The implementation deliberately **does not grant permissions**: origin-scoped prompts, allow-once/remember decisions, persisted revocations and frontend explanatory UX are still unimplemented.
- Linux runtime permission tests, including unexpected WebKit permission subclasses and desktop portal behaviour, remain OPEN. This is a source-level fail-closed cut only.

## P2 policy extraction in this branch

- `blocker::evaluate_request` computes a pure policy decision from the existing adblock engine (block, filter rule, URL rewrite or redirect). Both Tauri document navigation and the Windows WebView2 resource callback use the same function.
- Rust tests cover blocking, allowed resources, tracking URL rewriting and malformed input. **Tests have not executed yet: P0 green compilation and Windows parity are still open.**
- Crucially, this does **not** intercept WebKitGTK subresource requests or implement Linux browser DNS enforcement. P2.2/P2.4 remain OPEN.

## P0 implementation in this branch

- `.github/workflows/linux-acceptance.yml`: Ubuntu 24.04 frontend and Rust jobs for locked dependency compilation and tests. **Defined; not accepted until a green run is linked to a commit.**
- `scripts/linux/preflight.sh`: local Linux environment receipt and sequential compile/test checks (terminates on failure).
- `zoom_tab` now uses Tauri's portable `set_zoom`; `toggle_devtools` invokes the native WebKit inspector in debug builds and reports unsupported release behaviour. **Source implemented; runtime verification open.**

## Next maximum-cut tasks

1. Run P0 on a real Linux machine; collect compiler output, do not invent further errors.
2. Extract `Webview2` COM request rules from `src-tauri/src/lib.rs` lines ~717–1700 into a pure Rust policy, tested without webview.
3. Prototype WebKitGTK's actual supported intercept hooks, verifying whether a blocked subresource reaches the local fixture server. If interception is not complete, explicitly choose an enforceable design instead of JS-only simulation.
4. Implement secure Linux identity/vault wrapping. Keep current fail-closed behavior until backed by tests.
5. Retain the existing upstream Windows behavior and contributor attribution.

## Non-goals

No UI framework rewrite, engine replacement without evidence, simulated network filtering counted as enforcement, or auto-merging into the original contributor branch without consent/permissions.

## P2.2 dedicated Linux cosmetic IPC (source implemented)

- Register a separate WebKit `bushidoCosmetic` channel for ordinary tabs, without remote Tauri capabilities. Scope Wry's standard handler to `ipc` only. Reuse the original Rust selector engine with current URL exceptions, generic-hide policy, whitelist and adblock preference. Bound probe size, token counts and rate. Windows retains its original message transport. Registration occurs after view creation; initial-probe timing, frame provenance, native compilation and dynamic release preferences remain open.

## P2.2 cosmetic transport batching

Bound observer tracking to 16,384 distinct tokens per kind per document, send at most 64 classes and 64 IDs per batch, bound serialized UTF-8 size, drain batches at the existing 500 ms cadence, and retain pending tokens when the bridge is not available. Linux receiver rejects inconsistent document URLs (not authenticated frame provenance). Native runtime acceptance remains open.

## P4 Linux media telemetry bridge (source implemented)

- The original media listener now sends bounded playback metadata via dedicated `bushidoMedia` WebKit channel, preserving Windows transport. Native receiver validates current URL, event kind, state, finite nonnegative numbers and bounded text; derives tab ID locally and emits existing `tab-media-state`. No artwork URLs, privileged shortcut actions, vault or general remote Tauri permissions are introduced.
- Late bridge does not consume the listener's deduplication state. Rate limiting can discard rapid transitions; actual playback telemetry, frame provenance and native compilation remain open. Shortcut actions require a trusted native key-event path, not arbitrary website messages.

## P4 focused native GTK keyboard shortcuts (source implemented)

- Register focused GTK key press/release/focus-out handlers on main, tab and Glance WebViews. Resolve GDK names through the original normalized keybinding map, dispatch only known application actions, consume matched presses, and clear held keys on focus loss. Linux no longer registers duplicate desktop-wide hotkeys or injects the Windows shortcut bridge into ordinary tabs/Glance. Rebinding updates the same map without global registration. Windows path is retained.
- GTK and JavaScriptCore direct dependencies are pinned to existing lockfile versions; fix owned WebView downgrade and use GTK gio/glib types in existing native IPC. Key mapping is intentionally bounded; shifted digits, arbitrary custom keys, main-window focus/propagation, auto-repeat and Wayland/plugin initialization require native tests. Not a global-shortcut parity claim.

- Focused shortcut mapping additionally rejects bare letters and shift-only text entry, even if a custom map contains such a binding. The native key handler still needs real GTK focus and propagation acceptance.

## P4 video presence and standard PiP telemetry (source implemented)

- Reuse media channel for bounded video snapshots with independent rate limit and native tab identity. Periodic reports include document video presence, standard PiP API support and actual active element state. Existing Windows event carries optional fields too. Frontend projects only selected-tab snapshots, clears on navigation, and requests initial detection immediately. PiP command checks API and respects disablePictureInPicture; confirmation requires trusted click in the page overlay. No claimed OS-level detached window fallback or WebKit PiP runtime support.

- Video snapshot projection ignores suspended/destroyed or internal active tabs, prunes inactive materializations, and deduplicates identical periodic snapshots to avoid needless React renders. This is UI state management, not authenticated video detection.

## P4 FindBar Linux result transport (source implemented)

- Linux find-in-page now returns a bounded count directly from the WebKit JavaScript evaluation callback, using a five-second timeout and typed result. No new remote message handler. The existing search behavior (`window.find`) and visible-text substring estimate are retained; the count is not necessarily identical to actual find results across frames, Unicode, or hidden DOM.
- FindBar correlates replies to its initiating request, ignores obsolete queries/tab switches, and clears old counts. Windows keeps its result-event path with request correlation. Native WebKit runtime and exact match/index parity remain open.

## P4 Linux Glance entry point (source implemented)

- Add a WebKit native link context-menu item `Open in Glance` for ordinary browser views. The browser derives its target from the native hit test, restricts it to HTTP(S) without URL credentials, requires native menu action activation, and rechecks the originating view URI and live tab registration. Emits the existing `glance-request` event with native tab ID; existing workspace/filter construction is reused.
- Do not inject the Windows-only Glance click interceptor on Linux, which previously consumed link clicks before trying unavailable WebView2 IPC. Linux Alt-click, pinned-tab previews, promotion-specific handlers and actual native context-menu/runtime acceptance remain OPEN. Context-menu entry is a deliberate alternative rather than exact input parity.

## P4 Native WebKit download lifecycle (source implemented)

- Linux ordinary tabs and Glance attach a native download listener scoped to the initiating WebView, preserving WebKit's original authenticated transfer instead of replaying a GET or exporting cookies. Native download records use the existing panel events, report progress/finish/failure and support native cancellation. Disconnect view-scoped handler on destruction. Use configured download/MIME routing and unique private per-transfer subdirectories with sanitized names and overwrite disabled. Failed/cancelled transfers clean their own directory; removing a completed record preserves its file.
- Native transfers deliberately disable pause/resume/retry; original HTTP-manager controls remain for its own transfers. Native rate limits, flat destination layout, ask-location flow, persistence, download start timing, shutdown and actual GTK integration remain open. No CI/native runtime acceptance claimed.

- Native completion wins a race with cancellation intent when WebKit reports successful finish and the destination exists. Filename length is capped in UTF-8 bytes; content length converts explicitly to the UI's unsigned byte count. Native cancellation command acknowledges dispatch, not the terminal signal; the panel's terminal event remains authoritative.

## P4.1 Native flat-directory publication (source implemented)
- Preserve the original WebKit transfer in a private hidden staging directory under the existing configured/MIME-routed destination. At successful finish, atomically hard-link the completed regular file to the flat destination, retrying numbered names only on AlreadyExists (including dangling symlinks). No exists/rename race and no overwrite or replay. Remove staging only after successful publication; preserve a completed staged file and show its warning in the download panel if publication fails. Open file/folder uses the actual retained path.
- Bound collision attempts; reject non-UTF-8 roots under the existing string-based path contract. Filesystems without hard-link support retain staging instead of attempting an unsafe overwrite/copy fallback. Added three Rust filesystem tests (not executed here); local Linux kernel primitive probes passed for 24 concurrent same-name transfers, dangling-symlink exclusion and failed-publication preservation. These are not Rust or WebKit runtime receipts.
- Ask-location dialog, hard-link-unsupported filesystem native acceptance, staging cleanup/recovery across restart, native pause/resume/rate limits and native integration remain OPEN. No CI work.

## P4.1 Native ask-location flow (source implemented)
- Native downloads honor the existing askDownloadLocation preference. On WebKitGTK >= 2.40 a GTK FileChooserNative Save dialog supplies a local absolute path and explicit filename; configured/MIME-routed root is the initial suggestion, not a forced destination. Keep existing files via the same no-replace publication policy (numbered name on collision, including an explicitly selected existing name). No new page IPC or request replay.
- No recursive GTK main loop: store one chooser per transfer, return from decide-destination and consume the matching response once. Cancellation or terminal completion removes/hides the chooser; duplicate/stale destination callbacks cannot revive it. Older WebKitGTK fails explicitly for ask-location mode rather than choosing a default silently; automatic downloads retain their prior minimum version. Two pure Rust regression tests added for the version and selected-path contracts; not executed. Native dialog concurrency, Wayland/X11/portal behavior, actual WebKit callback and cancellation ordering remain OPEN.

## P4 Linux Glance Alt-click navigation-policy hook (source implemented)
- Wry checks a typed per-native-view callback before its existing decide-policy handler would consume the signal. Callback is GTK-thread scoped, removed on widget destruction, and not exposed through remote-page IPC. Native WebKit link navigation requires a user gesture, non-redirect, primary button, Alt without conflicting modifiers, a focused live source and credential-free HTTP(S) URLs. On successful event dispatch Wry ignores the source navigation. Existing Glance destination workspace/filter construction is reused. Invalidate previous source navigation retry authority in debug native-filter mode; in-flight filter mutation is a separate gate.
- Context-menu validation shares the URL predicate. Frontend rejects missing/suspended/inactive-workspace sources and stale source URI, invokes creation once outside a state updater, isolates late failures by ID, and closes orphan late-created views. Existing Wry 0.54.1 patch exposed as a direct Linux-only dependency; lock edge updated, no new registry version.
- Two Rust regression tests added for gesture and URL policies; not executed. This targets WebKit NavigationAction only; target=_blank/new-window, iframe and JS-cancelled links, OS Alt-click interception, active-preview duplicate clicks, promoted-view hook installation, pin restore/auto-preview, native compilation and actual Wayland/X11 behavior remain OPEN. No CI work.

## P4 Linux native pinned-tab link previews (source implemented)
- Linux now has a separate native pin set linked to the live WebviewState tab registry, with consistent lock order plus explicit creation/close cleanup. Native set_tab_pinned updates this state rather than writing a page-controlled window variable. Suspended-but-live views keep their pin; destroyed/internal views restore from the existing session/UI flag through optional create_tab pinned argument. All six existing state-bearing create_tab invocation sites pass their pin (modern/legacy session, crash, destroyed resume, internal-page navigation, settings recreation). Fresh tabs and panels default unpinned. Pin UI dispatch is outside the state updater.
- The typed Wry native navigation hook now uses the original Glance listener's hostname comparison: direct primary external-host links auto-preview on pinned tabs, same hostname (including different port/scheme) stays normal unless Alt is held, unpinned regular clicks stay normal. Native user gesture, focus, no redirect, URL validation and conflicting modifier guards remain. Ordinary web panels do not receive the preview hooks. Existing Windows pin bridge is unchanged.
- Added two pure Rust regression tests for pin lifetime and hostname/gesture rules (not executed). Native compile, real native callbacks, rapid toggle versus recreation/close ordering, inactive workspace and hidden focus races, promoted/preloaded-view hook inheritance, target=_blank and iframe behavior remain OPEN. No CI work.

## P4 promoted Glance native feature inheritance (partial source)
- Glance captures the originating workspace in the frontend and native GlanceLifecycle map; open/close/promote are serialized with one async mutex. Promotion requires a live preview, native view and matching workspace, consumes the preview record, and reuses the same WebView. Late close_glance is a no-op after promotion, so it cannot destroy a promoted normal tab. Frontend waits for creation and promotion acknowledgements, rejects stale source/workspace, prevents duplicate promotion and disposes a late-promoted view with close_tab if the frontend closed it in the meantime. Explicit workspace switch dismisses the old preview.
- Install shared Linux media receiver, title/load callbacks and ordinary-tab Glance menu/input hooks at preview creation. Menu/input hooks are inactive until ordinary tab membership; downloads, shortcuts and filter callback remain the existing single registrations. Media receiver extracted from ordinary-tab path without changing message policy. No new remote IPC privileges. Frontend preserves latest preview URL/title and originating workspace on promotion.
- Split continues the original close-and-recreate behavior, guarded by active source workspace. Remaining OPEN: full ordinary-tab cosmetic/scriptlet/security setting parity, promoted media snapshot timing, native handler acknowledgements, actual GTK ordering/visibility, creation/close failure recovery, workspace changes through other UI paths, and preloaded views. Source review only; no native compile/CI claimed.

## P4 Native Linux FindController (source implemented)
- Replace Linux document-text estimation/window.find with native WebKit FindController search, search_next, search_previous and search_finish, using case-insensitive wrapped search. Native found/failed signals provide match counts, including explicit G_MAXUINT overflow representation. No remote page IPC or fake native match ordinal (UI displays total matches for native results; Windows count-event path retained).
- One active native operation per actual WebView, monotonic request IDs, permanent per-view signal handlers, clear/navigation/destroy invalidation and timeout quarantine. Clear drops the pending reply but does not reuse its slot before the untagged WebKit signal drains; timeout refuses another search for that view rather than guessing callback attribution. Frontend clears on query change/unmount/tab switch, ignores stale replies, guards duplicate search and clears displayed count on navigation. Two Rust state regression tests added but not executed. Native callback ordering, WebKit search semantics, timeout recovery/recreation, same-document URL transitions, rapid typing and Rust/GTK compilation remain OPEN. No CI work.

### FindController count-signal correction
WebKit bug [325421](https://bugs.webkit.org/show_bug.cgi?id=325421) documents older next/previous found-text counts collapsing to 1. Treat found-text as traversal success only, then perform explicit count_matches and accept counted-matches only in the counting phase. A failed traversal returns zero. Keep the same request slot across both phases and drain the matching signal when cancelled. Expired nonempty dispatches are ignored; frontend listeners reject after teardown. Actual dynamic-DOM count and highlight behavior require native testing.

## P4 Linux PiP capability boundary (partial)
- Upstream WebKit generic build options default PICTURE_IN_PICTURE_API and VIDEO_PRESENTATION_MODE off; GTK port options do not establish native detached video support. Do not re-request media URLs or label page overlay as native PiP. New Linux-only linux_pip.js uses browser presentation API only when available, selects loaded eligible connected video, requires real user activation and rechecks it immediately before request; repeated toggle dismisses, stale/untrusted activation rejected, abandoned prompt expires. Native JS evaluation result distinguishes unsupported/no eligible video from accepted gesture prompt; telemetry remains document-confirmed, never optimistic active state. Active exit permitted even if entry support changes. Windows command path retained; shared media detection now requires loaded video dimensions.
- Eight mocked Node policy cases added; actual detached PiP, native compositor and engine support remain OPEN. No custom media extraction or separate playback implementation.
- Added MCP_AUTOMATION.md: external Rust WebDriver/MCP reuse decision and staged A0-A4 local acceptance plan; no new dependency, embedded listener or CI changes this round.

### PiP local test receipt and pending request guard
Local Linux Node v22.16.0 mocked DOM run: 9/9 passed (128.26 ms test runner total); tested Git blobs linux_pip.js 1923ffcd772640a5b5aadcc58827cfd870e2fbd5 and test-pip-policy.cjs d1cbd1491d3f4397de25c12c4a98c0aa4a45357e. Guard repeated toggles and 15s prompt timeout while an accepted trusted activation still awaits engine promise; do not permit another entry request through that same live prompt. No claim against hostile page mutation or unresolved promises; navigation can reset the document. Added the mock suite to existing local preflight, not hosted CI. Cargo/rustc and WebKitGTK development packages unavailable in execution host; native acceptance remains OPEN.

## A0–A4 Rust MCP / local native acceptance — reuse options

Maintain these as explicit options, not mandatory new dependencies. For detailed architecture and benchmarks see [MCP_AUTOMATION.md](MCP_AUTOMATION.md).

| Option | Role | Selection gate |
| --- | --- | --- |
| [Tauri Driver](https://github.com/tauri-apps/tauri/tree/dev/crates/tauri-driver) + WebKitWebDriver | Official external native Tauri driver | First executable spike; real Bushido child WebViews |
| [Bruster](https://github.com/aliilyas/bruster) | Rust native browser, WebKitGTK/WebView2/WKWebView, MCP and JSON-lines | Strong reference/reuse candidate; isolate driver layer, do not substitute its browser |
| [Rust Browser MCP](https://github.com/EmilLindfors/rust-browser-mcp) | Existing Rust W3C WebDriver MCP implementation | Evaluate Tauri session launcher compatibility |
| [rmcp](https://github.com/modelcontextprotocol/rust-sdk) | Official Rust MCP SDK | Thin stdio adapter if existing servers cannot be reused |
| [Fantoccini](https://github.com/jonhoo/fantoccini) | Rust W3C WebDriver client | Reuse transport with rmcp if needed |
| [tauri-webdriver](https://docs.rs/crate/tauri-webdriver/latest) | Embedded driver fallback | Dev-only, only if external child-view targeting fails; security review |

| Gate | Deliverable | State |
| --- | --- | --- |
| A0 | Reuse options, compact scenario contract, native test boundary | DOCUMENTED |
| A1 | Local real Bushido driver, isolated profile, child-view addressing | DEFERRED until native build |
| A2 | Reuse/adapter selection and separate pinned Rust dependency graph | DEFERRED |
| A3 | Bounded semantic snapshots, typed batched actions, deterministic receipts, call/bytes/time/RSS benchmark | DEFERRED |
| A4 | Hosted CI integration after local acceptance | DEFERRED |

Prioritize implementation now; local policy tests permitted. No hosted CI work, production MCP listener, or Chromium surrogate for actual WebKitGTK acceptance. Prefer scenario execution over individual agent tool calls. Do not claim efficiency savings until measured.

## P3/P4 Linux explicit native saved-login fill (partial source)
- Remove legacy page vault script and unlock-triggered broadcast on Linux. Add ordinary-tab native context-menu action when autofill is enabled. Require actual loaded HTTPS URI with clean TLS flags, exact origin lookup including port, live tab outside panel, native account selection, isolated script world and per-document one-use ticket. Probe only one empty visible username/password pair in an unambiguous same-origin POST form. Revalidate URI, navigation generation, policy, lock, ticket and same form before filling. No auto-submit, page-supplied hostname, bulk password broadcast, or generic remote IPC. Read metadata first, decrypt only selected account using existing vault on worker, zeroize owned password/script buffers on drop. Existing vault DB is global; explicit user-approved fill is not a workspace-partitioned vault. Store exact HTTPS origin in existing domain field; no hostname fallback or silent migration. Automatic save, lock-versus-dispatch atomic revocation, native script/credential memory lifetime, preloaded/promoted Glance, cross-frame and multi-step login, native build/runtime, true workspace-specific vault schema and UI migration remain OPEN. Windows legacy path unchanged.

### Native vault provisioning and local policy receipt
- Existing Passwords settings lacked an Add action. Added explicit trusted-chrome HTTPS login provisioning; Rust normalizes Linux key to exact origin and preserves Windows hostname semantics. No automatic migration of legacy rows. Password form resets after completion/failure; native fill status reaches browser chrome. View destruction invalidates the document generation. Existing Settings list still uses its privileged bulk-decrypt API; reduce that exposure and address global vault workspace partitioning/metadata authentication in subsequent work.
- Local Node v22.16.0 mock DOM suite: 6/6 passed, 164.52 ms total. Source blob 8bc8a5acfd4fb93b6635118caa47540cbbc9c983 and test blob 0a5cf55dbd2757cdccdb00b24544a3c19f247086; fixtures cover ticket consumption, wrong ticket, origin/port, HTTP, frame, insecure context, cross-origin/credential action, GET, target/submit overrides, ambiguous or invisible/disabled/new-password/occupied inputs and document/form mutation. Added to local preflight. This tests isolated-world script logic in a mock, not native WebKit isolation or TLS, GTK chooser, credential store or compiled Rust. Cargo/GTK/WebKit dev packages absent.

### P3/P4 vault lifetime and Settings plaintext minimization (partial source)
- Upstream Windows autofill plaintext-delivery trust-boundary concern confirmed by source review at upstream 2cb6468a. Upstream SECURITY.md requests issues; submission blocked by connector HTTP 403. PR body records restrained security note; no exploit/compromise claim.
- Add a vault session epoch, advanced under the key mutex on setup/unlock/lock. Linux native fill captures epoch before probe, checks after chooser and decrypt, and admits synchronous WebKit isolated-world dispatch under the same lock. Lock/reunlock invalidates older authorizations; no mutex held across await. Already-dispatched engine work cannot be revoked by this design. Native timeouts remain indeterminate. Zeroize key copies and master-password input buffers; transactional initial setup refuses an existing vault rather than replacing hash/salt.
- Settings list now returns only id/domain/username (no encrypted columns queried or plaintext serialized); explicit selected reveal and native clipboard copy commands replace bulk decrypt in trusted Settings on both platforms. One revealed secret at a time, 15s expiry, lock event, navigation/teardown and request invalidation. Clipboard contents and already-returned JS strings cannot be reliably erased; Windows page-autofill bulk helper remains unchanged pending separate upstream fix. Existing privileged IPC boundary still needs native acceptance.
- Added two Rust regressions for old epoch rejection and metadata-only SQLite query (not executed). Workspace-partitioned schema and authenticated metadata/AAD, actual revocation after engine queue, concurrent unlock/lock ordering, auto-save and promoted/preloaded views remain OPEN. No native compile or hosted CI.

#### Lock-versus-unlock admission and session UI receipt
- Compare captured generation before expensive Argon2 work against the current key generation at installation; newer lock/unlock wins. Initial setup commits its two DB metadata rows inside successful transition admission; stale or failed transition cannot install key. Third Rust regression covers stale lock, failed commit and duplicate install, not executed.
- Replace lock-only event with epoch-tagged vault-state-changed plus state snapshot; Settings and App consume monotonic state, reset revealed passwords and update global auto-lock state. Correct existing modal flow that previously proceeded after a rejected unlock/setup. These source changes apply to both platforms. UI event subscription timing and native queued dispatch remain acceptance gates.
- Locally executed SQLite 3 via Python in-memory against the committed metadata query and equivalent two-row transaction with synthetic data: metadata query checks 4/4 PASS (no password columns in fixture, all/exact/port mismatch and column shape); setup transaction checks 4/4 PASS (initial absence, rollback, committed presence, two-row count). These are SQL smoke checks, not Rust integration, encryption or native runtime tests.

### P3/P4 versioned Linux workspace credential records and authenticated routing (partial source)
- Separate additive vault_scoped_v2 table; no alteration or silent reassignment of legacy rows. New Linux manual entries bind the explicit workspace and canonical HTTPS origin. Windows legacy storage/autofill unchanged. Native lookup uses the actual creation profile captured by the WebView hook; no global/default/legacy fallback and no hook when profile is missing. Account selection rechecks the selected username before decryption.
- ChaCha20-Poly1305 AAD uses domain-separated v2 framing with length-prefixed UTF-8 entry ID, workspace, origin and username; existing platform wrapping retained. Altered routing/selection metadata causes authentication failure on decrypt; plaintext metadata listings remain unauthenticated until selected, and timestamps are not authenticated. New table has no mutable credential update route; legacy update does not reach scoped records. Delete/reveal/copy/list are explicitly scoped; legacy Settings view is separate manual recovery (reveal/copy/delete), not a migration or autofill entitlement. Master key and lock remain global; trusted chrome can explicitly access legacy entries. No protection claimed against compromised trusted chrome or database deletion/rollback.
- New workspace IDs use UUIDs; existing persisted IDs retained. Prevent new sequential ID reuse and reject popup without an originating workspace. Settings keyed by workspace to discard stale view state. Two Rust tests for AAD mutation and SQLite scope isolation added, not executed. Existing historical numeric workspace identity reuse before this change cannot be retroactively repaired. Session restore/backups and orphan workspace recovery, full native compile/acceptance, legacy explicit migration, auto-save and promoted/preloaded Glance remain OPEN. No hosted CI.

#### Scoped-vault follow-through and local reference receipt
- Linux legacy write/update commands now fail explicitly; existing rows remain read/copy/delete recovery only. Settings scope generation guards stale list/reveal/delete results and refreshes triggered by completed old-scope operations, including switching away and back. No implicit conversion.
- Local independent Python reference check: cryptography 46.0.4 (ChaCha20-Poly1305), SQLite 3.46.1, using the v2 domain separator and length-prefixed field framing plus committed SQL schema/queries. 16/16 PASS: round-trip, each of four AAD field mutations, framing ambiguity, ciphertext modification, workspace list, exact origin, different workspace/port, absent workspace, legacy preservation, wrong-scope selection/deletion, nonce-length constraint. Does not execute Rust or platform wrapping. Rust/cargo and GTK/WebKit dev packages unavailable. Actual native acceptance, explicit migration/recovery of orphan scopes, post-dispatch revocation and authenticated metadata-at-list-time remain OPEN.

### P3/P4 submission-triggered native save consent (partial source)
- Ordinary Linux views with explicit creation profile and password-save enabled install a per-view random message channel in a named isolated WebKit world. Arm a fresh document ticket on finished HTTPS load with clean TLS; invalidate on load start/destroy. Require top frame, trusted password input and recent trusted form submit gesture, one unambiguous same-origin HTTPS POST pair, no alternate submit action. No beforeunload, bulk credentials, click-anywhere password harvesting or default-world IPC. Notice contains ticket/href/sequence/username only; a bounded candidate password snapshot is retained in isolated JS memory for at most 20 seconds (subject to engine timer scheduling), discarded on input/pagehide/consumption. JS strings cannot be guaranteed zeroized. User-initiated submission is not proof of login success or that a page did not mutate values.
- Native validates actual live URI/TLS, focus, profile and document ticket, monotonic sequence, policy and unlocked epoch. Native GTK approval is required before one isolated-world read; same document, form, fields, action and values must remain. Prompt expires/cancels when context or session invalidates. Save uses existing v2 AAD/platform wrapping, original epoch and expiry/policy/navigation checks at admission; insert-only, no existing account overwrite. A queued native read or admitted SQLite write cannot be guaranteed retractable. No full-page navigation/redirect capture, locked-vault prompt, signup/password-change, automatic update, Glance/preload capture or Windows change. Conservative same-document submission only; actual GTK/WebKit bridge and real login acceptance OPEN.
- Executed Node v22.16.0 mocked DOM tests 8/8 PASS (233.50 ms); tested blobs linux_vault_save.js 3e548709110b472c317f0843db9997af1c58869a and test-vault-save.cjs 619bb407963703215f3906e2a0eb415113fea3ab. Added suite to local preflight. No Rust/GTK build available, no hosted CI.

#### Save consent expiry/discard follow-through
- Explicit candidate deadline checked on read even when JS timers are throttled; trusted-input evidence consumed on submission. Matching candidate discard is separate from secret read and used on denied/invalid consent, including locked vault and busy view. Disconnect isolated world signal on destruction. JS engine memory retention still cannot be guaranteed; native cancellation cannot recall a queued read or committed write.
- Superseding Node v22.16.0 regression receipt: 10/10 PASS, 158.83 ms; exact blobs linux_vault_save.js 295fea69b661880340d486d17dc43640fa2fd4c5 and test-vault-save.cjs 90e507b9d175eb94d4906de97128630e286b07ba. SQLite 3.46.1 local exact insert SQL smoke 7/7 PASS (insert, duplicate no-op, original ciphertext unchanged, different workspace/port/user, explicit manual duplicate allowed). No native compile/runtime or concurrent DB execution claimed.

#### GLib executor compatibility
Native vault save dialog polling/read deadline and prior fill callback deadline now use futures-util select with GLib timeout futures on the GTK main context instead of Tokio timer calls. Existing futures-util dependency reused; no Cargo dependency change. This removes an implicit assumption that GLib-spawned work is entered into a Tokio timer runtime; actual timer/cancellation semantics and Rust compilation still OPEN.

### P4 Glance promotion security/credential inheritance (partial source)
- Share ordinary-tab document security script (service worker/font enumeration policies), native cosmetic receiver, and native credential hook installer. Glance gets HTTPS-only initial upgrade and navigation denial, HTTP(S)-only destinations without embedded credentials, existing document filter enforcement, native filter retry invalidation even on denied navigation, ordinary WebRTC privacy/preamble initialization and destination-specific cosmetic/cookie handling at completed load. Frontend passes current settings at creation rather than silently defaulting to a weaker preview. Glance cosmetic processing at finished load is later than ordinary tab processing; not timing parity.
- Lifecycle record retains originating workspace and credential-disable flags; preview never installs Linux credential hooks. Promotion validates exact workspace/preview ownership, registers the same view, removes panel membership, schedules shared credential hooks once and consumes record. Existing fill/save live checks and scoped v2 lookup apply; no reload or password replay. Hook scheduling does not acknowledge native installation, settings enabling after creation may require recreation, and native callback ordering remains OPEN. Windows legacy vault path unchanged; full native network/popup/devtools/preload/permission parity remains OPEN. No hosted CI.

#### Glance destination and failure handling follow-through
- Shared pure initial URL and navigation guard rejects non-HTTP(S)/userinfo and applies HTTPS-only policy consistently; three Rust tests added (initial upgrade vs later denial, unsafe schemes, independent hardening flags). Restore the latest workspace layout on failed Glance creation via a current callback ref, rather than leaving its native tabs hidden. Existing filter begin_navigation removes pending retry before policy rejection; no negative retry fixture executed natively.
- Re-ran existing exact vault save JavaScript suite locally: Node v22.16.0, 10/10 PASS, 180.70 ms; blobs 295fea69b661880340d486d17dc43640fa2fd4c5 and 90e507b9d175eb94d4906de97128630e286b07ba unchanged. This is a regression of the reused script only, not a Glance integration test. Rust/cargo and GTK/WebKit dev packages still unavailable. No hosted CI.

### P4 native download history / restart reconciliation (partial source)
- Separate private native-download-history SQLite DB, FULL synchronous transactions, one redacted metadata row per native UUID; no request URL, cookie, request body, headers or replay manifest. Persist start and chosen staging destination before allowing native write; bounded 5s progress checkpoints and durable failure/completion. If journal initialization/destination receipt fails cancel rather than continue untracked. Error details redacted on disk. Native resume/retry remain unsupported; startup maps interrupted records to failed with retained partial evidence, never infers completion from partial size. Completed receipts recheck regular file and recorded size (not cryptographic identity). No startup filesystem mutations or network replay.
- Completed staged file synced and recorded before flat publication; record final path before staging unlink, sync publication parent before cleanup. Crash between hard link and receipt can leave two names; prior staged receipt remains valid. Journal failure preserves staging. Runtime failure/cancel only removes empty owned directories, not recursive partial contents; explicit history clear/remove never deletes files. Actual power-loss ordering, multi-process ownership, storage failure, pending orphan discovery and secure deletion remain OPEN.
- Backend clear-completed command removes durable native records and memory entries without deleting files; React consumes cancellation events and does not replay native retry. Startup merges current events with loaded records (cancellation-vs-snapshot race still open); native file open checks completion and size. Native priority changes not persisted yet. Three Rust journal tests added, unexecuted. Native compile/runtime and hosted CI deferred.

#### Native history completion and clear ordering
- Private staged completion receipt is not published as a UI terminal state. Sync data, staging directory and root, record staged completion, hard-link final name, sync final parent, record final path, then unlink staged name. If final receipt or directory sync fails retain staged fallback and any new link. Hold item lock through final receipt/cleanup/event, shared with clear-completed; no stale completion upsert after clear through this path. Startup snapshot waits for listener registration, excludes IDs touched by live events, and upserts missing progress events. Native runtime event ordering, multi-instance ownership and durable deletion of external files remain unverified. Synchronous SQLite and fsync on GTK thread can cause latency; measure before acceptance.
- Local independent Python / SQLite 3.46.1 reference fixture executed 11/11 PASS: interrupted record before finish, staged receipt reconnect, hard-link crash window, no overwrite, abrupt subprocess exit before receipt commit and rollback, final receipt before cleanup, final bytes after cleanup, durable clear without deleting file, private file mode, missing path and symlink regular-file check. No compiled Rust, no real download, no power-loss simulation. No hosted CI.

### P4 speculative preload trust boundary — fail-closed retirement and native ownership (partial source)
- Audit: Linux had no actual hover preload producer (legacy script posts to WebView2); Windows producer constructed a hidden document without source workspace profile or ordinary policies. Global URL-only promotion could consume another tab's candidate, block a different navigation and replace its semantics; delayed cancel could close an unrelated newer preload. Full document GET speculation can cause side effects and URL equality cannot establish method/history/request equivalence.
- Retire the legacy hidden-document producer, message cases, three independent global state slots and URL-only promotion on both platforms; stop injection and remove frontend promotion fallback. Existing ordinary navigation is not intercepted by preload. The original script path is retained as an inert provenance marker. This intentionally disables Windows hover preloading in this branch; no new Linux speculative network requests or feature parity claimed. Explicit Glance remains available.
- Native ownership registry binds ordinary constructed WebViews to validated explicit workspace IDs and fresh generations. Close removes ownership. Glance creation uses the actual source's native owner and committed URL, compares the frontend-supplied workspace/source URL, derives the storage profile from native owner, rechecks owner+URL after construction and at promotion. Promoted view gains its own ownership; Linux fill/save require matching native workspace, not merely tab presence. Side panels do not acquire ordinary ownership. Two Rust regression tests written; not executed. Native registration/close races and same-URL document reload, actual Tauri callback/runtime acceptance, and future safe speculative optimization remain OPEN. Do not reintroduce speculative full navigation until source-generation, exact request and history equivalence, policy-complete constructor, cancellation and explicit privacy acceptance exist. No hosted CI.

#### Native owner generation follow-through
- Serialize ordinary native create/close against Glance creation/promotion using the existing lifecycle mutex. Linux native Glance context/Alt-click producers capture the actual constructor owner and send its generation through trusted chrome; open_glance requires the matching generation on Linux, in addition to source/workspace/URL checks. Glance installs those producers only after promoted ownership is registered; credential fill/save capture exact owner generation, and save worker repeats owner comparison at its admission checks. This rejects old callbacks even when tab ID/workspace are reused. Windows legacy Glance producer still lacks generation provenance; same-URL document navigation and already-queued engine/storage work remain separate gates. No new dependency. No native execution claimed.

### P5.1 Debian package candidate and distribution boundary (partial source)
- Separate auto-merged platform bundle configuration: Windows NSIS/upstream updater retained in Windows overlay; Linux deb only, no updater artifacts. Linux compiled builder omits upstream binary updater and UI queries a native capability before check/install. Linux desktop launcher and icons, browser categories, bundled GPL text and explicit runtime dependencies including WebKitGTK >=2.40, libsecret-tools, xdg-utils, CA trust and OpenSSL. No MIME/scheme registration or external URL argument yet; default-browser integration remains OPEN. No signing/public distribution.
- Local build entry point scripts/linux/build-deb.sh: clean tree, pkg-config and tool checks, npm ci, pinned CLI 2.10.0 with Cargo --locked, isolated artifact directory, version and static archive validation, commit/toolchain/lock/config/artifact hash and ELF dynamic-section receipt. SOURCE_DATE_EPOCH supplied, reproducible bytes not claimed. Toolchain/system snapshot pinning, dependency closure, package installation/uninstallation, Windows regression, source-offer/transitive license audit and AppImage/RPM/Nix remain OPEN. See docs/linux/PACKAGING.md.
- Executed static package checker 9/9 synthetic .deb checks PASS on Debian 13 amd64 with Python/dpkg-deb; tested script blob 97694456e37fc024a9b686144a080767efd3b3c6. No Bushido binary compiled or package built; Rust/cargo, GTK/WebKitGTK dev packages unavailable. No hosted CI.

#### P5.1 packaging follow-through
- Static checker included in local preflight. Exact committed build script blob `0f2e8fb2986c6e56e003851bb73dab3a47cd9d2f` verified with `bash -n` PASS; actual local script invocation exited 2 on missing rustc before building, installing dependencies or generating an artifact. Debian 13.3 container, Node 22.16.0. Missing cargo and GTK/WebKitGTK/libsoup development pkg-config packages confirmed separately. No candidate .deb, no native runtime, no installation or reproducibility claim. Use a real Linux desktop build host for next P5.1 acceptance; remaining safe URL activation/MIME integration and dependency closure must precede default-browser registration.

### P5.1 external URL activation / early Linux single-instance ownership (partial source)
- Before DNS, logging, filter cache, profile writes or WebView construction, Linux takes a nonblocking kernel flock on a private owner-only file in the existing startup data root. Retain inode/descriptor for process lifetime. Secondary instance forwards a bounded JSON URL batch over owner-private Unix socket with SO_PEERCRED same-UID checks, framed size and timeout, bounded atomic in-memory queue; acknowledged queue admission is not navigation. No fallback second writer, automatic duplicate retry, arbitrary CLI flags or privileged schemes. Only lock owner removes a stale socket. Reuse libc 0.2.180 already pinned in Cargo.lock (new direct Linux edge, no lock version change).
- Main-WebView-label-only IPC drains pending activation, restores/focuses window, waits for onboarding/workspace readiness and asks explicit consent for the current named workspace; context changes cancel instead of silently routing elsewhere. Ordinary addTab constructor remains authoritative. No URL persistence or logging in activation layer. In-memory queue may lose pending requests on crash or UI context change; same-user local processes are inside this boundary, no protection against malicious same-UID profile mutation, socket path-length failure is fail-closed. XDG profile identity assumes same canonical data root; independent profiles remain independent. Windows unchanged. Three Rust regression tests added, unexecuted; real instance lifecycle, desktop dialog/polling and Tauri invocation OPEN. No MIME/default browser registration yet; no hosted CI.

#### Activation follow-through and executed local reference receipt
- External boundary requires explicit HTTP(S) authority, rejects shorthand, backslash repair and even empty userinfo syntax; same batch/frame limits for primary arguments and socket requests. Socket inode chmod 0600 in private 0700 parent, without changing process umask. Native capability disables frontend polling on non-Linux platforms; current addTab ref avoids cancelling accepted links solely because a same-workspace tab list changed. Workspace switching still cancels, and activation queue is volatile. Window confirm UI remains unverified in native WebKit.
- Executed independent Python Linux kernel reference 11/11 PASS: directory/lock modes, competing process denied, release permits next process, inode retained, SIGKILL releases lock, O_NOFOLLOW rejects symlink, SO_PEERCRED matches UID, framed payload, one-byte acknowledgement, stale Unix socket and removal before rebinding. Actual Rust implementation not executed; no runtime singleton acceptance, no MIME registration. Rust/cargo and GTK/WebKitGTK dev packages absent. No hosted CI.

### P0 local acceptance orchestration (source implementation)
- New `scripts/linux/accept-local.py`: independent developer fixtures run even when native prerequisites are missing; dependency-aware frontend and locked Cargo stages, exact checkout/lock digests, per-stage private logs and SHA-256, machine-readable receipt after each stage. Keeps actual build/test failures distinct from missing prerequisites and rejects a changed checkout. No GUI or privacy gate is accepted by this runner; no hosted CI, system package installation, or profile access. Native P0 and later runtime gates remain OPEN.

- Local execution on Debian 13: exact runner Git blob `f6f665926d16c7694c3f7396f9c944e6b652fce5` verified with git hash-object; 3/3 unittest methods PASS (actual subprocess exit handling, missing executable and unsuccessful probe classification, dependent skip, log digest, JSON and changed/dirty Git checkout). Independent synthetic end-to-end run exercised all 15 stages with fake tool commands and fixture scripts; native probes and Cargo dependent stages blocked while frontend and independent fixture stages continued, all recorded log hashes matched, receipt complete and exit 2. This is runner behavior evidence, not real frontend/fixture/native acceptance. Rust/cargo and GTK/WebKitGTK/libsoup development packages absent in this environment. Full checkout acceptance deferred to native host; no hosted CI.

### P5.1 exact-commit source candidate (partial source)
- Added local Git archive generator with full tracked-blob and executable-mode verification, lockfile dependency inventory, local fork license declarations, notice paths and filter/scriptlet hash/header evidence. Candidate publication explicitly false and corresponding-source review OPEN. Five synthetic Git regression tests written; native/user acceptance pending. External dependency sources, actual binary closure, third-party license texts and distribution review remain OPEN. See SOURCE_DISTRIBUTION.md. No hosted CI.

- Source candidate synthetic regression suite included in both local preflight and the consolidated receipt runner (now 16 stages). Prevent Python import cache from dirtying the checkout. Tests remain queued for user-side execution; no hosted CI.

### P4.2 native screenshot and image clipboard (partial source)
- Linux visible capture uses WebKitGTK's asynchronous native visible-region snapshot, GTK/Cairo/GdkPixbuf PNG conversion, mapped/live ordinary-view checks, native owner generation and load-start invalidation, cancellation and bounded reply. Viewport and returned image dimensions capped (16,777,216 pixels, 16,384 per side); PNG payload capped 64 MiB. Existing React Area mode crops the same frozen image. Full-page and direct native area commands explicitly unavailable on Linux pending bounded document capture and coordinate contract; full-page picker capability follows compiled backend. Frontend drops stale workspace/tab results and no longer hides views on failed capture. Linux image clipboard uses GTK selection owner rather than temporary arboard object; native save validates PNG/dimensions, single filename and exclusive collision-safe 0600 file creation; partial write may leave a file. No added dependency; two Rust tests written, unexecuted. Actual binding compilation, pixel scaling, UI ordering, clipboard persistence/Wayland/X11 and native acceptance OPEN. No hosted CI.

- Linux visible capture, PNG save and image clipboard commands require trusted main-WebView label (not shared window label). Frontend associates screenshot/annotation with original workspace, tab and URL and clears on context mismatch; same-URL reload after capture, focus/layout races, clipboard admission vs timeout and native lifecycle remain open. No native checks executed.

### P4.2 native print and inspector policy (partial source)
- Linux explicit browser print uses WebKit PrintOperation on the GTK thread with native parent window, live ordinary owner and visible/nonloading checks, and a process modal slot guarding nested main-loop reentrancy. No page JavaScript print substitution, silent print or automatic retry; result means dialog returned, not job completion. Linux DevTools is debug-only and depends on actual per-view developer-extras setting; constructor applies Security disable flag to normal tabs, denies panels and Glance (including same-view promotion until recreate), and Wry explicitly sets both true and false. Trusted main-WebView IPC and frontend error reporting across shortcuts/menu. Security settings still use existing reload-all workflow; no immediate native revocation, page-originated print signals and actual print job result are separate gates. Two Rust tests written, not run; compile/native acceptance OPEN. No hosted CI.

- Native print now returns distinct cancelled/submitted outcomes (neither is physical job completion); non-Linux legacy JS dispatch returns requested. Print and inspector recheck initial committed URL on GTK admission in addition to ownership. Once native modal begins, document changes and actual job completion are not controlled by this command. Same-URL reload, modal lifecycle, binding compilation and print failure/finished signals remain OPEN. No tests run.

### P3.3 / P0 zkSEC and zkperf reconciliation (partial source)
- Inspected pinned zkSEC and zkperf owners; use proposal-only trace, separate grounded mu_exec and capability authority, plus five-layer evidence chain. See ZKSEC_ZKPERF.md. Native Wry deny remains unconditional; generic permission-request has no verified requesting-frame origin in our implementation. Added opt-in debug-only saturating count of native deny calls and trusted chrome read command (no recording of origin/payload or production telemetry). No grant/remember or origin-scoped prompt yet. Five-layer explicit local evidence index hashes only named files, keeps missing layers and all grounding/acceptance OPEN; not a proof or verified binary/source linkage. Three Python tests and one Wry unit test written, not run. Local runner now 17 stages; no hosted CI. Native origin producer, actual trace/IR same-object adapter and performance measurements remain OPEN.

### P2.4 original resolver authority and wire integrity (partial source)
- Removed unauthenticated HTTP /level; trusted main-WebView in-process controller, saved initial level and monotone policy generation. Pin HTTPS bootstrap IPs, no ambient proxy/redirect, bounded DNS wire and matching transaction/question validation, case-insensitive tracker check. Unsafe raw packet cache disabled intentionally pending message-aware cache; latency tradeoff documented. Linux unused resolver disabled by default, debug fixture explicit; raw TCP preconnect disabled. Browser-wide WebKit DoH/egress remains OPEN, no production privacy claim. See DNS_CONTROL.md. Two Rust tests and native Python test adaptation written, not run. No new dependencies, no hosted CI.

- Native DNS backend status distinguishes configured-but-unverified Windows, fixture-only and unavailable; Security/General DNS preference no longer implies encrypted WebKit traffic. Added HTTP policy mutation negative control and GET transaction ID check; reduced repeat-query loop while cache is disabled. No execution; user native acceptance pending.

### A0.1 shared ITIR MCP observation route
- ITIR-suite PR #73 introduces read-only zkSEC policy preview, zkperf stream window planning and Bushido evidence-index self-hash checks. External local evidence client uses existing ITIR JSON bridge safe_call with fixed tool and proposal-only receipt validation; tests authored, not run. No new browser dependencies or embedded MCP. Local runner 18 stages. Rust sidecar live MCP/driver and same-object grounding remain OPEN. P2 native proxy/DoH binding still OPEN; this tranche advances acceptance-side integration, not network enforcement.

### P2.4d restricted positive DNS replay constructor
- Added debug-only opt-in exact-wire positive A/AAAA cache, transaction rebinding, conservative TTL aging, explicit HTTP freshness gating, primary-only storage, policy generation and bounded capacity. Default/release remains uncached. Upstream nonzero/ambiguous Age fails closed until general TTL aging. Four Rust regressions written, not run. Native proxy/egress and general cache admission remain OPEN.

- P2.4d/e follow-up: cache constructors recheck key/query identity, Linux loopback fixture Host and browser-origin metadata checks + no-store, 32 active resolver calls cap. No native authentication or proxy binding implied. One additional Rust negative control written, not run.

### P2.4e native proxy constructor sentinel (partial source)
- Existing Tauri -> runtime-wry -> WebKit WebsiteDataManager proxy path wired into tab/panel and Glance constructors for immutable debug opt-in numeric loopback endpoint with fresh private XDG roots and separate workspace namespace. Explicit proxy missing manager fails view construction. New non-forwarding local sentinel observes fixed HTTP document/subresource/frame/worker/redirect categories and rejects CONNECT; proposal-only aggregate receipts. No actual DoH routing, TLS tunnel or direct-egress closure. Four tests written, not run; 19-stage acceptance runner. See EGRESS_FIXTURE.md. Native compilation and runtime acceptance OPEN.

### P2.4e opaque HTTPS positive control and direct-address candidate
- Separate fixed-loopback CONNECT relay and synthetic TLS endpoint, exact authority, bounded relay, no arbitrary upstream or DNS. Original deny-only sentinel retained. Actual resolver private candidate producer reuses direct positive wire parser, binds exact query/response, primary upstream, policy epoch, expiry and independently approved address through numeric port-443 constructor; no dialing or IPC. Actual live resolver-to-transport weld, TLS/native trust and confinement OPEN. Five tests written, unexecuted; 20-stage local runner.

- P2.4e follow-up: direct-address candidate binds exact original resolver instance via Weak/Arc pointer equality, not merely generation equality. Cross-instance negative control added to existing test; TCP fixture reads segmentation-independent bytes. No test execution or live DNS/tunnel weld.

### P2.4e scoped connection receiver and policy cancellation
- Internal candidate-consuming test-only loopback transport admission with exact owner/epoch/hostname/IP binding, 8 slots, peer verification, bounded bidirectional opaque relay, per-poll policy check and watch cancellation with deadline. Non-test admits fail closed; no browser IPC, live proxy or trusted real destination producer. Three Rust tests authored, not run. P2.4e live DNS-to-transport scope and P2.4f kernel/network confinement OPEN.

- P2.4e same-constructor follow-up: real primary DoH path and synthetic loopback transport use identical private wire-to-candidate constructor; added mismatched transaction/expiry/stale-generation regression and bounded test waits. Four Rust tests in scoped receiver suite, not executed. Actual external admission remains fail closed.

### P2.4e explicit operator scope preview
- Debug fixture startup selects fixed synthetic loopback TLS scope with explicit workspace and origin port; native main-only proposal preview uses actual owner/URL and shared blocker, no whitelist override or token. Reject mismatch/rewrite/redirect; recheck owner/URL. Native document epoch and CONNECT-to-view/request attribution OPEN. Transport receiver still test-only; no live authority derived from preview. Three Rust tests authored, not run.

### P2.4e native top-level document epoch observation
- Debug-only scope observer uses native Started/Committed/Finished (not Tauri's committed-mapped Started), fresh UUID, owner and native object identity, failure/TLS/process/destroy/URI invalidation. Late attach requires complete subsequent load; close forgets; promoted Glance starts unknown. GTK-thread v2 scope preview rechecks same epoch and original policy, still proposal-only and document_epoch_verified=false. Three Rust tests authored, not run. Native lifecycle edge acceptance and request attribution OPEN.

### P2.4e native resource lifecycle observation
- v3 preview uses same native document epoch observer, resource-load-started and per-resource URI/failed/TLS/finished callbacks; fixed categories, bounded hooks and omission counts, old epoch excluded. No frame identity, dispatch/redirect proof or CONNECT binding; nullable sent-request argument deferred pending pinned binding verification. Three Rust tests authored, not run. Native coverage, attribution and live scope issuance OPEN.

- P2.4e matching independent HTTPS route matrix: original TLS fixture now has nine exact synthetic fixed routes with aggregate origin arrivals and missing positives; same category names as native resource v3 preview, no identity join asserted. One Python test authored, not run. No dependencies or runner stages added.

### P2.4e nullable sent-request and fixed redirect witness
- Native v4 resource observer uses GLib checked GValues with Option<URIResponse>, no generated nonnullable reference. Original object/epoch sequence and exact fixed 302 route edge; explicit malformed/unlinked/omission counters, 16 events per tracked resource. Three Rust tests authored, not run. Web extension pre-send API has page but no frame/type argument. Pinned native build, actual callback, arbitrary redirect/physical correlation and trusted extension admission OPEN.

### P2.4e debug web-process extension source and loader
- Original extensions_path chain reused; explicit private single ELF and SHA-256 check, optional separate clean source build receipt, isolated debug scope only. Native C page-created/send-request exact synthetic direct/redirect target cancellation; bounded fixed messages, independent A/B origin control routes and missing baseline rules. UI message receiver, extension build/loading and request enforcement acceptance OPEN; source/binary attestation and TOCTOU OPEN. One Rust test authored, not run.

- P2.4e v5 extension receiver: native WebView fixed-name user messages only, no parameters/FDs, 512 observations per view lifetime, separate from epoch attribution; owner/native view recheck, no binary linkage or execution authority. Two Rust tests authored, not run. Actual native build/load/message delivery and independent A/B enforcement OPEN.

### P2.4e original blocker finite fixture projection
- Original list compiler + two exact synthetic rules, same live fixture engine, no normal cache, freeze manual/background updates. 4 routes x 3 sources x 9 types -> A/D/X via original evaluate_request. Versioned WebKit GVariant init data via vendored constructor, first-view immutable guard, C consumes only fixed table, X/missing abstains. No universal frame/type equivalence or production enforcement. Three Rust tests authored, not run; C/native compile, delivery, runtime policy identity, dynamic updates and physical A/B OPEN.

### P2.4e fresh native policy challenge
- v2 immutable generation+table initialization; original WebView-to-WebPage request/reply with fresh nonce, actual parsed table and launch UUID, exact response/nonpayload-FD checks, native owner/document recheck, bounded single outstanding per view. No reusable authority or binary/application attestation. v7 scope receipt records matching reply only; four Rust tests authored, not run. Physical reply, process replacement, original enforcement and dynamic policy update OPEN.

### P2.4e applied policy prefix v8
- Same per-page native cancellation state records bounded 128 ordered fixed-route decisions and previous-response presence, saturating omission, page UUID. Fresh v2 challenge returns actual generation/table and prefix; Rust validates each row, explicit empty/truncated/malformed/mismatch. No join by aggregate counts, no document/frame/physical identity or execution authority. Two additional tests, three updated; none run. Native acceptance and independent A/B reconciliation OPEN.

### P2.4e strict offline A/B reconciliation
- Origin v3 fresh process UUID, fixed origin port and finalization flag; preview v9 includes actual operator origin port. Offline four-receipt consumer validates contracts, distinct runs, matching ports, ADAD and nonempty untruncated native prefix with all four controls and previous-response target, baseline reachability and enabled positive/negative controls. Consistency/contradiction/inconclusive only; no same-run or physical request proof. SHA-256 input commitments, private exclusive output. Five Python tests authored, added local runner stage; none run. ITIR typed joint receipt bridge and independent process/OS linkage OPEN.

### P2.4e ITIR-MCP typed candidate projection
- Adjacent ITIR-suite draft mcp-zksec-zkperf-observation-adapters now has itir.zkperf.bushido_ab_candidate, reusing original zkperf window_plan and observed-only authority profile. Bushido operator-run fixed-tool local stdio client validates exact safe_call abstention, supplied reconciliation canonical digest, one stream observation/window and explicit false authority flags; no arbitrary tool selection, extra network or browser IPC. Three Python tests authored; local runner now 22 stages. Native, exact same-run, independent OS, source/binary attestation and trusted promotion OPEN. No tests run.

### P2.4e independent live procfs socket witness (2026-11-23)
- Synthetic origin v3 adds producer-claimed proxy PID/start-ticks/network namespace/listening-socket inode and proxy port. Local explicit operator `proc-socket-witness.py` checks live PID identity twice, original listener inode in its fd set and /proc/net/tcp LISTEN port, browser parent and bounded descendant fd socket inodes with same namespace and an established TCP remote endpoint. No global process secrets/cmdline/environment collected; cannot infer request identity from TCP association. Browser children that use another network process, inaccessible procfs or quiescent connections produce inconclusive. Four authored tests and local self-test stage (23 total), not run. No OS sandbox, signed producer provenance, or native acceptance; the independent witness is an additional candidate layer, not authority.

### P2.4e live/final process provenance join
- Corrected TLS fixture socket inode extraction regex (previous over-escape blocked valid socket:[inode] links). New bounded `bind-proc-provenance.py` compares finalized enabled origin run+proxy port+PID startticks+namespace against live /proc witness, binds separate hashes for live origin snapshot, final origin, independent witness and A/B reconciliation. No false equality between mutable live and final bytes; no per-request proof or authority. Five tests authored and local runner now 24 stages, unexecuted. Next: ITIR typed provenance observation extension, native build, physical negative controls.

- ITIR joint provenance lane: adjacent ITIR-suite PR #73 registers `itir.zkperf.bushido_socket_candidate` using original zkPERF window planner. New Bushido fixed-tool stdio client `itir-bushido-socket.py` checks strict observed-only response, canonical join digest, original four source-hash echo and stream identity; no arbitrary MCP routing. Three Python tests authored; local runner 25 stages, not executed. Native source/run identity and binary attestation OPEN.


### P5.2 Arch/CachyOS packaging candidate (2026-11-23)
- Priority over Debian for local developer host: native local Git-archive SHA256-staged makepkg PKGBUILD (no unverified SKIP), locked npm/Cargo source build, static package inspector, no automatic install or MIME/updater/default registration. Retain Debian checker independently. Three Python static self-tests authored, local acceptance runner now 26 stages; tests, makepkg build and native install/uninstall not run in assistant environment. First user-run `makepkg` artifact plus independent pacman/Wayland/X11 smoke and dependency closure next. Preserve original GPL/source and upstream attribution. No hosted CI or AUR publish.

### P1 Linux WebKit child viewport clipping / upstream issue #7 (2026-11-24)
- Source repair: React sends monotonically increasing epoch on every layout call (including modal and screenshot hides), with Linux Rust dropping older commands. Linux Rust bounds each native pane against the sidebar/top chrome and current window dimensions, using one atomic SetBounds instead of queued SetPosition plus SetSize. Linux Tauri runtime stops fractional resize of non-full-window child views, while retaining automatic full-window React shell resizing. Windows unchanged. New source not compiled or tested here; next: release compile, KDE Wayland YouTube/resize and split/overlay regressions, X11 repeat, inspect compositor behavior under fractional scaling. Keep PR draft until observed.


### P1 issue #7 geometry follow-up — serialized owner and panel coverage (2026-11-24)
- Current implementation: one global Linux async mutex now serializes **sequence admission through all native WebView bounds dispatches**. A monotone React layout sequence supersedes stale work, including legacy `switch_tab`/resize calls once modern commands have been accepted; no old command can interleave its writes after a newer one while the mutex is held. Side panel positioning shares the same mutex and bounds calculation, and issues one atomic SetBounds for visible panels. Offscreen panel hides remain explicit. Fifth pure Rust negative-control test checks stale/equal/legacy epoch rejection. Earlier original patch and this follow-up require compile/runtime validation; tests authored here, **not run**.
- User-reported baseline (pre-issue-#7 source patch): Arch release package built, 134 Rust and 30 frontend tests passed and native GUI launched with YouTube navigation; #7 screenshot shows WebKit painting above React controls. Treat these as separate historic acceptance from the current geometry source; do not promote #7 resolved.
- Priority next: `cargo check --release --locked`, `cargo test --locked`, `npm test`, native KDE Wayland active window + YouTube/multiple navigation, resized/maximized, split tab, panel toggle and screenshot/vault overlay. Then X11, fractional scaling and new `makepkg` candidate. Verify viewport bounds and shell resizing independently. Pinned Wry child-auto-resize heuristic can be replaced by explicit kind classification if runtime evidence shows failure. ITIR-MCP tests/physical same-request correspondence remain separate from this P1 UI blocker.
