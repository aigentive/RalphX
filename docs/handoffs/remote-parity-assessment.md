# Remote Client/Host Parity — Assessment

**Branch:** `feat/remote-multi-env` (HEAD `e07e22dc4`) · **Date:** 2026-08-04
**Owner rule being measured against:** *"Everything you can do on the host should be doable from the client."*

Verdict in one line: the **read plane is close to done** and the **honesty layer is now good** (Phases 0–5 landed; ~30 prior-review findings verified fixed). What remains is not a long tail of bugs — it is **one missing capability class**: a paired device can *watch* and apply *brakes*, but cannot *approve, resume, publish, or inspect changes*.

Scale today: **247 of 568** ledger rows registered. Refusals: 100 host-denied, 114 spawn-denied, 58 v1-deferred, 24 audit-refused.

---

## 1. Four structural facts that explain every gap

Read these first — they are why "just register it" is usually not the fix.

| # | Fact | Evidence |
|---|---|---|
| 1 | The facade is exhaustive over `generate_handler!`. A command outside that macro **cannot** be registered remotely — this is why all 51 `plugin:*` commands are permanently unreachable. | `remote_server/registry.rs:5` |
| 2 | The process-spawn floor is absolute. Any command whose own closure resolves a CLI path is refused at every scope, even a read. 114 of 568 rows die here. | `ralphx-remote-protocol/src/lib.rs:103` |
| 3 | **`ui:elevated` is never minted.** Pairing grants read+operate; the host can toggle `ui:agent` only. | `remote_environment_service.rs:54`, `remote_device_commands.rs:299` |
| 4 | The fetch remount is read-only by construction (9 read routes) because a proxied fetch carries no `requestId` and would be at-least-once. | `fetch_remount.rs:16-21` |

**Consequence of #3:** the 58 "v1-deferred" rows are not deferred, they are *unreachable* — ticketing, repository settings, role defaults, update channel, folder references. Closing them needs a scope decision, not code.

---

## 2. Two remedies cover most of the 214 refusals

Almost every "impossible" item is either **a read that incidentally shells out** or **a write whose spawn happens downstream**.

- **Cache the shelled value** → the read becomes registerable. (diffs, project capability, MCP catalog, validation summary)
- **Persist an intent for a host-owned dispatcher** → the write becomes registerable. Proven six times already: `request_remote_agent_conversation_start` → `spawn_remote_conversation_start_dispatcher`.

---

## 3. Tier 0 — live defects to fix regardless of parity ambition

| # | Defect | Evidence | Size |
|---|---|---|---|
| 1 | **A remote client can halt the host's scheduler and can never restart it.** `pause`/`stop_execution` are registered; `resume_execution` is spawn-denied. The bar is one ungated toggle, so resume yields "Failed to resume execution" — implying a transient fault. | `ExecutionControlBar.tsx:707-752`, `api/execution.ts:139-145`; gate row `executionResume` exists at `agent-gate.ts:156`, unwired | S |
| 2 | **The host's release channel governs the CLIENT's auto-update.** `get_update_channel` is registered (returns the *host's* channel) and feeds `check({target})`, which runs on the *client*. A host on `nightly` silently pulls the client onto prereleases and auto-relaunches — contradicting the dialog's own "app updates stay with this Mac". | `UpdateChecker.tsx:58-63/172`, `SettingsDialog.tsx:254` | S |
| 3 | **Permission notifications do nothing remotely.** The handler calls the unregistered `get_pending_permissions` and swallows the error. The registered twin `list_pending_permission_gates` already exists and is used elsewhere. | `notificationNavigation.ts:97` vs `PermissionDialog.tsx:287` | S |
| 4 | **Fabricated repository capability.** The remote project twin derives `repository_capability_kind` from `github_pr_enabled`, so PR-mode-off *asserts* "could not be inspected", and PR-mode-on enables a toggle that invokes an unregistered command. Both directions wrong. *(Regression introduced by the Phase 5 fix.)* | `remote_workspace_commands.rs:148-155`, `RepositorySettingsSection.tsx:327-336` | S |
| 5 | **An unanswerable modal pushed to the client.** `recovery:prompt` is relayed, but `resolve_recovery_prompt` is denied and the dialog is ungated; both buttons throw and the prompt is never cleared. | `RecoveryPromptDialog.tsx:46-60` | S |
| 6 | Ungated plan-bar pause/resume/stop; `resumeExecutionIfStopped` half-applied mutation (7 sites); `mark_notification_read` unhandled rejection; `list_workspace_open_targets` swallowing its own gate so the disabled state never renders. | `AgentsArtifactPane.tsx:3038-3057`, `CompletedTaskDetail.tsx:60-62`, `useNotificationToasts.ts:86-88`, `AgentsWorkspaceOpenControl.tsx:114-116` | S each |

---

## 4. Artifacts on the client (the reported bug)

**Root cause is not the artifacts.** Every artifact read is already registered or remounted. The pane is blocked one layer up: `get_agent_conversation_workspace` is spawn-denied, the client calls it with no remote branch, and the hook reads only `.data` — so the rejection becomes "no workspace" and every downstream query is `enabled`-gated off. Nothing fires, nothing errors, empty state renders.

Fix underway: a spawn-free `get_remote_agent_conversation_workspace` twin built on `agent_workspace_response_without_repair_recovery_for_state` (the recovery-free builder the sidebar path already uses remotely), plus a pane-gate fix where the pane gates on `workspace.mode` while the hook that auto-opens it gates on `conversation.agentMode`.

Note: `fetch_remount.rs` already mounts `workspace-review-context` specifically so a paired device can open a review — that remount is currently **dead code** because of this same blocker.

---

## 5. Domain summary

| Domain | State | Principal gap |
|---|---|---|
| Conversations / chat | ~85% — best covered | Queue management, attachments (host endpoint exists, unused), fork/archive/mute/persona |
| Agent modes & workspaces | Read-only | The workspace read twin (§4) unblocks visibility; mutations need intent twins |
| Artifacts & plans | Reads fine | **Plan approval / ideation accept-reject / plan edit — the biggest product gap** |
| Tasks & execution | Board live | Scheduler is a one-way brake (Tier 0 #1) |
| Automations | Honest, ~65% | Run-now, retry-judge, create-draft, setup-edit |
| Diff / workspace changes | **0 of 29** | Worst-degrading domain: silent absence, an impossible Retry button, and a false "No commit history available" |
| Publish & PR / GitHub | Read badges only | Publish/close-PR intent twins; `gh` auth is genuinely host-bound |
| Ticketing | 0 of 20 | Blocked by the unminted scope, not by engineering |
| Settings | 7 of ~45 files remote-aware | Global execution settings shows a value the host never took |
| Notifications | Best-designed surface | Two live bugs (Tier 0 #3, #6) |
| App / system | Startup & terminal correct | **No host-update management exists at all** (§6) |

---

## 6. Owner question: version/update management from the client

**Today:** every updater call routes to the **client's own** binary via the `plugin:` prefix rule — correct for updating the client, and Phase 2 fixed a genuinely dangerous bug where `relaunch()` would have restarted the *host*.

**Managing the host's updates: absent entirely.** No command, route, or UI exists. And it is *unregistrable by construction* — plugin commands bypass `generate_handler!`, over which the facade is exhaustive.

**But it is unimplemented, not impossible.** The updater plugin runs on the host and can be driven by host Rust; only the client-side plugin JS is unproxyable. What it needs:

| Piece | Shape | Size |
|---|---|---|
| Fix the channel bug (Tier 0 #2) | client-only | S |
| Show the host's version/platform post-pairing — already on the wire (`endpoints.rs:245-252`, Hello frame), shown once pre-pairing then discarded | client-only | S |
| Set the host's channel remotely — reclassify `set_update_channel` (audit records the body as a clean single DB write; the refusal is authority-only) | boundary decision + S | S |
| Report host update availability — host polls itself and persists the answer; a spawn-free read twin serves it | (b) | M |
| Trigger host update + relaunch | (b) intent twin + dispatcher | L + decision |

The last one needs an answer to: what happens to the paired session across the restart, and to every running agent.

---

## 7. Ranked roadmap

**Tier 1 — highest parity value**
1. Plan approval + ideation accept/reject + plan edit — three intent twins (M). *The stated reason remote exists.*
2. Resume/start execution + resume/restart task — intent twins (M). Removes the one-way brake.
3. Spawn-free workspace read twin (M) — unblocks the whole Publish/PR/Changes surface's visibility. **In progress.**
4. Diff/workspace changes — host-side snapshot + read twin (L). Converts the largest denied block into reads. Interim honesty pass: S.
5. Attachments — wire the client to the existing `/remote/v1/attachments/upload`; add a host-attachment fetch route (M).
6. `get_project`/`list_projects` cached capability (M) — the open census owner call; fixes Tier 0 #4 properly.

**Tier 2 —** settings sweep (M), global execution twin (S), ticketing short-circuit + linked-chip state (S), automation twins (M), queue twins (M), notification dead-ends (S), host version surfacing (S), publish/close-PR twins (L), mute/fork/archive twins (M).

**Tier 3 — substrate:** unknown-outcome reconciliation as a seam rather than per-call-site (M); degraded/read-only mode reaching surfaces beyond `useAgentGate` (M); extend `GATE_WIRED_FILES` to 7 files whose gates could be deleted today with no CI failure (S).

---

## 8. Decisions only the owner can make

Each widens a deliberate refusal. Engineering cannot proceed without a call.

| # | Refusal | Parity would require | Risk accepted |
|---|---|---|---|
| D1 | `ui:elevated` never minted | Mint a third tier, or reclassify 58 rows individually | A new trust level in pairing. **Biggest single lever** — gates ticketing, repo settings, role defaults, update channel |
| D2 | `deletesEntity` hard-denied (100 rows) | Admit deletes at `agentControl` with dedup + confirmation | A paired phone can destroy host entities. *The confirm dialogs already promise this and then fail* |
| D3 | Ticketing reads `Elevated`/credentials | Split reads (`ui:read`) from writes — no new scope needed | A paired device sees the host's ticket board; the credential never crosses the wire |
| D4 | `set_update_channel` authority-only refusal | Reclassify → `AgentControl` | A paired device changes the host's release train |
| D5 | Automation pause/stop are `AgentControl` | Reclass to `operate` (authority-reducing), as already done for `stop_task`/`pause_task` | Consistency favours widening |
| D6 | Fetch remount read-only | — | **Recommend: do not widen.** Every gap has an invoke-shaped fix |
| D7 | `ptyControl` denies the terminal | Stream PTY over WS + write-back frame | Interactive shell on the host. Highest blast radius here |
| D8 | Client restarting the host app | Intent twin + dispatcher | Terminates every running agent |

---

## 9. Bottom line

Three moves get most of the way to "everything you can do on the host":

1. **The intent-twin pattern, five more times** — plan approval, execution resume, publish, automation run-now, queue. The machinery exists and is proven.
2. **The cached-shell-out pattern, twice** — diff snapshots and project capability. Turns the two largest denied blocks into reads.
3. **One scope decision (D1/D3)** — unlocks ticketing and the settings tail with no new engineering pattern.

The one to fix this week regardless: **a remote client can stop the host's scheduler and has no way to start it again.**
