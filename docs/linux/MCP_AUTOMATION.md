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
