# Remote Multi-Env — Full Remote Management: Cleanup + Implementation Spec (2026-08-01)

Goal: a paired `ui:agent` client manages the host with near-local fidelity — conversations (start/continue/steer/stop), artifacts, workspace reviews + Review PR, workspace automation flags, and options. Read-only/one-shot behaviours shipped as v1 stopgaps are removed. Companion analysis: `full-host-control-reassessment.md` (same directory) — all file:line evidence lives there.

## Owner decisions (resolved 2026-08-01, this spec)

| # | Decision | Resolution |
|---|---|---|
| 1 | Q3 (remote start host-only) | **Superseded** by shipped Parts B+C. Record in tracker; spawn-free start is sanctioned. |
| 2 | Stop tier | Intent-row redesign, ledgered authority-reducing, registered at **`ui:operate`** (brakes stay on the default pairing). |
| 3 | Auto-merge/auto-commit from remote | **Permitted** under `ui:agent` (full-management goal). No extra host toggle. |
| 4 | Role defaults | Narrowed variant registered: refuses `approval_policy`/`sandbox_mode` writes; those two stay host-local. |
| 5 | UX-1/R1 project reads | **Authorized**: spawn-free project read path (twin or seam split), registered `ui:read`. |
| 6 | Host-produced attachments | Host-owned rows readable by **any paired device at `ui:read`**, conversation-scoped ids. |

## Invariants (NON-NEGOTIABLE, every work package)

1. **Detector-(c) process floor is absolute.** No command that resolves a CLI path or spawns is ever registered. Unlocks are redesigns: intent rows drained by host-owned dispatchers, seam splits isolating DB-only writes, projected/persisted reads (twin pattern).
2. Never soften the tier boundary: reads → `ui:read`; brakes/inert → `ui:operate`; arming/steering/content writes → `ui:agent`. Pins (`role`, `mode`, field-absence) over trust.
3. Every spawn-free steering command gets a `DECLARED_MEMBERSHIPS` row (P-17b is detector-generated and blind to them) and, when it arms a loop, a `SPAWN_TRIGGERING_STATE_SURFACE` row in `authority_audit.rs`.
4. Fail closed. No `.ok().flatten()`/`unwrap_or_default()` on reads that gate rendering or authority. Registering a command with a known fail-open requires fixing it first.
5. New intent surfaces follow the proven shape: entity + status enum with terminal failure states, CAS `claim_pending_*` repo method with **distinctive method names** (detector-b ties mechanically), migration (forward-only, after `v20260801120000`), dispatcher in the **always-run startup prefix** (`startup_pipeline.rs` before the recovery early-return), stale-lease sweep, revalidation before spawn, terminal/failure surfacing to the client (design doc §7 hazard: persisted-but-never-dispatched must become a visible failure).
6. TDD; focused tests only (rule 8); `cd src-tauri && cargo clean` after any Rust test run in your worktree (rule 8.5). Regenerate `docs/generated/remote-commands.json` + frontend generated mirrors after ledger/registry changes.
7. Stays denied forever (out of scope, do not touch): `agent_terminal_commands`, `api_key_commands`, `external_mcp_commands`, gh/git credential + origin ops, `resolve_merge_conflict`/`cleanup_task*`, installer surface, `workspace_open_commands`, `deletesEntity` rows, `update_custom_analysis`.

## Work packages

### Wave 1 (parallel worktrees off `feat/remote-multi-env`)

**WP1 — Conversation continuation (Option 1).**
Remove one-shot behaviour. New intent surface `remote_conversation_message_requests` (or `kind` column reuse — implementer's call, document it): client `request_remote_agent_conversation_message` (AgentControl, pinned `role:"user"`, caps `[MutatesAgentConsumedContent, SeedsSpawnTriggeringState]`) + `get_remote_conversation_message_request` poll (Read). Dispatcher terminal call is `ChatService::send_message` (provider-session resume seam, `chat_service_context.rs:2791-2843`) — **not** `AgentConversationStartService::start`. Client: `sendMessage` remote branch becomes live-run ⇒ existing `send_remote_chat_message` path; idle ⇒ intent + poll; remove/replace the `REMOTE_CHAT_SEND_NOT_STEERABLE` dead-end UX. Also carry the UX-5 fix: composer options (model/effort) travel in the intent instead of being silently dropped.

**WP2 — Stop + brake surfacing.**
`request_remote_agent_stop` intent row (statuses incl. `NoLiveRun` terminal), dispatcher calls host-local stop (the pkill path stays host-owned). Ledger: authority-reducing, register at `ui:operate` with `AUTHORITY_REDUCING_EXEMPTIONS` row. Client: repoint stop affordance for remote envs; **fix both swallow sites** (`useChatActions.ts` catch → surfaced error/toast state; `AgentComposerSurface.tsx` `void onStop?.()` → awaited with failure surfacing — check local UX parity); add gate-map entries so unavailable ops render hints, not enabled buttons. Include `send_queued_agent_message_now`'s stop half only if trivial; else record deferred.

**WP3 — Render fidelity: remounts + tool-call detail.**
(a) Add `GET /api/agent-workspaces/:conversation_id/workspace-review-context` and `.../pr-review-context` to `REMOUNT_ALLOWLIST` + handler arms; audit `?refresh_target=true` for read-onlyness first — if it mutates, strip the param on the remote path and document. (b) Fix the five `.ok().flatten()` fail-opens in `load_delegated_tool_runtime_snapshot`, reclassify both tool-call-detail commands to `read`, register at `ui:read`. Also fix the A3/L2 ledger-claim mismatch (transcript trio's five swallowed delegated-tool reads) if it is the same seam.

**WP4 — Registration sweep + dead-capability wiring.**
(a) `AppError` serialization (or fallible dispatch arm) → register the ~10 clean `transport-shape-deferred` rows (5 task-step ops, `reorder_task_steps`, folder-reference reads, `abort_seeded_agent_conversation`). (b) Register the 25 `registerable` rows with frontend affordance repoints (registration without repoint = dead flag). (c) UX-2: repoint the UI transcript reads to the six registered `get_remote_*` twins for remote environments. (d) UX-1: spawn-free `list_projects`/`get_project` (twin or seam split past the spawning hydrator), registered `ui:read`; update the CI pin test to assert the *spawning* getters stay unregistered while the twins are. (e) Bucket C small fixes as reachable: `get_pending_permissions`/`get_pending_questions` fail-open, `get_manual_role_defaults` fabricated default, `set_active_plan`.

### Wave 2 (after Wave 1 merges)

**WP5 — Diff/branch audit lane.** Hand-trace 29 `diff_commands` + 4 `plan_branch_commands`; register the persisted/cache-served reads; host-side snapshot job + remote read for the genuinely-spawning ones. Unlocks review/PR fidelity.
**WP6 — Options & automation seam splits.** Auto-publish / PR-supervision flag writes split from `resolve_agent_workspace_pr_automation_target`; `set_agent_conversation_muted`; `update_execution_settings` (write-then-host-drains); non-Ultra `update_agent_conversation_coordination_mode`; resume family (UX-4) via intent rows; `activate_agent_task_pipeline`/`_plan_direct_implementation` seam splits.
**WP7 — Host-produced attachments.** Ingress minting host-owned `remote_attachments` rows (conversation-scoped), authorization per decision 6, client GET + blob rendering replacing the "Stored on the host" placeholder; startup orphan-blob sweep (B-new1).
**WP8 — Policy unlocks + review fixes.** Narrowed role-default variant (decision 4); `repository_settings` flip per 1.3 flagged-not-flipped review; folder-reference allowlist confinement; `request_task_changes_from_reviewing` (propagate serde errors) + `reject_fix_task` (mediator pins corrective target).

## Merge protocol

Each WP: own worktree + branch `feat/rme-wp<N>-<slug>` off `feat/remote-multi-env`. Orchestrator reviews the diff, re-runs the WP's stated gates, merges into `feat/remote-multi-env` sequentially, regenerates `remote-commands.json` + frontend mirrors after each merge (the generated files are expected conflict points — resolution is always "regenerate", never hand-merge). Tracker updated per WP (Q3 supersession recorded in WP1's merge).
