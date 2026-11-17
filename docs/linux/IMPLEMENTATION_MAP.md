# Implementation map and work packages

This map refers to the 2026-11-17 source snapshot on `linux-port-pr4-source`. Paths and line references are indicative; locate symbols before editing.

## Work packages

| Package | Existing source/interface | Linux work | Dependency |
| --- | --- | --- | --- |
| WP-A compile | `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/src/screenshot.rs`, `src-tauri/forks/*` | Genuine Linux checks; cfg fixes strictly scoped by compiler failures | None |
| WP-B tab shell | `create_tab`, `open_glance`, `WebviewBuilder`, tab/layout Tauri commands | Verify child webview sizing, navigation, session restoration, popups | WP-A |
| WP-C network policy | `src-tauri/src/lib.rs` WebView2 COM block in `create_tab`, `src-tauri/src/blocker.rs` | Split request decision from Windows COM callbacks; shared typed result, URL/source/context/headers | WP-A |
| WP-D WebKit adapter | Vendored `src-tauri/forks/wry` and `tauri-runtime-wry`, Linux WebKitGTK | Integrate enforceable request events at engine/network layer, not only navigation callbacks | WP-B/C |
| WP-E DNS | `src-tauri/src/dns_resolver.rs`, current Windows `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` | Route *actual* WebKit networking through selected resolver; measure leaks | WP-D |
| WP-F storage | `src-tauri/src/sync/keys.rs`, `src-tauri/src/vault.rs` | Linux Secret Service + authenticated envelope implementation added; verify real desktop lock/unlock, persistence, migration and recovery | WP-A |
| WP-G workspace isolation | `with_profile_name`, `WebviewBuilder::data_directory`, `clear_workspace_data`, vendored Tauri/Wry | Linux tabs and Glance select distinct WebContext directories; selective clear now propagates native WebKit callback completion (runtime unverified) | WP-B |
| WP-H permissions | `respond_permission`, Windows COM `PermissionRequested`, vendored Wry `webkitgtk/mod.rs` | Linux native default-deny callback added; implement origin-scoped request-to-prompt path with explicit allow/deny and persistence | WP-B |
| WP-I feature extras | `src-tauri/src/screenshot.rs`, `src-tauri/src/downloads.rs`, zoom/devtools/clipboard commands | Real Linux implementations and tests | WP-B |
| WP-J release | `src-tauri/tauri.conf.json`, `.github/workflows/release.yml` | Linux-specific bundling/CI; retain Windows NSIS | WP-A–I |

## Existing work and limitations

- Forked Wry/Tauri already contains GTK/WebKit implementations; do not replace it blindly.
- Linux `dl_task_chunked` currently falls back to a single stream, not multi-segment parity.
- Windows-only request handling includes headers, blocking, downloads, messages, navigation/error, popups, permission and context menu paths.
- Windows runtime flags cannot be assumed to configure Linux WebKit.
- The current Linux key-wrap shim returns an error instead of plaintext. This is a deliberate safety restriction, not finished key storage.
- Unsupported screenshot/zoom/clear/permission commands must have explicitly reported errors until functional implementations exist.

## Engineering rules

**Reuse -> isolate -> implement -> verify.** Avoid a duplicate browser pipeline. Keep the UI independent of engine APIs. Keep policy tests deterministic and network/runtime tests grounded in observable server/packet logs.

For CI, start with `cargo check --locked` and `npm run build`; add GUI smoke tests on a Linux runner with an appropriate graphical session rather than pretending compilation alone validates rendering.

## PR review checklist

- [ ] Current Linux `cargo check --locked` accepted
- [ ] Current Linux `cargo test --locked` accepted
- [ ] Frontend build accepted
- [ ] Native tab/nav smoke evidence
- [ ] No silent Linux security no-ops
- [ ] Network policy / DNS / isolation security fixtures accepted
- [ ] Windows regression checked
- [ ] PR #4 attribution and upstream coordination preserved
