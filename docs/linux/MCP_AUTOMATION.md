# Local automation / Rust MCP decision (2026-11-20)

Priority: prepare the acceptance seam now; do not add an embedded MCP server, Cargo dependencies, CI workflows, or a second browser engine during P4 implementation. Full automation integration is a P0/P1 acceptance-sprint task, independent of release builds.

## Reuse selection

| Candidate | Assessment |
| --- | --- |
| [tauri-driver](https://github.com/tauri-apps/tauri/tree/dev/crates/tauri-driver) + WebKitWebDriver | First target: external official Tauri driver exercises the application binary. Verify child-WebView selection and automation context support before relying on it. |
| [Bruster](https://github.com/aliilyas/bruster) | Rust native-WebView browser + MCP and JSON-lines control, MIT. Best reference for compact commands and native backend; standalone Bruster is NOT evidence for Bushido. Audit whether its driver layer can be reused without replacing our browser. |
| [rust-browser-mcp](https://github.com/EmilLindfors/rust-browser-mcp) | MIT Rust WebDriver MCP server; Chrome/Firefox documented. Evaluate existing W3C transport, adapt only the driver/session launcher if compatible. |
| [rmcp](https://github.com/modelcontextprotocol/rust-sdk) + [fantoccini](https://github.com/jonhoo/fantoccini) | Thin Rust sidecar fallback, using official SDK stdio plus established WebDriver client; no custom protocol implementation. |
| [tauri-webdriver](https://docs.rs/crate/tauri-webdriver/0.2.0) | Embedded plugin fallback if external driver cannot address child views. Requires separate dev-only dependency and isolation review. |
| CDP-only browser MCP | Not acceptable for Linux WebKitGTK parity receipts; Chromium is a different browser. |

No candidate is pinned, installed, benchmarked or accepted yet. Prefer official external driver, then reuse an existing Rust MCP adapter, then thin rmcp wrapper as needed. Reassess exact versions/licenses and lock them when actually integrated.

## Proposed boundary

MCP stdio sidecar -> existing compact WebDriver client -> tauri-driver -> WebKitWebDriver -> same Bushido executable + isolated test profile. No generic remote-page Tauri IPC, production plugin, LAN listener, credential extraction or access to a user's real browser profile. Local ephemeral session, loopback driver, allowlisted local fixtures; teardown process and profile on completion. Native GTK compositor/chooser/permission/PiP windows may need a separate OS-level test fixture; WebDriver DOM evidence is not native-window acceptance.

Keep the contract scenario-first:
- capabilities: backend, versions, available targets and explicit unavailable options.
- snapshot: bounded semantic interactive tree with short refs scoped to current view/document; no full HTML by default.
- action batch: ordered, capped typed actions (navigate fixture, click ref, fill, key, wait for condition, select view), fail at first error and return compact step receipts.
- run scenario: named deterministic test (find, tab/split, workspace, Glance, download, PiP, network fixture).
- artifacts: explicit screenshots/logs only on request/failure; return local paths plus hashes, not base64 by default.
- session close: dispose all views and isolated data.

Use the same scenarios via ordinary Rust test runner or MCP; MCP is orchestration, not the test oracle. Acceptance receipts include exact Bushido SHA, WebKit/GTK/toolchain, fixture ID, per-step observations, server request counters where relevant, failures and artifact hashes. Token/runtime budget: measure calls, serialized output bytes, elapsed time and peak RSS for identical scenario with single actions vs bounded batches; no guessed savings.

## Sprint order and checkpoint

A0 now: document choice and reuse existing local fixtures; standalone Node policy regression can run without GTK (not native CI).
A1 acceptance sprint: build pinned app, run external driver with isolated XDG paths and local fixture, prove main and child target addressing, no profile leakage.
A2: evaluate existing Rust WebDriver MCP server with Tauri capabilities; adapt or use rmcp/fantoccini thin sidecar if needed. Keep separate lockfile/package and no shipping integration.
A3: compact batches, snapshots, deterministic receipts, negative security controls and benchmark.
A4: only after local acceptance, decide CI integration. Do not run hosted CI for the current implementation sprint.

### A0 local receipt seam
`python3 scripts/linux/accept-local.py` now records the P0 stage outcomes, exact source SHA and lock hashes, host/tool versions, and private per-stage log hashes. This is a build/fixture orchestrator, not an MCP server or WebDriver session. A1 must consume its successful receipt and attach distinct scenario evidence for the actual same-source executable and isolated profile.

## A0.1 ITIR shared zkSEC/zkperf connectors

ITIR-suite PR #73 (ad35e4ac8c9ce606418a760a122bc603aaf92b94) introduces the three read-only tools itir.zksec.policy_preview, itir.zkperf.window_plan, itir.zkperf.evidence_index_check in the existing registry, visible via FastMCP and existing native JSON-lines bridge. No new protocol or browser runtime server. The tool families are observation-only and cannot execute, publish, grant permissions or establish grounded security claims.

The external operator-side scripts/linux/itir-evidence-check.py calls only the evidence-index tool via existing python -m itir_mcp --bridge safe_call. Explicit trusted local Python executable and suite checkout required. Example:
```sh
python3 scripts/linux/itir-evidence-check.py --index /private/index.json \
  --python /absolute/itir/.venv/bin/python --suite-root /absolute/itir \
  --output /private/new-itir-receipt.json
```
The client requires abstained/proposal-only response, checks claimed index digest and commit identity and rejects all verified-file/proof/authorization claims. No arbitrary tool name, no shell, no embedded MCP. Trusted local provider only; subprocess timeout and input bounds; response size checked after capture (not a sandbox for hostile provider). No actual provider calls during preflight, just synthetic contract tests. The future Rust sidecar can consume the same registry over MCP stdio with a pinned SDK and explicit tool allowlist, distinct from the browser action driver. Actual native bridge/provider acceptance remains OPEN.

### A1 geometry/pixel diagnostic scenario (P1 issue #7)
Use the same running Bushido binary, not Chrome or a replacement WebKit browser. Source instrumentation: BUSHIDO_GEOMETRY_TRACE=1; local fixture scripts/linux/geometry-fixture.html and operator capture helper scripts/linux/geometry-capture.sh. Record exact binary hash, source SHA, session/backend, timestamps, view identity and native allocation trace. Reuse trusted-chrome screenshot::capture_visible for native WebKit PNG; capture independent compositor window before invoking any screenshot overlay that hides children. Current helper is operator-driven, not MCP-integrated; window identity must be checked visually. Never silently substitute a WebKit snapshot for a compositor screenshot.
A1 remaining: native compile, same-view identity and time correlation, non-mutating native snapshot export, automated window selection/capture under KWin Wayland with explicit consent, X11 fallback, image edge comparison and deterministic A/B repair. A2 sidecar capabilities should distinguish page_snapshot, window_capture, geometry_trace and run_geometry_scenario, with private local paths/hashes, explicit unsupported/failure outcomes and no arbitrary desktop capture. No embedded server, hosted CI or production remote capture.

### A1/A2 promoted — external observation sidecar (2026-11-24)
Priority supersedes the deferred sprint order above: install the local sidecar against the already-running instrumented binary before further viewport patches. scripts/linux/bushido-mcp.py uses the existing Python FastMCP SDK (separate environment, no browser dependency or Rust rebuild); Rust rmcp/Fantoccini remains the native-driver target. Initial tools: capabilities, geometry_trace, inspect_geometry (log-derived), window_capture (explicit operator focus, Spectacle), page_snapshot (explicit unsupported), action_batch (opt-in X11 fixed navigation keys only), run_geometry_scenario (observation packet only). No native attachment or Wayland input claim. Same-UID selected process, private output, bounded log tail, no shell command tool, no remote listener. Log association and active-window identity are not attested.

Local installation using an existing ITIR environment with MCP SDK, or a separate venv:
```sh
python3 -m venv ~/.local/share/bushido-mcp/venv
~/.local/share/bushido-mcp/venv/bin/pip install 'mcp>=1.12,<2'
codex mcp add bushido -- ~/.local/share/bushido-mcp/venv/bin/python /absolute/bushido-linux-port/scripts/linux/bushido-mcp.py --pid PID --log /tmp/bushido-geometry.log --output /absolute/private/bushido-artifacts --allow-capture --allow-image-return
codex mcp list
```
Replace PID and absolute paths; restart Codex session as needed. Dependency range is provisional, exact lock/SDK transport smoke and local registration OPEN. Never claim screenshot target verified merely from the selected PID. Use isolated test profile. Next: pinned SDK handshake/tools/list/call, KWin consent-based target selection/input (no xdotool fallback on Wayland), non-mutating internal screenshot export, GTK owner/view identity join, automated fixture actions and pixel comparison. Existing native binary can be inspected without rebuilding; changed native code still needs incremental compile for confirmation.

- A1.1 image return follow-up: `view_capture(sha256)` returns MCP PNG image content from session-produced captures only, with separate `--allow-image-return` opt-in, 8 MiB cap and hash recheck. PID start ticks and executable path pinned at startup. SDK import/transport and actual image delivery unverified; no local SDK in assistant runtime. Active-window identity remains operator-verified, Wayland actions remain open.
