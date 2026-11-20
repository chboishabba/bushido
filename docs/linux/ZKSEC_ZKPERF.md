# zkSEC / zkperf reconciliation — Bushido Linux

Pinned inspected upstream owners:
- zkSEC ac5ecbadf241f1abb49038b4755a5e74c9209861: COMPACTIFIED_CONTEXT.md, src/zksec/security.py, admissibility.py, ingest_types.py, build_mu_exec_from_ingest.py, mu_exec.py, docs/producer_contracts/mu_exec_ingest_v1.md.
- zkperf 29275570392595547a41f63d444ec01e7595516a: docs/NEED_FOR_INTROSPECTION.md, docs/RECORDING_TOOLS.md, src/witness.rs.

Reuse contract (not an embedded dependency): native page signals and trace observations enter proposal ring; trusted chrome/actor + specific capability + verified scope + trusted approval are distinct from data. Native callback is the enforcement owner. No trace, model, performance threshold or external proposal can grant permission. No source copied from either dependency, no remote collection, no hidden proof transport, no blockchain integration.

## P3.3 — native deny observation

The existing vendored Wry generic permission-request signal denies all requests. A top-level WebView URI is NOT a requesting-frame origin; no proof of actual origin or user activation is supplied by this generic callback. Do not use top-level URI as authorization. Keep all requests denied until a capability-specific native origin and document/lifetime binding is established. Later typed requests must be scoped to requesting origin + top-level origin + workspace + native generation + document epoch + exact capability; consent is explicit single-use and expires on navigation, close, workspace change or policy change. Unknown types, panels and unproven iframe origin fail closed. Legacy Windows domain grants must not be imported as Linux authority.

Debug-only opt-in BUSHIDO_LINUX_PERMISSION_OBSERVATION=1 before process start enables a saturating process aggregate of calls to native deny(). Trusted main-WebView command linux_permission_observation returns a proposal_only object with decimal counter, no URL or payload. It is not per-view attribution or real observed denial outcome. No frontend polling or production recording. Measure on isolated synthetic profile only; use before/after snapshots and separately observe fixture behavior. No exposure to remote child IPC. The count is process-global, not a cross-workspace security proof. Disabled/release/non-Linux returns explicit error. Native invocation/compilation open.

## Five-layer local evidence index

zkperf specifies binary closure, source/debug symbols, raw traces, derived models and build events. scripts/linux/witness-receipt.py takes an explicitly supplied local JSON spec, hashes only named files from a private isolated evidence directory, records absent layers explicitly, and emits a proposal-only local index. It does not collect traces, infer file semantics, validate same-binary linkage or prove the contents. A claimed commit is not a verified source/binary weld. Hash commitment is not ZK or authenticity. Inputs must be reviewed and quiescent; concurrent mutation and same-user filesystem races are outside this tool's protection. No external transmission. File names are part of the private index, do not use secret-bearing paths. Example spec:

```json
{"commit":"0123456789012345678901234567890123456789","layers":{"binary_closure":[],"source_debug":[],"traces":[],"models":[],"build_events":["receipt.json"]}}
```

`python3 scripts/linux/witness-receipt.py --root /path/to/private/evidence --spec /path/to/spec.json --output /path/to/new-index.json`

Next: gather exact binary + build ID and debug symbols, real dependency closure, native fixture traces and zkperf outputs; map actual events to pinned zkSEC TraceProposalFact schema via a reviewed producer adapter, then actual Ghidra IR facts and explicit same-run/same-binary/object linkage. Existing zkSEC fixture builder's default linkage scores depend on presence of facts, not sufficient real-world same-object verification. Do not auto-promote unrelated trace and IR slices, invent producer identities, claim invariant failure from trace alone or pass aggregate counts as per-event syscall facts. Keep authority decisions and performance observations independent. No hosted CI.
