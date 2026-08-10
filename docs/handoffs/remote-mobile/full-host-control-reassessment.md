# Remote Multi-Env — Full Host-Control Reassessment (2026-08-01)

Scope: verification of the 2026-07-31 gap report against branch tip `bb84dd53c`, plus the phase plan for lifting v1 deferrals so a paired client can manage the host — conversations, artifacts, reviews, options — with near-local fidelity under `ui:agent`. Evidence gathered by three independent code sweeps over this worktree; all file:line refs verified at `bb84dd53c`.

Authority note: the gists circulated with the report (`ce3bf9c4…`, `5964a766…`) are the **provider-connections** spec/plan, not the remote spec. The authoritative remote spec is `.artifacts/specs/remote-multi-env/source-spec.md` (round 6) + `tracker.md` + `docs/handoffs/remote-mobile/spec-amendment-proposal.md`.

---

## 1. Verdict on the reported A-list gaps

All five confirmed, but three had wrong root causes. Corrections matter because they change the fix shape.

### Gap 1 — Continue an idle conversation: CONFIRMED, intentional (Option 2)

- `send_remote_chat_message` refuses without a live run at `src-tauri/src/commands/remote_chat_commands.rs:123-135` (`REMOTE_CHAT_SEND_NOT_STEERABLE`). Deliberate: the queued row is drained only inside a live run (`chat_service_queue.rs:987`), so a no-run send would be a persisted-but-never-delivered false success. This is design §4.1 **Option 2** shipped as designed; **Option 1** (durable queue + host dispatch driver, `SeedsSpawnTriggeringState`) is the unbuilt continuation seam, pre-acknowledged in the tracker ("future PR if the UX reads as arbitrary" — it does).
- The spawn-free start path mints **new conversations only**: `RequestRemoteAgentConversationStartInput` has no `conversation_id` field (`remote_conversation_start_commands.rs:71-79`); the dispatcher calls `AgentConversationStartService::start`, which treats a supplied id as a seeded *draft*, never a resume target (`start.rs:442`).
- The real continuation seam is provider-session resume inside `ChatService::send_message` (`chat_service_context.rs:2791-2843`, `--resume <session_id>`), reachable only via `send_agent_message` — ledgered `host-denied-spawns-process`.

### Gap 2 — Stop a running agent: CONFIRMED, including "silent"

- `stop_agent` is unregistered because its implementation resolves `pkill` (`capability_ledger.rs:2956-2963`) — blocked by the detector-(c) process floor, not by risk class, even though it is authority-*reducing* (the spec's own exemption category; `stop_execution`/`pause_execution`/`deny_permission_request` all carry `AUTHORITY_REDUCING_EXEMPTIONS` rows).
- It is missing from the agent-gate/inert affordance maps, so the Stop button renders **enabled** remotely; the host answers `REMOTE_COMMAND_UNAVAILABLE`, and the failure is swallowed twice: `useChatActions.ts:448-459` (`logger.warn` only) and `AgentComposerSurface.tsx:2239-2243` (`void onStop?.()`).
- The chat-send design doc never audited `stop_agent` (§7 disclaims it) — this is an unexamined gap, not deferred work.

### Gap 3 — Host-produced attachments: CONFIRMED, worse than reported

- Not a scoping bug: host-agent files never get a `remote_attachments` row at all — the only writer is the client `upload_handler` (`attachments.rs:120-305`). The fetch route is additionally device-scoped in SQL (`sqlite_remote_request_dedup_repo.rs:248`).
- **No frontend code calls `/remote/v1/attachments/{id}` at all** — the placeholder ("Stored on the host", `host-affordances.ts:31`) cannot ever fill by construction. `MessageAttachments.tsx:92-94` still says the endpoint work is "DEFERRED TO 3.1"; 3.1 closed without it (tracker's orphaned-scope ledger owes it explicitly).
- Fix = (a) host-side ingress minting rows for host-produced files, (b) an authorization model beyond `device_id = uploader` (host-owned vs device-owned), (c) client GET + blob URL rendering. No spawn machinery involved.

### Gap 4 — Expand a tool call: CONFIRMED unregistered; "never audited" REFUTED

- `get_agent_message_tool_call_detail` / `get_agent_timeline_item_tool_call_detail` are `v1-audit-refused / fail-open-until-fixed`: `load_delegated_tool_runtime_snapshot` applies `.ok().flatten()` to five repo reads (`unified_chat_commands/mod.rs:2496-2536`), so an outage serves a **stale persisted tool result as current**.
- Handlers are pure DB reads (`mod.rs:10015`, `:10056` — no AppHandle/ExecutionState/ChatService). Fix = propagate the five errors, reclassify from the conservative module default to `read`, register at `ui:read` (registered sibling `get_remote_agent_conversation` already carries transcript text at that tier).

### Gap 5 — Open a workspace review: CONFIRMED; blocker is the fetch remount, not invoke

- The event forwards fine and both artifact halves are already fetchable (`get_artifact` is registered). The break: the artifact ids come only from `GET /api/agent-workspaces/:id/workspace-review-context` via `backendFetch`, and that route is not in the 8-row `REMOUNT_ALLOWLIST` (`fetch_remount.rs:92-141`). Ids stay null → both `get_artifact` queries stay disabled → opened tab, no artifact.
- Fix = one `RemountRoute` (GET, `RiskClass::Read`) + handler arm; audit the `?refresh_target=true` variant (`chat.ts:4148`) for read-onlyness first. Same story for `pr-review-context` (`AgentsArtifactPane.tsx:979`).

---

## 2. The bigger reframe the report missed

The tracker's UX lens (record 16) is blunter than the gap report: **"the remote product is substantially non-functional today even though the security model is sound."** Two items sit *ahead of* the A-list:

- **UX-1 (front door):** `list_projects`/`get_project` are spawn-blocked and CI-pinned unregistered (`spawning_project_getters_are_elevated_and_not_registered`); census batch R1 (spawn-free project read) is a code change gated on an owner call. ⚠️ The dogfood session reported project/task reads working — reconcile whether a twin landed post-census or the dogfood host used a different path before treating UX-1 as open.
- **UX-2:** the purpose-built remote transcript twins (`get_remote_agent_conversation*`, 6 registered commands) have **zero frontend callers** — the UI still invokes the unregistered local names. Registration without affordance repointing ships dead capability (chat-send lane finding #1).

Also open from the tracker follow-up ledger (owner sign-off owed): A2 `get_artifacts(None)` fail-open, A3/L2 transcript trio swallows 5 delegated-tool reads while its ledger claims propagation, A-new `UpdateTaskInput.internal_status` silently dropped on the registered Operate path, B-new1 attachment orphan-blob quota leak, D1/M3 drift/mirror CI gates skipped by backend-/docs-only PRs.

---

## 3. What "full host control" actually requires — inventory triage

Manifest state at tip: 555 ledger rows; **226 registered** (130 read / 93 agentControl / 5 operate — tracker counts are stale); 114 `host-denied-spawns-process`; 98 `host-denied`; 58 `v1-deferred`; 34 `v1-audit-refused`; **25 `registerable` but unregistered**.

Standing owner rulings (2026-07-28) that bound this phase: detector-(c) process floor is absolute ("no exceptions ever") — every unlock below is a redesign, never a relaxation; "permitting more = register more surface under `ui:agent`, never soften the tier boundary"; every spawn-free steering command needs an explicit `DECLARED_MEMBERSHIPS` row (P-17b generates from detector output and cannot see them).

| Bucket | Content | Cost |
|---|---|---|
| **A — free now** | 25 `registerable` rows: `queue_agent_message`, `get_effective_manual_role_default`, `set_max_concurrent`, 6 agent-profile ops, 11 ideation ops (incl. `send_chat_message`, `send_orchestrator_message`), `get_team_artifacts_by_session`… | Registration + affordance repoint only |
| **B — one shared fix** | `transport-shape-deferred` rows (~10, bodies audit clean): all 5 task-step ops, `reorder_task_steps`, folder-reference reads, `abort_seeded_agent_conversation` | Make `AppError` serializable (or add a rendering dispatch arm). Highest ratio in the inventory |
| **C — small named bug fixes** | fail-open/corrective rows with fixes already written in the manifest: the 2 tool-call-detail reads, `request_task_changes_from_reviewing` (propagate 2 serde errors), `reject_fix_task` (mediator pins target), `get_manual_role_defaults`, `get_pending_permissions`/`questions`, `set_active_plan`… | Per-command TDD fix + reclassify + register |
| **D — intent-dispatcher / seam splits** | The control plane: continue, stop, queued-send-now, persona/mode switch, auto-publish & PR-supervision flags, muted, execution settings, resume family (UX-4), workspace-hydrator reads, 29 `diff_commands` + 4 `plan_branch_commands` (blanket "may spawn git", never hand-traced) | Pattern is proven twice (`send_remote_chat_message`, `request_remote_agent_conversation_start`); details §4 |
| **E — policy-only unlocks** | `update_/clear_manual_role_default` (a variant refusing `approval_policy`/`sandbox_mode` writes is the honest split), `repository_settings` (already "flagged-not-flipped for phase review"), `add_conversation_folder_reference` (manifest names the unlock: project-root allowlist confinement) | Owner decision + narrowed variant |
| **F — stays denied** | terminal/PTY, api-key/external-MCP/credential ops, git-origin/gh-auth, destructive cleanup/merge-conflict ops, `deletesEntity` rows, installer surface, `workspace_open` (opens on the wrong machine), attachments-as-written (`writesArbitraryPath`; the bounded endpoint in §1-Gap 3 is the replacement) | — |

---

## 4. Proposed lanes (order matters)

**Lane 0 — front door + dead capability (prereq).** Resolve the UX-1 project-read question (owner call on census R1); wire UX-2 (repoint UI to the registered transcript twins); Bucket A registrations with affordance repoints; Bucket B `AppError` serialization.

**Lane 1 — Continue + Stop (converts "watch" to "manage").**
- *Continue:* clone the intent pattern (`remote_conversation_start_requests` shape: entity + CAS-claim repo with distinctive method names for detector-(b), migration, 2s dispatcher with stale-lease sweep in the always-run startup prefix, `SPAWN_TRIGGERING_STATE_SURFACE` row, `DECLARED_MEMBERSHIPS` row, pinned `role:"user"`). Critical difference: the dispatcher terminal call must be `ChatService::send_message` (hits the `--resume` seam at `chat_service_context.rs:2791-2843`), **not** `AgentConversationStartService::start` (fresh-run semantics). Client: invert today's liveness check — live run ⇒ existing queued path; idle ⇒ persist continue-intent + poll. Must solve the design doc §7 hazard: a persisted-never-dispatched message needs terminal/failure surfacing.
- *Stop:* two coherent options — (i) stop-intent row drained by a host loop (request path never resolves a CLI), or (ii) refactor termination off `pkill` and take an `AUTHORITY_REDUCING_EXEMPTIONS` row at `ui:operate` like `stop_execution` (spec-aligned: stop is authority-reducing and arguably shouldn't need `ui:agent` at all). Either way: fix both silent-swallow sites (`useChatActions.ts:452-459`, `void onStop?.()` at `AgentComposerSurface.tsx:2241`) and add the affordance to the agent-gate map so unavailable renders as a hint, not an enabled button.

**Lane 2 — render fidelity (all spawn-free).** Workspace-review + PR-review context remounts (Gap 5); tool-call detail fail-open fix + registration (Gap 4); host-produced attachment ingress + authorization + client fetch (Gap 3, the largest of the three — needs a small design).

**Lane 3 — diff/branch audit lane (unlocks the review/PR domain).** Hand-trace the 29 `diff_commands` + 4 `plan_branch_commands` past the blanket module reason; split persisted/cached-served reads (registerable) from genuinely-spawning ones (host-side snapshot job read remotely, per the twin/projection precedent). Biggest single unlock, biggest unaudited surface — budget it as its own lane.

**Lane 4 — options & workspace automation.** Seam-split the DB-only flag writes from their incidental spawn probes: `set_agent_conversation_workspace_auto_publish`/`_pr_supervision` (split flag write from `resolve_agent_workspace_pr_automation_target`), `set_agent_conversation_muted`, `update_execution_settings` (write-then-host-drains), `update_agent_conversation_coordination_mode` (non-Ultra variant is spawn-free today), `activate_agent_task_pipeline`/`_plan_direct_implementation` (manifest pre-argues the seam split). Resume family (UX-4) rides the same pattern.

**Lane 5 — Bucket E policy decisions + Bucket C fix batch.**

---

## 5. Owner decisions needed before/while building

1. **Q3 supersession:** the 2026-07-28 ruling said remote *starting* stays host-only in v1; Parts B+C then shipped spawn-free remote start. Record the supersession (or revert) — the tracker ruling and the shipped code currently contradict.
2. **Stop tier:** `ui:operate` via authority-reducing exemption (spec-consistent) vs `ui:agent` via intent row. Recommend (ii)+(i) hybrid: intent row, ledgered authority-reducing, registered at `ui:operate`.
3. **Auto-merge from a phone:** the report's open flag — remote devices can flip `auto_commit`/PR auto-merge under `ui:agent`. Confirm intended, or pin those two behind an explicit host-side toggle.
4. **Role defaults:** accept the narrowed variant (no `approval_policy`/`sandbox_mode` writes) or keep `v1-deferred`.
5. **UX-1/R1:** authorize the spawn-free project-read code change (if still open post-dogfood).
6. **Attachment authorization model** for host-produced files (host-owned rows readable by any paired `ui:read` device vs per-conversation scoping).

## 6. Deploy note

None of this is observable until the Mac Studio host runs a build ≥ `bb84dd53c` (which also carries the dispatcher-placement fix `bb84dd53c` — the start dispatcher previously sat behind the recovery pipeline and could be disabled with it).
