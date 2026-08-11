> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# RX-Native Team Mode

**Status:** Proposed target architecture

**Drafted:** 2026-07-28

**Related:** `native-agent-delegation-bridge.md` | `delegated-session-model.md` | `.claude/rules/delegation-topology.md` | `.claude/rules/multi-harness.md`

## Decision

Build Team as a durable, conversation-scoped coordination product over RalphX's existing provider-neutral delegation, task, chat, event, and recovery primitives.

Team remains a **capability**, not a mutually exclusive workspace mode:

- Edit + Team means a lead and standing teammates may edit the shared workspace.
- Plan + Team means the lead may use standing teammates for bounded planning/research.
- Tasks + Team means the same roster and board remain visible.
- Switching workspace mode must not destroy the team.

The defining unit is a persistent named teammate that can complete a turn, become idle, receive another assignment, and resume its own delegated conversation. A one-shot `delegate_start` call remains a disposable delegated subroutine, even when the parent conversation has Team enabled.

“Standing” means durable logical identity and conversational continuity, not an OS process kept alive while idle. RalphX should launch provider processes only for active turns and preserve the member through its delegated conversation/session records.

## What Team Must Add

These are the capabilities that distinguish Team from an ordinary coordinator with `delegate_*` tools:

| Team-only capability | Required behavior |
|---|---|
| Durable roster | Stable, human-readable member names and roles survive turns and app restart. |
| Standing workers | A successful member turn ends in `idle`, not removal; later work resumes the member's delegated conversation. |
| Shared coordination board | The coordinator conversation's `AgentTask` ledger is the authoritative team board. |
| Named assignment | The lead assigns a board task to an existing member without replaying session/run/job IDs. |
| Routed messaging | Coordinator, member, and broadcast messages are durable, attributed, addressable, and delivered at safe turn boundaries. |
| Automatic lead wake-up | Results/questions arriving while the lead is idle schedule one deduplicated coordinator continuation; no model polling loop. |
| Team control plane | UI shows roster, state, current assignment, activity, usage, inbox, stop/restart, and member transcript access. |
| Team lifecycle | Teams suspend, resume, drain, close, and recover independently from one delegated job. |
| Mixed-provider roster | Each member resolves its own supported harness/model/effort through the shared runtime registry. |
| Workspace coordination | Parallel mutation has explicit writable ownership and serialized shared validation resources. |

## RalphX-Native Design Choices

Team must extend RalphX's durable, provider-neutral architecture:

| Concern | RalphX decision |
|---|---|
| JSON team registry and filesystem mailboxes | SQLite repositories + durable chat queue + typed events |
| `tmux` / iTerm panes | Existing `AgenticClient` harness adapters and process registry |
| File locks as the primary concurrency primitive | Repository transactions, exact-run settlement, and typed workspace reservations |
| Model-visible team/session bookkeeping | Transport-injected identity; backend resolves team/member/session/run |
| Name as internal identity | Stable IDs internally, unique normalized names for model/UI addressing |
| Separate vendor Team runtime | One RalphX contract across Claude, Codex, and future harnesses |
| Mailbox permission protocol | Existing RalphX permission state/bridge with member attribution |
| Automatic plan approval | Existing user-owned approval policy; no Team-specific bypass |

## Current RalphX Overlap

| Concern | Current state | Gap |
|---|---|---|
| Team selection | `CoordinationMode::RxNativeTeam` persists on `ChatConversation`; the frontend exposes a feature-flagged capability picker. | Selection injects a prompt overlay; it does not create a team aggregate. |
| Team prompt | `application/managed_team/team_prompt_contract.rs` tells the coordinator to use `delegate_start/wait/cancel`. | The live contract describes one-shot delegation, not standing teammates. |
| Canonical authorization | `agents/<agent>/agent.yaml` owns MCP grants and `delegation.allowed_targets`; backend validates caller→target topology. | No Team-specific coordinator/member authorization surface. |
| Delegated context | `DelegatedSession`, `ChatContextType::Delegation`, delegated conversations, provider session attribution, and session reuse exist. | No stable logical member binding or idle/re-task lifecycle. |
| Delegate lifecycle | `native_delegation.rs` creates/reuses a delegated session, launches exact runs, snapshots jobs, and cancels. | Launch orchestration is handler-owned and projected as a job, not a member turn. |
| Task coordination | `AgentTaskService` provides conversation-scoped tasks, blockers, claims, exact assignment reservation, completion/release, and recovery. `team_assign`, `team_roster`, and coordinator-only `team_status` cover member-addressed assignment plus roster/liveness projection. | UI board/roster surface (Slice 4) is not yet built. |
| Recovery | `AgentTaskAssignmentRecoveryService` reconciles reserved/active assignments against durable run/process authority. | No team/member/message reconciliation. |
| Provider neutrality | `AgentClientBundle` resolves Claude/Codex through `AgentHarnessKind`; omitted child settings use delegated-role defaults. `team_add_member` now gates on `ensure_provider_spawn_enabled` at roster admission. | No per-member replacement flow; a provider disabled after a member is added still fails only at next dispatch. |
| Messaging | `TeamMessage`/`TeamMessageDelivery` router, durable delivery projection, coordinator wake batching, `ManagedTeamWakeDispatcher`, and the settlement completion signal (`notify_coordinator_assignment_settled`) exist end-to-end. | `TeamWakeRecipientKind::Member` has no production producer yet (member-targeted automatic wakes are deliberately deferred); UI composer/recipient wiring is not yet built. |
| UI activity | Delegated lifecycle events already carry session/conversation/run/provider attribution and render inline cards. | No roster, idle state, team inbox, aggregate progress, or control panel. |
| Workspace | Conversation workspaces and shared-worktree delegation rules exist. | Writable ownership/resource serialization is policy, not a first-class Team reservation. |

### Current Owning Seams

- Team intent/types: `src-tauri/crates/ralphx-domain/src/entities/team.rs`
- Team capability validation/prompt: `src-tauri/src/application/managed_team/`
- Delegated launch/reuse: `src-tauri/src/http_server/handlers/coordination/native_delegation.rs`
- Delegated context: `src-tauri/crates/ralphx-domain/src/entities/delegated_session.rs`
- Task/assignment authority: `src-tauri/src/application/agent_task_service.rs`
- Assignment recovery: `src-tauri/src/application/agent_task_assignment_recovery.rs`
- Provider registry: `src-tauri/src/application/agent_client_bundle.rs`
- Conversation queue/runtime: `src-tauri/src/application/chat_service/`
- UI event projection: `frontend/src/hooks/useChatEvents.ts`
- Team send scaffolding: `src-tauri/src/commands/unified_chat_commands/mod.rs`, `frontend/src/api/chat.ts`
- Team board reads: `frontend/src/api/agent-tasks.ts`

## Target Architecture

```text
User
  ↕
Coordinator ChatConversation
  ↕
ManagedTeamService
  ├─ TeamSession + TeamMember repositories
  ├─ coordinator AgentTask ledger + exact assignment settlement
  ├─ TeamMessage router → durable chat queues
  ├─ workspace ownership/resource reservations
  └─ team/member lifecycle projection
       ↕
DelegatedSession + delegated ChatConversation
       ↕
ChatService / AgentClientBundle
       ↕
Claude | Codex | future harness

All state changes → backend events → existing conversation event/store seam → Team UI
```

`managed_team` is the owning application seam. It should compose existing services, not call HTTP handlers or introduce a second chat/delegation engine.

The launch orchestration currently embedded in `native_delegation.rs` should be mechanically extracted behind a reusable application-level delegation entry point. Both `delegate_start` and Team member turns then call the same launch path.

## Aggregate And Data Model

### TeamSession

One coordinator conversation may have historical teams but at most one non-closed Team session.

| Field | Purpose |
|---|---|
| `id` | Stable backend identity; never model-supplied. |
| `project_id` | Project containment and provider/settings resolution. |
| `coordinator_conversation_id` | Team namespace and authoritative task board scope. |
| `status` | `active`, `suspending`, `suspended`, `draining`, `closed`, `failed`. |
| `strategy` | Optional `research`, `debate`, `execution` hint; not scheduler authority. |
| `concurrency_limit` | Backend-owned effective limit from runtime config/settings. |
| `budget` / `usage` | Optional effective token/cost limits and aggregate projection. |
| `created_at`, `updated_at`, `closed_at` | Lifecycle audit. |
| `version` | Optimistic concurrency for lifecycle transitions. |

### TeamMember

`TeamMember` is the stable logical teammate; `DelegatedSession` is its current conversational runtime binding; `AgentRun` is one work turn.

| Field | Purpose |
|---|---|
| `id`, `team_id` | Stable membership identity and containment. |
| `name`, `normalized_name` | Unique human/model address; reserve `coordinator` and broadcast aliases. |
| `canonical_agent_name` | Canonical metadata, prompt, MCP grants, and delegation topology source. |
| `role_summary` | Bounded standing responsibility shown to the lead/member. |
| `harness` | Stored member harness; not silently inherited from the lead. |
| `logical_model`, `logical_effort` | Requested/provider-neutral selection when explicit. |
| `delegated_session_id` | Optional reusable delegated conversation binding, created on the first turn. |
| `generation` | Increments only when a runtime binding is explicitly replaced. |
| `status` | `provisioning`, `idle`, `working`, `awaiting_input`, `awaiting_approval`, `stopping`, `suspended`, `failed`, `stopped`. |
| `active_agent_run_id` | Backend projection of the authoritative current turn. |
| `active_assignment_id` | Exact assignment attempt, not just a task owner string. |
| `joined_at`, `last_active_at`, `stopped_at` | Roster/history projection. |
| `last_error` | Typed/sanitized terminal detail. |

Names are addresses, not authority. Transport context identifies the caller; the backend maps it to the exact Team member.

### TeamMessage

Team messages are a durable transport spool and audit projection, not a second chat transcript.

| Field | Purpose |
|---|---|
| `id`, `team_id`, `sequence` | Stable identity and deterministic ordering. |
| `sender_kind`, `sender_member_id` | Transport-derived coordinator/member/system provenance. |
| `target_kind`, `target_member_id` | Coordinator, exact member, or broadcast. |
| `message_kind` | Instruction, result, question, status, control, or approval notice. |
| `content` | Bounded user/model-visible payload. |
| `delivery_state` | Queued, delivered, acknowledged, cancelled, failed. |
| `source_run_id`, `source_assignment_id` | Optional exact authority/attribution. |
| `created_at`, `delivered_at`, `acknowledged_at` | Delivery/recovery evidence. |

Delivery projects the message into the target conversation through the existing durable queue. `team_message_id` is the idempotency key so replay/restart cannot inject it twice.

### Task And Assignment Extensions

Keep the coordinator conversation's `AgentTask` ledger authoritative:

- Do not mirror lead tasks into member-local ledgers.
- Generic member task tools stay private to the delegated session.
- Members see only the exact caller assignment through the existing trusted assignment contract.
- Add optional `team_id` / `team_member_id` to assignment records so multiple named members may use the same canonical agent safely.
- Member completion is intent; task completion occurs only after exact current-run settlement.
- Failure, cancellation, release, orphan recovery, or stale completion reopens the exact board task.

The UI may show the whole coordinator board. This does not grant every member model the right to enumerate or mutate it.

### Workspace Reservations

Parallel mutation requires first-class ownership rather than prompt-only promises:

| Reservation | Behavior |
|---|---|
| Writable paths | Canonicalized, workspace-contained prefixes/globs attached to an active assignment. |
| Generated outputs | Included in overlap checks; two tasks cannot own the same generated artifact. |
| Resource locks | Named resources such as Rust validation, package build, migration numbering, or shared dev server. |
| Validation lane | Exactly one heavyweight validator for the shared worktree. |

Path reservations must be validated at the filesystem sink. Missing scope may be allowed for read-only work; write-capable assignments without a safe scope must warn or fail according to the active policy.

## Lifecycle

### Team

```text
active → suspending → suspended → active
active | suspended → draining → closed
any non-closed state → failed → suspended | draining
```

- Selecting Team ensures a non-closed `TeamSession` for the conversation.
- Workspace mode changes do not alter Team state.
- Switching the capability to Solo while members are active must be explicit:
  - suspend members and preserve roster/history, or
  - drain and close the team.
- A Project conversation already in `RxNativeTeam` cannot leave Team mode through an ordinary send: `ChatService` rejects a send that tries to flip `coordination_mode` away from `RxNativeTeam`. Leaving requires the capability-change action, which stages the Team exit above; the send path is not a second, uncoordinated exit route.
- Conversation archive/delete drains the team before final cleanup.
- Closed teams remain historical records; runtime processes and active reservations do not.

### Member

```text
provisioning → idle → working → idle
working → awaiting_input | awaiting_approval → working
idle | working → stopping → stopped
idle | failed → suspended → idle
working → failed → idle | suspended | stopped
```

Critical rule: a terminal `AgentRun` is not automatically a terminal `TeamMember`.

### Assignment

Use the existing exact-run state machine:

```text
reserved → active → completion_requested → completed
                 ↘ release_requested → released
reserved | active → failed | cancelled
```

Only the current member generation + delegated session + agent run may settle the current assignment. Late output from an older run may be retained as history but cannot change board/member authority.

## Model-Facing Tool Surface

Ordinary delegation remains available for disposable work. Team adds a small member-oriented surface when `coordination_mode=rx_native_team`.

### Coordinator tools

| Tool | Model supplies | Backend owns |
|---|---|---|
| `team_add_member` | Unique name, canonical agent, standing role; optional supported harness/model/effort | Team/session/member IDs, topology validation, provider readiness pre-flight, runtime binding |
| `team_list` | — | Idle-only assignment-target projection |
| `team_assign` | Member name, task ref, bounded instruction, optional typed work scope | Exact assignment reservation, run creation/resume, current-attempt binding |
| `team_send_message` | Member name or broadcast, content | Sender identity, routing, delivery, wake-up |
| `team_stop_member` | Member name, reason | Drain deadline, cancellation, assignment reopening, process cleanup |
| `team_roster` | — | Bounded name/role/status projection scoped to the caller's Team; coordinator or member authority |
| `team_status` | — | Coordinator-only liveness join (running state, last activity, latest run), capped at 32 entries |

Whole-Team suspend/close is **not** a model-facing tool: it is a user-driven capability change that routes through `exit_team`, which stages the exit, drains members, and settles assignments. `TEAM_TOOL_NAMES` in `team-tool-policy.ts` is the exact live list.

The `team_coordinator` profile now grants seven tools (previously five): `team_roster` and `team_status` were added so the coordinator has a bounded read surface for "who exists" and "is anyone stuck," not just member management.

### Tool Selection

| Tool | Use it to |
|---|---|
| `team_list` | Find idle assignment targets |
| `team_roster` | See who exists — name/role/status, coordinator or member authority |
| `team_status` | See if anyone is stuck — coordinator-only liveness join, capped at 32 entries, degrades a missing delegated session to a null `agent_state` |

### Member tools

Reuse trusted assignment tools where possible:

- `get_delegate_assignment`
- `complete_delegate_assignment`
- `release_delegate_assignment`
- `team_send_message`
- `team_roster` — bounded roster/status view (member authority is accepted; `team_status` is not)

Only the coordinator may add, assign, stop, suspend, or close members. A member may use ordinary `delegate_start` only when its canonical allowlist permits it; that child is an ephemeral delegate, not a roster member.

### Tool Rules

- Tool schemas never accept team/session/member/run/job IDs.
- Caller and team identity come from transport/runtime context and are validated backend-side.
- Human member names resolve within the caller's exact active Team only.
- Team tools are hidden outside RX Team and hidden from unauthorized roles.
- Canonical `delegation.allowed_targets` remains the target authorization source.
- Tool grants, prompt contracts, HTTP types/routes, authorization, generated MCP output, and tests change together.

## Runtime Flows

### Add A Member

1. Lead calls `team_add_member`.
2. Backend resolves the caller's active Team from the conversation/run context.
3. Validate unique name, coordinator authority, canonical target allowlist, provider readiness, and concurrency. Provider readiness is a pre-flight gate (`ensure_provider_spawn_enabled`), resolved with the same unset-harness default as `team_assign`: a disabled provider fails admission with `409` and a Settings > Harness > Providers pointer instead of admitting the member and deferring the failure to first dispatch.
4. Create an idle `TeamMember`; do not start a provider process merely to populate the roster.
5. The first assignment creates its delegated-session binding through the shared delegation launch seam and injects:
   - stable member name and standing role
   - Team contract
   - exact workspace/context envelope
   - the exact assigned caller task, not unassigned board contents
6. Launch failure → member `failed` and assignment reopened with no leaked ownership.

### Assign And Re-task

1. Lead creates/refines a coordinator-ledger task.
2. Lead calls `team_assign(member_name, task_ref, instruction, scope)`.
3. Backend transaction reserves the exact task, validates workspace/resource conflicts, and binds the member.
4. If the member is idle, continue its delegated conversation using stored provider lineage.
5. If busy, reject or queue according to explicit policy; never silently replace the active assignment.
6. Member requests completion/release.
7. Current run termination settles the exact attempt.
8. Successful settlement → task `done`, member `idle`, result queued to lead through the completion-signal contract below.

### Completion Signal And Wake Dispatch

Settlement notifies the coordinator instead of relying on polling:

1. Member assignment settlement (`settle_member_assignment`) reaches a terminal status and the member returns to `idle`.
2. The service sends a `System`-authored `TeamMessage` to the coordinator (`notify_coordinator_assignment_settled`) with a deterministic idempotency key on `(assignment_id, terminal_status)`; a replayed settlement returns the original envelope instead of a second wake. Notification failure is logged and swallowed — settlement authority never depends on it.
3. Delivery projection queues a `TeamWakeBatch` row (`Queued`) for the coordinator.
4. `ManagedTeamService::claim_next_actionable_wake_batch` durably CASes the batch to `Launching` and preallocates the coordinator `AgentRunId` binding.
5. `ManagedTeamWakeDispatcher::dispatch_pending_wakes` claims and dispatches the batch. Dispatch is spawned in the background from the settlement path (`tokio::spawn`), never awaited, so a slow or failing dispatch cannot block settlement.
6. If the coordinator is idle, the dispatcher sends a hidden `resume_in_place` turn on the coordinator conversation, bound to the preallocated run, and settles the batch to `Settled` on success or `Failed` (with retry, bounded by `park_wake_retry_max`) on send failure.
7. If the coordinator already has an active run, the dispatcher cancels the batch instead of leaving it at `Launching` forever: the settlement message is already sitting in the coordinator's durable queue and drains at the coordinator's next turn boundary, so a second wake would only produce a duplicate turn.

### Message Delivery

- Running target → durable queue; deliver at the existing safe turn boundary.
- Idle member target → queue, then start one continuation run for all currently pending messages.
- Running lead → queue Team events/messages without interrupting its current model turn.
- Idle lead → schedule one deduplicated continuation containing typed Team-origin envelopes.
- Pure idle/status notifications update UI only; they do not wake the lead model.
- Coalesce wake-ups and enforce configured concurrency/budget limits to prevent automatic-turn loops. Only `WakeBatch` run bindings whose status is `Planned | Launching | Running` count against `TeamSession.automatic_wake_limit`; terminal (`Settled | Failed | Cancelled`) bindings persist for the row's history but do not count, so the budget recovers as batches finish instead of decaying to zero. Exhausting the live-binding budget emits `team:needs_attention` so a stalled Team is diagnosable from the UI, not only from a queue inspection.
- Broadcast → create one recipient delivery per active member under one source message.
- Member-to-member → allowed only inside the same Team; coordinator remains lifecycle/task authority.

Plain member output must not masquerade as a user message. The prompt composer should render typed, escaped Team-origin context with sender and assignment provenance.

### Stop And Close

1. Mark member/team `stopping` or `draining`; reject new assignments.
2. Deliver a typed shutdown control notice when a run can finish cooperatively.
3. Wait only for a backend-configured grace period.
4. Cancel remaining process/run through existing stop/cancel seams.
5. Reopen unresolved assignments and release reservations.
6. Mark members stopped, close Team, keep audit/transcript records, remove ephemeral runtime state.

Members do not veto a user/lead shutdown.

## Provider-Neutral Contract

- Each fresh member resolves: explicit supported override → effective Delegated Subagent role/provider setting → harness fallback.
- Parent harness is lineage, not a child default.
- A member's stored harness remains stable across normal re-tasking.
- Idle members retain no provider process; continuity is the stored delegated conversation/provider session.
- Provider session continuation stays behind `DelegatedSession`; the model never supplies it.
- Harness/model replacement is explicit and increments `TeamMember.generation`.
- A provider failure never silently moves a member to another harness; UI/lead chooses retry, suspend, or replace.
- Claude/Codex differences remain inside `AgenticClient`, harness registries, runtime builders, and capability descriptors.
- Adding a third harness extends registries/adapters; Team code must not add `claude | codex` branches.

Mixed teams are a required first-class case:

```text
Codex lead → Claude member
Claude lead → Codex member
Codex lead → Codex + Claude members
```

## Permissions And Approvals

Reuse the existing permission request/response infrastructure:

- Resolve `agent_run_id` → active Team member for UI attribution.
- Show member name, canonical agent, assignment, command/tool, and requested scope.
- Deliver the user's decision only to the exact requesting run.
- Persist policy updates at the existing owner level; do not create Team mailbox permission state.
- A stale/replaced member run cannot consume a newer decision.

Plan approval follows the active workspace/user policy. Team must not auto-approve a member plan merely because the coordinator exists.

## UI Product Surface

Keep the existing capability picker. Add a Team control panel to the active conversation rather than a new top-level workspace mode.

### Minimum Panel

- Team state: active/suspended/draining/failed
- Aggregate progress: open/active/done tasks
- Roster rows:
  - member name + canonical role
  - harness/model
  - idle/working/waiting/failed state
  - current task
  - last activity
  - usage
  - message, stop, retry/open transcript actions
- Team activity/inbox with attributed questions/results/errors
- Task board using the existing conversation task API
- Conflict/approval banners

### Composer

- Recipient selector: coordinator/default, exact member, broadcast.
- `TeamMessageTarget` becomes functional and is resolved server-side.
- Messages sent directly to a member do not start a parallel lead run.
- Team-origin messages render with member badges and cannot be confused with user content.

### Existing Delegate Cards

Keep inline delegated cards as immutable member-turn history. Add the logical `team_member_id/name` projection so multiple runs reconcile under the same roster member.

### Interaction Performance

- Paint the Team panel shell before lazy imports, data fetches, transcript hydration, or process actions.
- Warm the panel on safe intent/idle.
- Keep open/close interaction decoupled from member runtime startup/teardown.
- Use Playwright-first visual and interaction coverage.

## Recovery And False-Success Rules

Startup reconciliation derives Team state from durable authority:

1. Load non-closed teams and members.
2. Reconcile current assignment using existing exact run settlement.
3. Verify delegated session, active conversation, current run, and process registry agree.
4. Running and exact process present → retain `working`.
5. Terminal current run → settle once, then `idle` or `failed`.
6. Missing/stale/ambiguous run → fail closed, reopen task, release reservations, mark member `failed`/`suspended`.
7. Requeue undelivered Team messages idempotently.
8. Never auto-resume write work after crash unless current-run authority and workspace reservations remain valid.

Required stale-state attacks:

- older member generation completes after replacement
- older run reports completion after a new assignment starts
- message delivery commits but event emission fails
- event emits before delivery commit
- assignment completion intent exists but the run fails
- provider session resumes under the wrong member/team
- active task has no exact assignment
- UI receives lifecycle events from an inactive parent run
- app exits during team drain
- repository read fails and would otherwise look like an empty roster/board

## Implementation Slices

Each slice is a vertical, testable increment. Do not land a prompt/UI fiction before backend authority exists.

### Slice 0 — Contract And Extraction

- Freeze Team invariants, state enums, tool names, and transport-owned identity rules.
- Mechanically extract reusable delegated launch/reuse orchestration from the oversized HTTP handler into the existing application seam.
- Keep `delegate_start/wait/cancel` behavior and schemas compatible.
- Update architecture docs whose “current state” predates `ChatContextType::Delegation`.

**Proof:** existing focused delegation/task-assignment tests stay green; extraction produces no behavior change.

### Slice 1 — Durable Team And Roster

- Add `TeamSession` / `TeamMember` domain entities, repositories, SQLite migration, memory repos, and shared AppState wiring.
- Add validated lifecycle transitions and startup reconciliation skeleton.
- Make Team capability create/load a session; add read-only roster command/API.
- Keep feature flag off by default.

**Proof:** one non-closed team per coordinator conversation, unique normalized member names, dual-AppState repo identity, restart round-trip.

### Slice 2 — Standing Member Runtime

- Add `team_add_member`, `team_list`, `team_assign`, `team_stop_member`.
- Bind stable members to reusable delegated sessions.
- Re-task by starting a new run in the same delegated conversation.
- Extend exact assignment rows with optional Team/member identity.
- Project run terminal state to member `idle` without closing membership.

**Proof:** two sequential tasks use one member/delegated conversation, distinct exact runs, preserved provider lineage, stale first-run completion cannot settle the second task.

### Slice 3 — Durable Messaging And Lead Wake-Up

- Add Team message repository/router and idempotent chat-queue projection.
- Activate `TeamMessageTarget`.
- Add `team_send_message` for coordinator/member surfaces.
- Add safe-boundary delivery, idle-member continuation, and deduplicated lead continuation. Done: `ManagedTeamWakeDispatcher` is the consumer for claimed wake batches, and `notify_coordinator_assignment_settled` is the settlement completion signal — see Runtime Flows > Completion Signal And Wake Dispatch.
- Add typed Team message rendering/events (UI still open, see Slice 4).

**Proof:** busy/idle delivery, broadcast fan-out, cross-Team spoof rejection, restart replay without duplicate model injection, no user-role confusion.

### Slice 4 — Team UI

- Add lazy Team panel shell, roster projection, board, activity/inbox, member actions, transcript navigation, and composer recipient selection.
- Reconcile existing delegated lifecycle cards by member identity.
- Attribute permission requests and usage to members.

**Proof:** user-visible React tests + Playwright flows for add/assign/idle/re-task/message/stop/recovery; stale events cannot overwrite current state.

### Slice 5 — Workspace Safety, Budgets, And Hardening

- Add typed writable/resource reservations and backend overlap enforcement.
- Add Team concurrency/cost/token controls and aggregate usage.
- Complete suspend/drain/close and crash recovery.
- Run mixed-provider capability tests and remove the feature flag only after parity.

**Proof:** conflicting mutation scopes reject, one validator lease, budget gate fails closed, Claude↔Codex matrix, orphan recovery, clean drain.

## Test Strategy

| Layer | Required coverage |
|---|---|
| Domain | Team/member transitions, name normalization, current-generation authority, message state, reservation overlap. |
| Repository | SQLite + memory parity, uniqueness, optimistic versioning, idempotent delivery, forward-only migration. |
| Application | Add/assign/re-task/message/stop/recovery through production service entry paths. |
| Delegation | Existing job behavior unchanged; Team reuse preserves canonical agent/harness/session binding. |
| Assignment | Wrong/stale run cannot complete; failure/cancel/release reopens; suppressed side effects have absence assertions. |
| MCP | Allowed/denied agent matrix, mode-gated visibility, schema parity, no caller-supplied orchestration IDs. |
| Provider | Claude/Codex lead/member combinations, provider readiness, stale session recovery, explicit replacement. |
| Frontend | Roster/board/inbox behavior, recipient routing, permissions, stale event rejection, accessible controls/tooltips. |
| Visual | Scoped Playwright Team panel and composer flows; no Native Tauri Computer Use unless explicitly requested. |
| Recovery | Crash at every reservation/run/message/drain boundary; false-success review against stale attempts and fail-open reads. |

Use focused tests only during implementation. Any Rust test invocation requires the repository's mandated final `cargo clean`.

## Compatibility And Migration

- Keep `CoordinationMode::RxNativeTeam` and `TeamIntent` transport compatibility.
- `CapabilityIntent` remains the provider-neutral direction; avoid new Team-named compatibility aliases.
- Existing conversations with `rx_native_team` and no `TeamSession` lazily create one on the next Team-capable action.
- Existing `delegate_*` calls and historical cards remain valid and are not retroactively roster members.
- Additive Team fields on assignment/event payloads remain optional until all consumers migrate.
- Legacy Claude-only data remains derivable during the documented multi-harness migration window.
- Feature flag stays off until durable lifecycle + recovery + minimum UI land together.

## Non-Goals

- Recreating Claude Code's vendor Team implementation.
- Treating every one-shot delegate as a roster member.
- Nested Team rosters or member-created teammates.
- Separate worktrees per member in the first version.
- A second task ledger or mirrored task authority.
- Model-managed polling, job IDs, timestamps, rescue flags, wait knobs, or session IDs.
- Silent provider failover.
- Auto-approving permissions or plans.

## Definition Of Done

RX Team is real when this scenario works across app restart and mixed providers:

1. User enables Team inside an Edit conversation.
2. Lead adds two named members with different canonical roles/harnesses.
3. Lead creates and assigns disjoint tasks.
4. Both members work concurrently under enforced workspace ownership.
5. One member asks a question; it reaches the lead with correct attribution.
6. The other completes; exact task settlement moves it to idle.
7. Lead re-tasks that same member; its delegated conversation/provider context continues.
8. User messages a member directly from the composer.
9. App restarts; roster, board, messages, and safe member states recover without false completion.
10. Lead drains/closes the team; unresolved work reopens and no process/reservation leaks remain.

Until that contract exists, `rx_native_team` is accurately described as a delegation-oriented capability overlay, not a standing multi-agent team runtime.
