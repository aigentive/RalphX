---
paths:
  - "src-tauri/src/remote_server/**"
  - "src-tauri/src/commands/remote_*.rs"
  - "src-tauri/crates/ralphx-remote-protocol/**"
  - "frontend/src/lib/remote/**"
  - "frontend/src/api/*.ts"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Remote Facade

Scoped rules for the `:3849` host/client surface. These were root `CLAUDE.md` principles 27-28; they moved here so they load only for the code they govern.

| Rule | Detail |
|---|---|
| Command registration (NON-NEGOTIABLE) | Every `:3849`-reachable command is a hand-audited `remote_server/registry.rs` allowlist entry with a `capability_ledger.rs` class; never a passthrough, a `generate_handler!` edit, or a command fork. Details: `docs/architecture/remote-protocol.md` |
| Event fan-out (NON-NEGOTIABLE) | Remote events travel by classification-table delivery class — Durable rides the sequencer, Transient broadcasts with no seq and is never persisted, LocalOnly never leaves the host. ❌ Re-deriving remote event sources from `EventSink`/`AppState.events` |
| Twin naming | A spawn-free twin is named for what its closure does (persists an intent), not for what the host later does (starts an agent). |
| Absence is the signal | Client gates derive `unavailable` from a command's absence in the generated manifest — never from a hardcoded name list. |
| Spawn-distinct paths are distinct FUNCTIONS (NON-NEGOTIABLE) | When a local path may reach a spawn/recovery seam and its twin may not, express the difference as two functions sharing only work that names NEITHER seam. ❌ One body behind a `bool`/enum flag: the ledger detectors are call-graph based, so a shared body that names both seams puts the spawned side in the twin's closure whatever the flag does at runtime. Measured 2026-08-05 on `set_agent_conversation_muted`: the flag form ran 18 hops into `reconcile_reserved_claude_registration` and the corrective-transition sinks, failing `detector_c_floors_process_spawn_authority`, `batch13_detector_gap_is_measured_not_inherited`, and `no_registered_facade_target_reaches_a_corrective_transition`. Same reason a closure/bare-fn indirection must not be used to *hide* a launch — the gate is only worth what the graph can see. |
| Test the real envelope | Remote client tests assert the shapes the transport actually produces — `{outcome: "commandError", error: <the command's own error value, unwrapped>}` per `network-invoke.ts`, explicit nulls, real casing. ❌ Mock-convenient envelopes (`{outcome: "error", …}`) that no client path can emit; they pass while proving nothing. |

## Client surfaces

- **Remote Connection Journal** — `stores/remoteConnectionJournalStore.ts` is the per-environment connection diagnostics ring buffer; `lib/remote/environment-runtime.ts` is its single writer, the banner Details dialog its reader. Remote HTTP reads must lift the host's `REMOTE_COMMAND_UNAVAILABLE` envelope into `RemoteTransportError` (capability boundary, tolerated by the hydration barrier) — ❌ flattening it into generic HTTP errors.
- **Tauri Plugin Prefix Rule** — every `plugin:*` command (opener/dialog/fs/updater/process/global-shortcut/notification) routes to THIS device via the one prefix rule in `lib/remote/local-only-commands.ts`; their subject is the machine showing the UI. Host-targeted plugin calls need a reviewed row in `HOST_TARGETED_PLUGIN_COMMANDS` (empty today) and then a registration or ledger row. ❌ Per-call-site remote branching for plugin invokes; ❌ passing a host filesystem path to `openPath`/`revealItemInDir` — those degrade through host-affordance gating to `HostPathCopyButton`.
- **Syncing Presentation** — `lib/remote/supervisor-presentation.ts` owns the `syncing` projection (live socket mid-hydration → chip-only accent pulse in `EnvironmentSwitcher`, no banner) with K=2 barrier-failure / T=12s one-way escalation back to `reconnecting`; still read-only. ❌ New surfaces reading FSM state directly to infer "connection dropped".
