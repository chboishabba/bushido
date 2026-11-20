# Bushido native Linux port — execution roadmap

**Updated:** 2026-11-17  
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
| P5.1 | Release | Linux package configuration separated from Windows NSIS | Install/uninstall and smoke tests on target distro | OPEN |
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
