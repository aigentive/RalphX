---
paths:
  - "src-tauri/src/domain/state_machine/**"
  - "src-tauri/crates/ralphx-domain/src/entities/status.rs"
  - "src-tauri/src/application/task_transition_service.rs"
  - "src-tauri/src/application/task_scheduler_service/**"
---

# Task State Machine

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

**Required Context:** task-git-branching.md | task-execution-agents.md

---

## 28 Internal Statuses

| # | Status | Category | Terminal? |
|---|--------|----------|-----------|
| 1 | `backlog` | Idle | — |
| 2 | `ready` | Idle | — |
| 3 | `blocked` | Idle | — |
| 4 | `executing` | Active (agent) | — |
| 5 | `qa_refining` | Active (agent) | — |
| 6 | `qa_testing` | Active (agent) | — |
| 7 | `qa_passed` | Transient | — |
| 8 | `qa_failed` | Waiting (human) | — |
| 9 | `pending_review` | Transient | — |
| 10 | `reviewing` | Active (agent) | — |
| 11 | `review_passed` | Waiting (human) | — |
| 12 | `escalated` | Waiting (human) | — |
| 13 | `revision_needed` | Transient | — |
| 14 | `re_executing` | Active (agent) | — |
| 15 | `approved` | Transient | — |
| 16 | `pending_merge` | Transient | — |
| 17 | `merging` | Active (agent) | — |
| 18 | `waiting_on_pr` | Waiting (GitHub) | — |
| 19 | `updating_plan_branch` | Active (system) | — |
| 20 | `updating_task_branch` | Active (system) | — |
| 21 | `branch_update_blocked` | Waiting (human) | — |
| 22 | `merge_incomplete` | Waiting (human) | YES |
| 23 | `merge_conflict` | Waiting (human) | — |
| 24 | `merged` | Done | YES |
| 25 | `failed` | Done | YES |
| 26 | `cancelled` | Done | YES |
| 27 | `paused` | Suspended | — |
| 28 | `stopped` | Done | YES |

**Transient:** Auto-transitions immediately to next state (no UI dwell time).
**Terminal semantics:** `is_terminal()` = Merged | Failed | Cancelled | Stopped | MergeIncomplete — but only Merged | Cancelled satisfy dependencies (`is_dependency_satisfied()`).
**Branch-update states:** `updating_plan_branch` / `updating_task_branch` / `branch_update_blocked` belong to the durable branch-update workflow (`branch_update_workflow.rs` / `branch_update_executor.rs`); the statig machine handles only pause/stop/cancel for them — completion/blocked exits are executor-owned CAS writes with lease/fencing authority.
**`waiting_on_pr`:** entered by the PR-mode merge engine (`side_effects/merge_attempt/pr_mode.rs`, canonical engine write); GitHub is the merge authority while polling.

---

## State Transition Table

Source of truth: `InternalStatus::valid_transitions()` (`crates/ralphx-domain/src/entities/status.rs`). Regenerate from that function, not the statig machine (which is a subset).

| From | Valid Targets |
|------|--------------|
| `backlog` | `ready`, `cancelled` |
| `ready` | `executing`, `blocked`, `pending_merge`, `cancelled` |
| `blocked` | `ready`, `cancelled` |
| `executing` | `qa_refining`, `pending_review`, `updating_plan_branch`, `updating_task_branch`, `failed`, `blocked`, `stopped`, `paused` |
| `qa_refining` | `qa_testing`, `stopped`, `paused` |
| `qa_testing` | `qa_passed`, `qa_failed`, `stopped`, `paused` |
| `qa_passed` | `pending_review` |
| `qa_failed` | `revision_needed` |
| `pending_review` | `reviewing` |
| `reviewing` | `review_passed`, `revision_needed`, `escalated`, `updating_plan_branch`, `updating_task_branch`, `stopped`, `paused` |
| `review_passed` | `approved`, `revision_needed` |
| `escalated` | `approved`, `revision_needed`, `pending_review` |
| `revision_needed` | `re_executing`, `cancelled` |
| `re_executing` | `pending_review`, `updating_plan_branch`, `updating_task_branch`, `failed`, `blocked`, `stopped`, `paused` |
| `approved` | `pending_merge`, `ready` |
| `pending_merge` | `merged`, `merging`, `waiting_on_pr`, `updating_plan_branch`, `updating_task_branch`, `merge_incomplete`, `stopped`, `paused`, `cancelled` |
| `merging` | `merged`, `waiting_on_pr`, `updating_plan_branch`, `merge_conflict`, `merge_incomplete`, `stopped`, `paused`, `cancelled` |
| `waiting_on_pr` | `merged`, `merge_incomplete`, `pending_merge`, `updating_plan_branch`, `stopped`, `paused`, `cancelled` |
| `merge_incomplete` | `pending_merge`, `merging`, `waiting_on_pr`, `merged`, `stopped`, `paused`, `cancelled` |
| `merge_conflict` | `pending_merge`, `merging`, `merged`, `stopped`, `paused`, `cancelled` |
| `updating_plan_branch` | `updating_task_branch`, `branch_update_blocked`, `executing`, `re_executing`, `reviewing`, `pending_merge`, `waiting_on_pr`, `merged`, `failed`, `escalated`, `merge_incomplete`, `stopped`, `paused`, `cancelled` |
| `updating_task_branch` | `branch_update_blocked`, `executing`, `re_executing`, `reviewing`, `pending_merge`, `waiting_on_pr`, `failed`, `escalated`, `merge_incomplete`, `stopped`, `paused`, `cancelled` |
| `branch_update_blocked` | `updating_plan_branch`, `updating_task_branch`, `stopped`, `cancelled` |
| `merged` | `ready` |
| `failed` | `ready` |
| `cancelled` | `ready` |
| `stopped` | `ready` |
| `paused` | `executing`, `re_executing`, `qa_refining`, `qa_testing`, `reviewing`, `merging`, `waiting_on_pr`, `updating_plan_branch`, `updating_task_branch` |

Branch-update completion/blocked exits go through the branch-update executor's CAS writes, not statig machine events.

---

## Transition API Contract

| Path | Rule |
|------|------|
| Normal workflow transitions | Use validated `TaskTransitionService::transition_task*()` or `handler.handle_transition(...)` |
| Corrective / repair-only jumps | Use explicit `transition_task_corrective()` / `apply_corrective_transition()` |
| Canonical engine writes | Direct `internal_status` persistence is allowed only inside transition-handler / merge-engine / branch-update-executor core paths that also persist history/events |
| Forbidden | ❌ Direct repo/DB `internal_status` mutation for already-active live workflow tasks |

---

## Auto-Transitions (Immediate, Chained)

| Reached State | Auto-Transitions To | Why |
|---------------|---------------------|-----|
| `qa_passed` | `pending_review` | QA done → start review |
| `pending_review` | `reviewing` | Spawn reviewer agent immediately |
| `revision_needed` | `re_executing` | Spawn worker agent for revision immediately |
| `approved` | `pending_merge` | Start merge workflow immediately |

These are persisted as intermediate states but the system chains through them in one operation.

---

## Side Effects on State Entry

| State | Side Effects |
|-------|-------------|
| **ready** | Spawn `qa-prep` agent (background, if QA enabled). After 600ms delay → `try_schedule_ready_tasks()` |
| **executing** | Create worktree dir + task branch. Persist `task_branch`/`worktree_path`. Spawn **worker** agent |
| **qa_refining** | Wait for qa-prep if needed. Spawn `qa-refiner` agent |
| **qa_testing** | Spawn `qa-tester` agent |
| **qa_passed** | Emit `qa_passed` event |
| **qa_failed** | Emit `qa_failed` event. Notify user |
| **pending_review** | Start AI review via `ReviewStarter`. Emit `review:update` event |
| **reviewing** | Spawn **reviewer** agent |
| **review_passed** | Emit `review:ai_approved`. Notify user "Please review and approve" when `require_human_review=true`; otherwise the review-complete path may continue immediately to `approved` |
| **escalated** | Emit `review:escalated`. Notify user "Please review and decide" |
| **re_executing** | Spawn **worker** agent with revision context |
| **approved** | Emit `task_completed` event |
| **pending_merge** | Run `attempt_programmatic_merge()` (see task-git-branching.md) |
| **merging** | Spawn **merger** agent |
| **merged** | `dependency_manager.unblock_dependents()` → auto-unblock waiting tasks |

## Side Effects on State Exit

| From State | Exit Effects |
|------------|-------------|
| Agent-active states (`executing`, `re_executing`, `qa_refining`, `qa_testing`, `reviewing`, `merging`) | Decrement `running_count`. Emit `execution:status_changed`. `try_schedule_ready_tasks()` (slot freed) |
| `executing`, `re_executing` | `auto_commit_on_execution_done()` — commits any uncommitted changes as `feat: {task_title}` |
| `reviewing` | Emit `review:state_exited` |

---

## Happy Path Flows

### Without QA
```
Backlog → Ready → Executing → PendingReview → (auto) Reviewing
→ ReviewPassed → (HumanApprove | auto-approve when `require_human_review=false`) Approved → (auto) PendingMerge
→ Merged (fast) or Merging → Merged
```

### With QA
```
Backlog → Ready → Executing → QaRefining → QaTesting
→ QaPassed → (auto) PendingReview → (auto) Reviewing
→ ReviewPassed → (HumanApprove | auto-approve when `require_human_review=false`) Approved → (auto) PendingMerge → Merged
```

### Revision Loop
```
Reviewing → RevisionNeeded → (auto) ReExecuting → PendingReview
→ (auto) Reviewing → ... (repeat until pass or fail/cancel)
```

---

## TaskEvent → Transition Dispatch

### User Events

| Event | From → To |
|-------|-----------|
| `Schedule` | `backlog` → `ready` |
| `StartExecution` | `ready` → `executing` |
| `Cancel` | (most states) → `cancelled` |
| `Pause` | agent-active states → `paused` (non-terminal, resumable) |
| `Stop` | agent-active states → `stopped` (terminal, requires manual restart) |
| `HumanApprove` | `review_passed`/`escalated` → `approved` |
| `HumanRequestChanges` | `review_passed`/`escalated` → `revision_needed` |
| `ForceApprove` | (override) → `approved` |
| `Retry` | `failed`/`cancelled`/`stopped`/`merged` → `ready`; `waiting_on_pr`/`merge_incomplete`/`merge_conflict` → `pending_merge` |
| `SkipQa` | `qa_failed` → `pending_review` |

### Agent Events

| Event | From → To |
|-------|-----------|
| `ExecutionComplete` | `executing`/`re_executing` → `qa_refining` (QA on) or `pending_review` (QA off) |
| `ExecutionFailed` | `executing`/`re_executing` → `failed` |
| `NeedsHumanInput` | `executing` → `blocked` |
| `QaRefinementComplete` | `qa_refining` → `qa_testing` |
| `QaTestsComplete(true)` | `qa_testing` → `qa_passed` |
| `QaTestsComplete(false)` | `qa_testing` → `qa_failed` |
| `ReviewComplete(approved)` | `reviewing` → `review_passed` |
| `ReviewComplete(!approved)` | `reviewing` → `revision_needed` |
| `MergeAgentFailed` | `merging` → `merge_conflict` |
| `MergeAgentError` | `merging`/`waiting_on_pr` → `merge_incomplete` |

### System Events

| Event | From → To |
|-------|-----------|
| `StartReview` | `pending_review` → `reviewing` |
| `StartRevision` | `revision_needed` → `re_executing` |
| `StartMerge` | `approved` → `pending_merge` |
| `MergeComplete` | `pending_merge`/`merging`/`waiting_on_pr` → `merged` |
| `MergeConflict` | `pending_merge`/`merge_incomplete` → `merging` |
| `ConflictResolved` | `merge_conflict`/`merge_incomplete` → `merged` |
| `BlockersResolved` | `blocked` → `ready` |
| `BlockerDetected` | `ready`/`re_executing` → `blocked` |
| `PlanBranchUpdateRequired` / `TaskBranchUpdateRequired` | executing/re_executing/reviewing/pending_merge/merging/waiting_on_pr → `updating_plan_branch` / `updating_task_branch` |

---

## Guard Conditions

| Guard | Location | Effect |
|-------|----------|--------|
| `context.qa_enabled` | `ExecutionComplete` dispatch | Routes to `qa_refining` vs `pending_review` |
| `task.task_branch.is_none()` | `on_enter(Executing)` | Only creates branch on first execution (skip on re-entry) |
| Self-transition | `can_transition_to()` | No state can transition to itself |

---

## Branch Freshness Conflict Routing

When `ensure_branches_fresh()` detects stale branches at execution/review entry:

- `BranchFreshnessConflict` is an `AppError` handled by `TaskTransitionService::handle_branch_freshness_conflict` via **corrective transitions** — not a statig machine event to `merging`. Tasks already in `updating_plan_branch`/`updating_task_branch` hand off to the branch-update workflow instead.
- Reviewing-origin conflicts WITHOUT merge-conflict evidence park in `pending_review`; with evidence (or non-review origins) they route to `merging`.
- **Caps:** `FRESHNESS_RETRY_LIMIT = 5` with one auto-reset + extended cooldown; second cap → `ExecutionBlocked`; review-origin count ≥5 → `failed` via corrective transition.
- **Return path** keys on `plan_update_conflict` metadata (`freshness_return_route`), not a `branch_freshness_conflict` bool; origin values include `executing` | `re_executing` | `reviewing` | `waiting_on_pr` | `pr_branch_publication`.
- **Metadata:** `FreshnessMetadata` (`transition_handler/freshness.rs`) — `freshness_origin_state`, `freshness_conflict_count`, `freshness_backoff_until`, `freshness_auto_reset_count`.

**Review gate invariant:** Even when human review is disabled, AI approval still uses `reviewing → review_passed → approved`; do not reintroduce direct `reviewing → approved`.

---

## Pause / Resume / Unblock Behavior

### Pause (`TaskEvent::Pause`)

| Aspect | Detail |
|--------|--------|
| **Which states** | All `AGENT_ACTIVE_STATUSES` (9): `executing`, `re_executing`, `qa_refining`, `qa_testing`, `reviewing`, `merging`, `pending_merge`, `updating_plan_branch`, `updating_task_branch` |
| **Which states NOT** | Idle (`backlog`, `ready`, `blocked`), terminal (`failed`, `cancelled`, `stopped`), transient states |
| **Result** | Task → `paused`. Agents killed via `running_agent_registry.stop_all()` |
| **Metadata** | `PauseReason` written to `task.metadata["pause_reason"]` before transition. Variants: `UserInitiated` (scope: "global"/"task") | `ProviderError` (auto-resumable) |
| **running_count** | Decremented by `on_exit` handlers in `TransitionHandler` — no manual reset needed |
| **`Paused` is NOT terminal** | `is_terminal()` → false. `is_paused()` → true. `is_active()` → false |

### Resume (`resume_execution` command)

The state machine does **not** handle resume events directly. Resume is command-layer logic:

1. Query all tasks in `Paused` status for the project
2. Read `task.metadata["pause_reason"].previous_status`
3. Fall back to `status_history` if metadata absent
4. Validate `restore_status` is in `AGENT_ACTIVE_STATUSES` — skip otherwise
5. Check capacity: `running_count + local_restore_count < max_concurrent` (cannot use `can_start_task()` — pause flag still set during restoration loop)
6. `transition_task(task_id, restore_status)` → re-enters the pre-pause state
7. Clear `pause_reason` from metadata, then call `execute_entry_actions()` → respawns agent

```
Paused → [command reads metadata] → executing (or re_executing / qa_refining / …)
```

⚠️ `Stopped` tasks are NOT restored by resume. They require manual `Retry` → `ready` → re-execution.

### Unblock (`BlockersResolved` system event)

| Aspect | Detail |
|--------|--------|
| **Event** | `BlockerDetected { blocker_id }` → `ready`/`re_executing` → `blocked` |
| **Unblock trigger** | `BlockersResolved` → `blocked` → `ready` |
| **Auto-trigger** | `merged` entry calls `dependency_manager.unblock_dependents()` — resolves tasks waiting on the just-merged task |
| **Context** | `TaskContext.blockers: Vec<Blocker>` tracks all blockers. `resolve_all_blockers()` clears the vec on `BlockersResolved` |
| **Human input block** | `NeedsHumanInput { reason }` → `executing` → `blocked`. Stored as `Blocker::human_input(reason)` in context |
| **Cancel** | `Blocked` tasks can be `Cancel`led directly |

### `PauseReason` Metadata (key: `"pause_reason"`)

```rust
// UserInitiated: written by pause_execution command
PauseReason::UserInitiated { previous_status, paused_at, scope }
// ProviderError: written by chat_service_handlers on API error (auto-resumable)
PauseReason::ProviderError { category, message, retry_after, previous_status, paused_at, auto_resumable, resume_attempts }
```

- `from_task_metadata(metadata)` → reads (with backward-compat for legacy `provider_error` key)
- `write_to_task_metadata(existing)` → merges into existing JSON, preserving other keys
- `clear_from_task_metadata(existing)` → removes `pause_reason` + legacy `provider_error` key
- `previous_status()` → returns the pre-pause status string (parse with `InternalStatus::from_str`)

---

## Pause vs Blocked Behavior

### Which Statuses Are Affected by Pause

| Status | Affected by global pause? | Why |
|--------|--------------------------|-----|
| `executing`, `re_executing`, `qa_refining`, `qa_testing`, `reviewing`, `merging`, `pending_merge`, `updating_plan_branch`, `updating_task_branch` | YES → transitions to `paused` | In `AGENT_ACTIVE_STATUSES` |
| `blocked` | NO — immune to pause | Not in `AGENT_ACTIVE_STATUSES` — no agent to kill |
| `ready` | NO — stays queued | No agent running; scheduler gated by `is_paused()` flag |
| `paused` | — | Result of pausing an agent-active task |
| `stopped` | — | Terminal; NOT restored on resume (requires manual `Retry`) |

### Dependency Resolution During Pause

`Blocked → Ready` proceeds **normally during pause** — it is a non-spawning transition:
- `on_enter(Merged)` → `unblock_dependents()` fires regardless of pause state
- Newly-Ready tasks **stay in `ready`** until resume — the scheduler checks `execution_state.is_paused()` before spawning any agent

### Resume Ordering Guarantee

```
1. Find all Paused tasks for the project
2. For each (pause flag still SET):
   - Check capacity: running_count + local_restore_count < max_concurrent
     (cannot use can_start_task() — returns false while pause flag is set)
   - Read pre-pause status from metadata (status_history fallback)
   - transition_task → re-enter pre-pause state → respawn agent
   - Increment local_restore_count
3. execution_state.resume() — clear pause flag
4. try_schedule_ready_tasks() — pick up waiting Ready tasks
```

**Why this ordering:** clearing the pause flag before step 2 would allow the scheduler to race against the restoration loop, consuming capacity slots meant for paused tasks.
