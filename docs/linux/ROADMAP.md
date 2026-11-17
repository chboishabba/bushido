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
