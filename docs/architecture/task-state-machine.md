# Task State Machine

**Audience: RalphX maintainers.** This is internals reference, not a user guide. For task-managed delivery from a user's perspective, see `docs/user-guides/`.

Every task in RalphX is governed by a state machine. It defines exactly which states a task can be in, which transitions are allowed, what happens automatically, and where human decisions are required. This document describes the full scope of that system — what each state means, how tasks move through them, and how each situation is handled.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| How does a task start? | Move it to **Ready** (Schedule). The system auto-schedules it into **Executing** when a slot is available. |
| What starts the AI agents? | Entering **Executing**, **QaRefining**, **QaTesting**, **Reviewing**, or **Merging** automatically spawns the corresponding agent. |
| Who approves a task? | You do — the AI reviewer brings the task to **ReviewPassed**, then you click **Approve**. |
| What is a "transient" state? | A state the system passes through automatically without waiting for you (e.g., **Approved** → **PendingMerge** happens instantly). |
| My task is stuck — what do I do? | Check the state. If it's **QaFailed**, **Escalated**, **MergeConflict**, or **MergeIncomplete**, the task needs your attention. Use the action buttons in the task detail view. |
| Can I run a task again after it's done? | Yes — **Retry** from **Merged**, **Failed**, **Cancelled**, or **Stopped** sends it back to **Ready**. |
| What is "Paused" vs "Stopped"? | **Paused** is resumable (agent is killed but task retains its state). **Stopped** is a terminal state — requires manual Retry to re-run. |
| Does QA always run? | No — QA is per-project. If disabled, tasks go directly from **Executing** → **PendingReview** (skipping all QA states). |
| What blocks a task? | A dependency on another unfinished task, or an agent signaling it needs human input. The task enters **Blocked** and waits. |

---

## Table of Contents

1. [Overview](#overview)
2. [The 24 States](#the-24-states)
3. [State Diagram](#state-diagram)
4. [State Transition Reference](#state-transition-reference)
5. [Auto-Transitions](#auto-transitions-chained-states)
6. [Happy Path Flows](#happy-path-flows)
7. [AI Agents and Their Role](#ai-agents-and-their-role)
8. [Human-in-the-Loop Controls](#human-in-the-loop-controls)
9. [Pause, Resume, and Stop](#pause-resume-and-stop)
10. [Blocked and Unblock Mechanics](#blocked-and-unblock-mechanics)
11. [Side Effects Reference](#side-effects-reference)
12. [Guard Conditions](#guard-conditions)
13. [Troubleshooting](#troubleshooting)
14. [Configuration Reference](#configuration-reference)

---

## Overview

The task state machine is the **core guardrail** of RalphX's autonomous AI development workflow. Rather than letting agents do anything at any time, every task progresses through a fixed set of states. Each state change is:

- **Explicit** — only defined transitions are allowed; the system rejects invalid transitions
- **Logged** — every transition is recorded in the state history for full audit trails
- **Side-effected** — entering a state triggers specific actions (spawn an agent, create a git branch, notify you)
- **Guarded** — conditions must be met before certain transitions fire (e.g., no dirty working tree, no unresolved blockers)

This structure means you always know where a task is, what is happening to it, and what you need to do. Agents cannot silently skip states or jump ahead — the machine enforces the full lifecycle.

### The Lifecycle at a Glance

```
Ideation → Backlog → Ready → Executing → QA (optional) → Review → Approved → Merge → Done
```

Each phase may loop: a failed QA sends the task back to execution, a revision request sends it back through another execution cycle, and a failed merge triggers an agent or manual resolution.

---

## The 24 States

### Idle States

These states have no active agent. The task is waiting for scheduling, a human action, or a dependency.

| State | What's Happening | What You See | Action Needed? |
|-------|-----------------|--------------|----------------|
| **Backlog** | Task created but not ready to start | Gray badge in Kanban/Graph | Optional — schedule when ready |
| **Ready** | Waiting for an execution slot | Blue badge; may say "Queued" | No — system picks it up automatically |
| **Blocked** | Waiting on a dependency or human input | Orange badge with blocker info | See the blocker reason; resolve if it needs you |

### Active States (Agent Running)

An AI agent is currently working. You can view live agent activity in the task's chat panel.

| State | Agent | What's Happening |
|-------|-------|-----------------|
| **Executing** | Worker | Main development work — writing code, editing files, running commands |
| **QaRefining** | QA Refiner | Refines the test plan based on the actual implementation |
| **QaTesting** | QA Executor | Runs browser-based tests against the implementation |
| **Reviewing** | Reviewer | Code review — checks implementation quality, correctness, and completeness |
| **ReExecuting** | Worker | Second (or later) execution pass, incorporating reviewer feedback |
| **Merging** | Merger | AI-assisted conflict resolution (triggered when auto-merge fails) |

### Waiting-for-Human States

The system has reached a decision point that requires your input.

| State | What Happened | What You Need to Do |
|-------|--------------|---------------------|
| **QaFailed** | QA tests did not pass | Review the failures and choose: **Retry** (sends to RevisionNeeded) or **Skip QA** (sends to review) |
| **ReviewPassed** | AI reviewer approved the work | Review the summary and click **Approve** to proceed, or **Request Changes** to revise |
| **Escalated** | AI reviewer couldn't decide (too risky or ambiguous) | Read the escalation reason and make a judgment call: **Approve** or **Request Changes** |
| **MergeConflict** | Merger agent could not resolve conflicts | Resolve conflicts manually, then click **Retry Merge** |
| **MergeIncomplete** | Merge failed for a non-conflict reason (git error, timeout) | Check the error details and click **Retry Merge** |

### Transient States

States the system passes through automatically — you will see them briefly in the UI as the machine chains to the next state.

| State | Auto-Transitions To | Why |
|-------|---------------------|-----|
| **QaPassed** | PendingReview | QA succeeded — immediately queue for review |
| **PendingReview** | Reviewing | Review queued — immediately spawn reviewer agent |
| **RevisionNeeded** | ReExecuting | Revision required — immediately spawn worker for revision pass |
| **Approved** | PendingMerge | Task approved — immediately start merge workflow |

### Merge States

| State | What's Happening | What You See | Action Needed? |
|-------|-----------------|--------------|----------------|
| **PendingMerge** | Auto-merge in progress (fast path) | Progress timeline with phases | No — wait for result |
| **Merging** | AI merger agent resolving conflicts | Agent activity in merge chat | No — agent is working |
| **MergeConflict** | Agent couldn't resolve conflicts | "Needs Attention" badge | Resolve manually or retry |
| **MergeIncomplete** | Merge attempt failed (non-conflict) | "Needs Attention" badge with error | Click **Retry Merge** |
| **Merged** | Code is on the target branch | Green checkmark; commit SHA shown | None — task is complete |

### Terminal States

Tasks in terminal states are done. You can restart any of them with **Retry**.

| State | How It Ended | Restartable? |
|-------|-------------|--------------|
| **Merged** | Successfully merged to target branch | Yes — Retry → Ready |
| **Failed** | Unrecoverable error during execution or merge | Yes — Retry → Ready |
| **Cancelled** | Intentionally abandoned | Yes — Retry → Ready |
| **Stopped** | Force-stopped by user | Yes — Retry → Ready |

### Suspended State

| State | What's Happening | How to Resume |
|-------|-----------------|---------------|
| **Paused** | Agent was killed; task retains its pre-pause state | Use **Resume** — restores agent and respawns it in pre-pause state |

---

## State Diagram

```
                    ┌──────────┐
                    │ BACKLOG  │
                    └────┬─────┘
           Schedule │    │ Cancel
                    ▼    ▼
                    ┌──────────┐
              ┌─────│  READY   │◄────────────────────────────────────────┐
              │     └────┬─────┘                                         │
              │          │ [auto, no blockers]    BlockerDetected         │
  BlockerDet. │          ▼                 ┌──────────────────┐          │
              │     ┌──────────────────────────────────────┐  │          │
              │     │ <<execution>>                         │  │          │
              │     │  ┌───────────┐  ExecutionComplete    │  │          │
              │     │  │ EXECUTING │──────────────────────►│  │          │
              │     │  └──────┬────┘                       │  │          │
              │     │ Failed  │ NeedsHumanInput             │  │          │
              │     └─────────┼─────────────────────────────┘  │          │
              │               │                   │             │          │
              │               ▼                   ▼             │          │
              │          ┌────────┐          ┌─────────┐        │          │
              │          │ FAILED │          │ BLOCKED │        │ Blocked  │
              │          └────┬───┘          └────┬────┘◄───────┘Resolved  │
              │          Retry│            Blockers│                        │
              │               │           Resolved │                        │
              │               │                   ▼                         │
              │               │         (back to READY above)               │
              │               │                                             │
              │     ┌─────────┴─────────────────────────────────────────┐  │
              │     │ <<qa>> [qa_enabled only]                           │  │
              │     │  ┌────────────┐  Complete   ┌───────────┐         │  │
              │     │  │ QA_REFINING│────────────►│ QA_TESTING│         │  │
              │     │  └────────────┘             └─────┬─────┘         │  │
              │     │                            Passed │ Failed         │  │
              │     │                       ┌───────────┼──────────┐    │  │
              │     │                       ▼           ▼          │    │  │
              │     │               ┌──────────┐  ┌──────────┐     │    │  │
              │     │               │ QA_PASSED│  │ QA_FAILED│     │    │  │
              │     │               └────┬─────┘  └────┬─────┘     │    │  │
              │     │              (auto)│         Retry│SkipQa     │    │  │
              │     └───────────────────┼──────────────┼───────────┘    │  │
              │                         │              │                  │  │
              │     ┌───────────────────▼──────────────▼──────────────┐  │  │
              │     │ <<review>>                                        │  │  │
              │     │  ┌───────────────┐ (auto) ┌───────────┐          │  │  │
              │     │  │ PENDING_REVIEW│────────►│ REVIEWING │          │  │  │
              │     │  └───────────────┘         └─────┬─────┘          │  │  │
              │     │                      Approved │   │ Escalate       │  │  │
              │     │              NeedsChanges │   ▼   ▼               │  │  │
              │     │                           │ ┌───────────┐ ┌──────┐ │  │  │
              │     │                           │ │REVIEW_PASS│ │ESCAL.│ │  │  │
              │     │                           │ └─────┬─────┘ └──┬───┘ │  │  │
              │     │                           │ Human │Approve    │     │  │  │
              │     │  ┌────────────────┐       │       ▼           ▼     │  │  │
              │     │  │REVISION_NEEDED │◄──────┘   ┌──────────┐         │  │  │
              │     │  └───────┬────────┘    (auto)  │ APPROVED │         │  │  │
              │     │         (auto)▼                └────┬─────┘         │  │  │
              │     │  ┌──────────────┐                   │(auto)         │  │  │
              │     │  │ RE_EXECUTING │                   │               │  │  │
              │     │  └─────────────┘                   │               │  │  │
              │     │  (loops back to REVIEWING)          │               │  │  │
              │     └────────────────────────────────────┼───────────────┘  │  │
              │                                          ▼                    │  │
              │               ┌─────────────────────────────────┐            │  │
              │               │ <<merge>>                         │            │  │
              │               │  ┌──────────────┐ (fast path)   │            │  │
              │               │  │ PENDING_MERGE│───────────────┼──► MERGED ─┘  │
              │               │  └──────┬───────┘               │     │ Retry   │
              │               │         │ (conflicts)            │     └─────────┘
              │               │         ▼                        │
              │               │  ┌────────┐                      │
              │               │  │MERGING │──────────────────────┼──► MERGED
              │               │  └───┬────┘                      │
              │               │      │ Agent fails               │
              │               │      ▼                           │
              │               │ ┌────────────────┐              │
              │               │ │ MERGE_CONFLICT /│◄────retry───┤
              │               │ │ MERGE_INCOMPLETE│             │
              │               │ └────────────────┘              │
              │               └─────────────────────────────────┘
              │
              │  ┌────────────┐     ┌───────────┐
              └─►│ CANCELLED  │     │  STOPPED  │
                 └────────────┘     └───────────┘
                    Retry ─────────────────────────────────► READY (above)
```

---

## State Transition Reference

The complete list of all valid transitions. Only these transitions are permitted — all others are rejected.

| From | Valid Next States |
|------|-----------------|
| `backlog` | `ready`, `cancelled` |
| `ready` | `executing`, `blocked`, `cancelled` |
| `blocked` | `ready`, `cancelled` |
| `executing` | `qa_refining`, `pending_review`, `failed`, `blocked`, `stopped`, `paused`, `cancelled` |
| `qa_refining` | `qa_testing`, `stopped`, `paused`, `cancelled` |
| `qa_testing` | `qa_passed`, `qa_failed`, `stopped`, `paused`, `cancelled` |
| `qa_passed` | `pending_review` |
| `qa_failed` | `revision_needed`, `pending_review` |
| `pending_review` | `reviewing` |
| `reviewing` | `review_passed`, `revision_needed`, `escalated`, `stopped`, `paused`, `cancelled` |
| `review_passed` | `approved`, `revision_needed` |
| `escalated` | `approved`, `revision_needed` |
| `revision_needed` | `re_executing`, `cancelled` |
| `re_executing` | `pending_review`, `failed`, `blocked`, `stopped`, `paused` |
| `approved` | `pending_merge`, `ready` |
| `pending_merge` | `merged`, `merging`, `cancelled` |
| `merging` | `merged`, `merge_conflict`, `merge_incomplete`, `stopped`, `paused` |
| `merge_incomplete` | `pending_merge`, `merged`, `stopped`, `paused`, `cancelled` |
| `merge_conflict` | `pending_merge`, `merged`, `stopped`, `paused`, `cancelled` |
| `merged` | `ready` |
| `failed` | `ready` |
| `cancelled` | `ready` |
| `stopped` | `ready` |
| `paused` | resumes to `pause_reason.previous_status` (the state the task was in when paused; can include merge states like `pending_merge`, `merge_conflict`, or `merge_incomplete`) |

### Transition Triggers

Each transition is driven by one of three sources:

| Source | Examples |
|--------|---------|
| **User action** | Schedule, Cancel, Pause, Stop, Approve, Request Changes, Retry, Skip QA, Force Approve |
| **Agent signal** | ExecutionComplete, ExecutionFailed, QaTestsComplete, ReviewComplete, MergeAgentFailed |
| **System (auto)** | BlockersResolved, StartReview, StartRevision, StartMerge, MergeComplete, ConflictResolved |

---

## Auto-Transitions (Chained States)

Some states are entered and immediately exited by the system without waiting for any action. These still appear briefly in the UI and are always recorded in the state history.

| Reached State | Auto-Transitions To | Trigger |
|---------------|---------------------|---------|
| `qa_passed` | `pending_review` | QA passed — start review phase |
| `pending_review` | `reviewing` | Review queued — spawn reviewer agent |
| `revision_needed` | `re_executing` | Revision requested — spawn worker for rework |
| `approved` | `pending_merge` | Task approved — begin merge workflow |

**Why these exist:** They record the intermediate state in history (useful for audit and time-travel) while keeping the system responsive. You won't need to click through them.

---

## Happy Path Flows

### Without QA

The simplest path when QA testing is disabled for the project:

```
Backlog → Ready → Executing → PendingReview → (auto) Reviewing
       → ReviewPassed → [You: Approve] → Approved → (auto) PendingMerge
       → Merged (fast auto-merge) or Merging → Merged
```

### With QA Enabled

When QA is on, the system inserts a full test cycle after execution:

```
Backlog → Ready → Executing → QaRefining → QaTesting
       → QaPassed → (auto) PendingReview → (auto) Reviewing
       → ReviewPassed → [You: Approve] → Approved → (auto) PendingMerge → Merged
```

### Revision Loop

When the reviewer finds issues, the task re-executes with feedback and loops back through review:

```
Reviewing → RevisionNeeded → (auto) ReExecuting
          → PendingReview → (auto) Reviewing
          → ReviewPassed → [You: Approve] → ...
```

This loop can repeat multiple times. Each cycle is tracked in the state history.

### Escalation Path

When the reviewer cannot make a decision (ambiguous, high-risk change):

```
Reviewing → Escalated → [You: Approve or Request Changes]
          → Approved → ... (merge)
            OR
          → RevisionNeeded → (auto) ReExecuting → ...
```

### QA Failure and Retry

```
QaTesting → QaFailed → [You: Retry] → RevisionNeeded → (auto) ReExecuting
          → (completes) → QaRefining → QaTesting → (QaPassed or loops again)
```

Alternatively, skip QA entirely:

```
QaFailed → [You: Skip QA] → PendingReview → (auto) Reviewing
```

---

## AI Agents and Their Role

RalphX uses different specialized agents at each active phase of the lifecycle. Each agent is spawned automatically on state entry and runs until it signals completion (or the system detects a stall/timeout).

| State | Agent | Model | What It Does |
|-------|-------|-------|-------------|
| **Executing** | `ralphx-execution-worker` | Sonnet | Reads the task proposal and plan, decomposes into sub-scopes, delegates to coder sub-agents (max 3 concurrent), steps through the implementation |
| **ReExecuting** | `ralphx-execution-worker` | Sonnet | Same as Executing, but first fetches review notes and open issues to incorporate reviewer feedback |
| **QaRefining** | `ralphx-qa-refiner` | Sonnet | Generates acceptance criteria and a structured test plan (may have run in background during Ready) |
| **QaTesting** | `ralphx-qa-executor` | Sonnet | Executes browser-based tests using agent-browser; reports pass/fail |
| **Reviewing** | `ralphx-execution-reviewer` | Sonnet | Reviews code quality, correctness, test coverage; calls `complete_review` with approved/needs_changes/escalate |
| **Merging** | `ralphx-execution-merger` | Opus | Resolves git conflicts in worktrees; reads conflict files, resolves markers, verifies no remaining conflicts |

### Background Agent: QA Prep

When a task enters **Ready**, the system starts `ralphx-qa-prep` in the background. This agent generates acceptance criteria and test steps ahead of time so that when the task reaches **QaRefining**, the prep work is already done. The QaRefining state waits for the background prep to complete before handing off to the tester.

### Agent Communication with the State Machine

Agents communicate results back to the state machine via MCP tools. The state machine only advances based on explicit signals — an agent cannot force a state transition without calling the right tool:

| Agent Signal | MCP Tool Called | Transition Triggered |
|-------------|----------------|---------------------|
| Execution done | `complete_step` (final step) | `executing` → `qa_refining` or `pending_review` |
| Execution failed | `fail_step` | `executing` → `failed` |
| Needs human input | `add_task_note` with block flag | `executing` → `blocked` |
| QA refinement done | (auto on agent exit) | `qa_refining` → `qa_testing` |
| QA tests passed | (result from qa-executor) | `qa_testing` → `qa_passed` |
| QA tests failed | (result from qa-executor) | `qa_testing` → `qa_failed` |
| Review approved | `complete_review { outcome: "approved" }` | `reviewing` → `review_passed` |
| Review needs changes | `complete_review { outcome: "needs_changes" }` | `reviewing` → `revision_needed` |
| Review escalated | `complete_review { outcome: "escalate" }` | `reviewing` → `escalated` |
| Merge conflict unresolvable | `report_conflict` | `merging` → `merge_conflict` |
| Merge agent error | `report_incomplete` | `merging` → `merge_incomplete` |

---

## Human-in-the-Loop Controls

RalphX is designed to run on its own, but you are always in control. These are the explicit intervention points.

### Approval Gate (ReviewPassed / Escalated)

The AI reviewer never directly approves a task into the merge pipeline. It brings the task to **ReviewPassed** (or **Escalated** if it needs your judgment) and waits for you. You then decide:

| Action | From State | Result |
|--------|-----------|--------|
| **Approve** | ReviewPassed, Escalated | → Approved → (auto) PendingMerge |
| **Request Changes** | ReviewPassed, Escalated | → RevisionNeeded → (auto) ReExecuting |

### QA Override (QaFailed)

When QA fails, you have two choices without needing to wait for the agent to re-run:

| Action | Result |
|--------|--------|
| **Retry** | → RevisionNeeded → ReExecuting (agent fixes, re-runs QA) |
| **Skip QA** | → PendingReview → Reviewing (skip further QA entirely) |

### Scheduling Controls

| Action | From States | Result |
|--------|-----------|--------|
| **Schedule** | Backlog | → Ready |
| **Cancel** | Backlog, Ready, Blocked, RevisionNeeded, most non-terminal states | → Cancelled |
| **Pause** | Executing, ReExecuting, QaRefining, QaTesting, Reviewing, Merging | → Paused (agent killed, resumable) |
| **Stop** | Same as Pause | → Stopped (terminal, requires Retry to restart) |
| **Retry** | Failed, Cancelled, Stopped, Merged | → Ready |

### Global Pause

You can pause **all** active tasks in a project at once. This sends every agent-active task to **Paused**. Tasks in **Ready**, **Blocked**, and other non-agent states are unaffected. When you resume globally, all paused tasks are restored in order (respecting concurrency limits).

---

## Pause, Resume, and Stop

### Pause vs. Stop

| | Pause | Stop |
|---|-------|------|
| **Result state** | `paused` | `stopped` |
| **Terminal?** | No — resumable | Yes — requires Retry |
| **Agent** | Killed immediately | Killed immediately |
| **Pre-pause state** | Stored in metadata | Not stored (discard) |
| **Auto-resume on provider error?** | Yes (if `ProviderError` pause type) | No |
| **Re-run path** | Resume → respawn agent in pre-pause state | Retry → Ready → Executing |

### How Resume Works

When you resume a paused task (or a paused project):

1. Read the pre-pause state from `task.metadata["pause_reason"].previous_status`
2. Fall back to `status_history` if metadata is absent
3. Verify the restore state is an agent-active state (skip otherwise)
4. Check capacity: current running tasks + tasks being restored < max concurrent
5. Transition the task back to its pre-pause state
6. Clear the pause metadata and respawn the agent

> **Warning:** Stopped tasks are not restored on resume. They require a manual **Retry** action which sends them back to **Ready** — the full execution cycle restarts from the beginning.

### Pause Reason Types

| Type | Who Sets It | Auto-Resumable? |
|------|------------|-----------------|
| `UserInitiated` | You (manual pause) | No |
| `ProviderError` | System (API rate limit / outage) | Yes (after `retry_after` elapses) |

### Paused State and Dependent Tasks

Tasks in **Paused** do **not** block their dependents from being unblocked. If the task they depend on is **Merged**, dependents are unblocked normally. The dependency system operates independently of the pause system.

---

## Blocked and Unblock Mechanics

### How a Task Gets Blocked

| Trigger | From State | Details |
|---------|-----------|---------|
| Dependency on unfinished task | `ready` | Auto-detected by scheduler at scheduling time |
| Agent signals human input needed | `executing` | Agent calls NeedsHumanInput with a reason message |
| Explicit blocker detection | `ready`, `re_executing` | `BlockerDetected { blocker_id }` system event |

### How a Task Gets Unblocked

| Trigger | What Happens |
|---------|-------------|
| Dependency task reaches **Merged** | `dependency_manager.unblock_dependents()` is called on `merged` entry (and some merge completion paths); dependents transition `blocked` → `ready` |
| Dependency task is **Cancelled** or **Stopped** | Dependents remain `blocked`; resolve blockers (for example via `BlockersResolved`) or update dependents manually as needed |
| All blockers resolved | `BlockersResolved` event → `blocked` → `ready` |
| You manually resolve the human input request | Clear the blocker in task detail; system fires `BlockersResolved` |

### During Global Pause

Blocked tasks are **immune to pause** — they have no running agent, so there is nothing to kill. When a dependency merges during a global pause, the dependent task transitions `blocked` → `ready` normally. However, the scheduler will not pick up newly-ready tasks until the project is resumed.

---

## Side Effects Reference

### On State Entry

When a task enters one of these states, the following happens automatically:

| State | Side Effects |
|-------|-------------|
| **ready** | Spawn `ralphx-qa-prep` agent in background (if QA enabled). After `scheduler.ready_settle_ms` (default 300ms; configurable via `RALPHX_SCHEDULER_READY_SETTLE_MS`) → schedule next ready task. |
| **executing** | Create task git branch. Create worktree (Worktree mode). Spawn **worker** agent. |
| **re_executing** | Checkout task branch (Local mode). Spawn **worker** agent with revision context (review notes + open issues). |
| **qa_refining** | Wait for qa-prep background agent to finish (if still running). Spawn `qa-refiner` agent. |
| **qa_testing** | Spawn `qa-tester` agent. |
| **qa_passed** | Emit `qa_passed` event. |
| **qa_failed** | Emit `qa_failed` event. Notify user. |
| **pending_review** | Start AI review via ReviewStarter. Emit `review:update` event. |
| **reviewing** | Checkout task branch (Local mode). Spawn **reviewer** agent. |
| **review_passed** | Emit `review:ai_approved`. Notify user "Please review and approve". |
| **escalated** | Emit `review:escalated`. Notify user "Please review and decide". |
| **approved** | Emit `task_completed` event. |
| **pending_merge** | Run `attempt_programmatic_merge()` — attempts a fast, no-agent merge. |
| **merging** | Spawn **merger** agent (opus model). |
| **merged** | Call `dependency_manager.unblock_dependents()` — unblocks any tasks waiting on this one. |
| **failed** | Notify user. Emit `task_failed` event. |
| **cancelled** | No additional state-entry event. Cancellation is signaled by the command layer via `task:cancelled` before transitioning into this state. |

### On State Exit

When a task leaves one of these states, cleanup happens automatically:

| Leaving State | Exit Effects |
|--------------|-------------|
| Any agent-active state (`executing`, `re_executing`, `qa_refining`, `qa_testing`, `reviewing`, `merging`) | Decrement running agent count. Emit `execution:status_changed`. Trigger `try_schedule_ready_tasks()` to fill the freed slot. |
| **executing**, **re_executing** | `auto_commit_on_execution_done()` — commits any uncommitted changes with message `feat: {task_title}`. |
| **reviewing** | Emit `review:state_exited`. |

---

## Guard Conditions

Guards are pre-conditions checked before certain transitions fire. If a guard fails, the transition is rejected and an error is returned.

| Guard | Checked When | What It Prevents |
|-------|-------------|-----------------|
| **Dirty working tree** (Local mode) | `on_enter(executing)` | Starting a task when you have uncommitted changes in the repo. Clean your working tree first. |
| **QA enabled flag** | `ExecutionComplete` dispatch | Routes to `qa_refining` (QA on) vs `pending_review` (QA off). Not a hard block — determines routing only. |
| **Task branch already exists** | `on_enter(executing)` | Only creates a branch on the *first* execution. Re-entry (e.g., after pause) skips branch creation. |
| **Running task in Local mode** | Task scheduler | Prevents a second task from entering `executing` when another is already running. Local mode is single-task only. |
| **Self-transition** | `can_transition_to()` | No state can transition to itself. |
| **Terminal state guard** | All terminal states | Terminal states (`merged`, `failed`, `cancelled`, `stopped`) cannot transition to other states except via **Retry** → `ready`. |

---

## Troubleshooting

### Task is stuck in Executing (no activity)

**What it means:** The worker agent may have stalled, crashed, or lost its connection.

**What to do:**
1. Open the task's chat panel — check for recent agent activity.
2. If the last message is old (no activity for several minutes), click **Stop** to terminate the agent, then **Retry** to re-run from Ready.
3. If you see an error about a dirty working tree (Local mode), commit or stash your changes and then retry.

### Task is in QaFailed

**What it means:** The QA agent ran tests and they failed. The failures are recorded with details.

**What to do:**
1. Open the task detail and review the QA failure report.
2. Click **Retry** if you want the worker to fix the failures (sends through RevisionNeeded → ReExecuting → full QA cycle).
3. Click **Skip QA** if the failures are known, acceptable, or you want to proceed directly to review.

### Task is in Escalated

**What it means:** The AI reviewer determined the task is too risky or ambiguous to auto-approve. It needs a human decision.

**What to do:**
1. Open the task detail and read the escalation reason.
2. Review the changes yourself (diff viewer is available).
3. Click **Approve** if you're satisfied, or **Request Changes** to send it back for revision.

### Task is in Blocked and won't unblock

**What it means:** Either a dependency task hasn't finished yet, or an agent requested human input.

**What to do:**
1. Check the blocker list in the task detail. Each blocker shows its type and the blocking task (if dependency-based).
2. If it's a dependency: wait for the blocking task to reach **Merged** or **Cancelled** — unblocking is automatic.
3. If it's a human input request: read the agent's message, take the requested action in your codebase or environment, then mark the blocker as resolved.

### Task loops between ReviewPassed and RevisionNeeded

**What it means:** The worker keeps making changes that the reviewer rejects, or the reviewer and worker disagree.

**What to do:**
1. Read the review notes in the task detail to understand what the reviewer is asking for.
2. If the issue persists after several rounds, use **Force Approve** to bypass the review (use sparingly — this overrides the AI guardrail).
3. Alternatively, **Cancel** the task and create a new one with clearer requirements.

### Task is in MergeConflict

**What it means:** The merger agent could not automatically resolve the git conflicts between this task's branch and the target branch.

**What to do:**
1. Open the task's merge panel — it shows each conflicted file with an inline diff viewer.
2. Resolve conflicts manually in your IDE (or use the inline viewer).
3. Stage and commit the resolved files.
4. Click **Retry Merge** — the system verifies no remaining conflict markers and completes the merge.

### Task is in MergeIncomplete

**What it means:** The merge attempt failed for a non-conflict reason (git error, timeout, missing branch, worktree issue).

**What to do:**
1. Check the error message in the task detail.
2. Click **Retry Merge** — most transient errors are resolved automatically on retry.
3. If a branch is missing: the task branch may need to be recreated (retry execution).
4. If the issue persists across multiple retries, check the configuration reference for timeout settings.

### "Dirty working tree" error in Local mode

**What it means:** You have uncommitted changes in your project directory and a task is trying to start execution (which requires switching branches).

**What to do:**
1. In your terminal: `git stash` or `git commit -am "wip"` to clean the working tree.
2. Click **Retry** on the task — it will pick up from Ready and start executing.

### Task in Ready but never starts executing

**What it means:** Either the concurrency limit is reached (another task is running in Local mode), or execution is globally paused.

**What to do:**
1. Check if another task is currently Executing. In **Local mode**, only one task runs at a time — wait for it to finish.
2. Check if the project is globally paused. If so, click **Resume** to start the queue.
3. Check the project's max concurrent tasks setting if you're in Worktree mode.

---

## Configuration Reference

### Project Settings (Kanban / Project Settings panel)

| Setting | Description | Default |
|---------|-------------|---------|
| `qa_enabled` | Whether tasks go through the QA phase (QaRefining → QaTesting) | `false` |
| `git_mode` | `Local` (one task at a time) or `Worktree` (parallel, isolated) | `Local` |
| `base_branch` | Target branch for standalone task merges | `main` |
| `max_concurrent_tasks` | Max tasks that can be executing simultaneously (Worktree mode) | `10` |
| `use_feature_branches` | Whether plan tasks use an isolated plan feature branch | `true` |
| `worktree_parent_directory` | Parent directory for task worktrees | `~/ralphx-worktrees` |

### State Machine Timing (ralphx.yaml)

| Setting | Description |
|---------|-------------|
| `qa_prep_timeout_secs` | How long to wait for the background qa-prep agent before proceeding |
| `execution_timeout_secs` | Inactivity timeout before a stalled worker is detected |
| `review_timeout_secs` | Inactivity timeout before a stalled reviewer is detected |
| `scheduler.ready_settle_ms` | Delay after entering Ready before the scheduler settles the Ready set (default: 300ms) |

### Merge Settings

See the [Merge Pipeline User Guide](./merge.md) for full configuration options covering merge strategies, validation modes, retry budgets, and timing.

---

## State Category Summary

| Category | States | Key Trait |
|----------|--------|-----------|
| Idle | `backlog`, `ready`, `blocked` | No agent running; waiting for scheduling or human/system event |
| Active (agent) | `executing`, `qa_refining`, `qa_testing`, `reviewing`, `re_executing`, `merging` | Agent is running; task is consuming a concurrency slot |
| Transient | `qa_passed`, `pending_review`, `revision_needed`, `approved` | Auto-advances; system passes through without waiting |
| Waiting (human) | `qa_failed`, `review_passed`, `escalated`, `merge_conflict`, `merge_incomplete` | Human decision required to advance |
| Merge | `pending_merge`, `merging`, `merge_conflict`, `merge_incomplete`, `merged` | Post-approval merge pipeline (see Merge Pipeline User Guide) |
| Terminal | `merged`, `failed`, `cancelled`, `stopped` | Done; all are restartable via Retry |
| Suspended | `paused` | Temporarily halted; resumable to pre-pause state |

---

## See Also

- [Agent Orchestration](agent-orchestration.md)
- [Execution Pipeline](execution.md)
- [Merge Pipeline](merge.md)
