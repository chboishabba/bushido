# Linux port continuation (PR #4)

This is an **unverified candidate**, not a verified Linux release.

## Changes

- Preserves AdithyaaSivamal's original Windows-only `controller()` guard from PR #4.
- Removes `.auto_resize()` from child tab webviews so the explicit pane layout owns geometry.
- Makes `arboard` available for Linux clipboard operations.
- Replaces silent Linux no-op behaviour in screenshot capture, zoom, workspace clearing and permission response with explicit unsupported-operation errors.
- Refuses non-Windows plaintext pass-through in the DPAPI compatibility shim. Sync identity storage and affected vault operations remain unavailable until a secure Linux backend is implemented.
- Adds a Linux-only test asserting that secret wrapping cannot silently store plaintext.

## Safety and verification

**Linux is not privacy-parity safe.** Windows WebView2 COM interception handles request blocking, headers, download events, browser DNS overrides, permission events and more. These are not automatically available in Linux WebKitGTK.

These changes have **not** been built or run in Linux. Before marking ready, run:

```bash
cd src-tauri
cargo check --locked
cargo test --locked
cd ..
npm ci
npm run build
npx tauri dev
```

Production Linux packaging also needs non-NSIS targets. Test end-to-end subresource interception, redirects, DoH routing, workspace isolation, permission mediation and fingerprint behavior. Do not advertise browser hardening until independently verified.

## Next implementation cut

Extract a shared request policy and implement a WebKitGTK-specific enforcement adapter; verify what cannot be implemented with available WebKit APIs and ensure remaining security features fail closed. Keep Windows behavior unchanged.

## Attribution

This branch is based on AdithyaaSivamal's PR #4 commit `f83d133` and extends that work; it is not a substitute for upstream PR #4.
