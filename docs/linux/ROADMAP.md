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
| P2.1 | Network | Shared Rust policy for document and subresource requests, redirects, headers | Policy unit tests + Windows parity vectors | OPEN |
| P2.2 | Network | WebKitGTK adapter attached at a *pre-network* enforcement point | Local HTTP fixture verifies blocked requests never reach server | OPEN |
| P2.3 | Network | HTTPS-only, redirects, third-party matching, scriptlets/cosmetic filters | Positive and negative network fixtures including iframes/workers | OPEN |
| P2.4 | Network | DoH reaches WebKit network stack, no fallback/leak at maximum privacy | Packet/DNS capture and failure-injection tests | OPEN |
| P3.1 | Security | Per-workspace cookie/cache/storage isolation + selective clear | Two-workspace fixture; no cross-workspace leakage; clear actually erases | OPEN |
| P3.2 | Security | Secret Service/portal or well-defined secure Linux key storage with versioned format | Rest/encryption, lock, migration and failure tests; no raw fallback | OPEN |
| P3.3 | Security | Permissions decision and prompt mediation with remember/deny | Camera/location/notifications/clipboard fixtures | OPEN |
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
