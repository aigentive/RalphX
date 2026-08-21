# Task Execution Pipeline

**Audience: RalphX maintainers.** This is internals reference, not a user guide. For task-managed delivery from a user's perspective, see `docs/user-guides/`.

RalphX automates the process of implementing a task — from scheduling the worker agent through code review and human approval. This document describes what happens at each stage, what surfaces in the UI, and how each failure mode is handled.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| How do I start a task? | Move it to **Ready** (or use ideation to create it). RalphX picks it up automatically when a slot is available. |
| How many tasks run at once? | Set **Max Concurrent** in execution settings. Default is 10 per project. |
| My task is stuck in Executing — what do I do? | The reconciler will auto-restart the agent. If it keeps failing, it moves to **Failed** after 5 retries. You can **Retry** or check the conversation for errors. |
| What is QA? | An optional automated testing phase after execution — a QA prep agent generates acceptance criteria, then a QA tester agent runs browser tests. |
| AI review passed — what do I do? | Review the findings in the task detail view and click **Approve** or **Request Changes**. |
| What does "Escalated" mean? | The AI reviewer couldn't decide. It's asking you to review manually. |
| What happens after I approve? | The task automatically enters the merge pipeline. See the [Merge Pipeline](merge-pipeline.md). |
| If I restart the app mid-execution, what happens? | RalphX resumes agent-active tasks automatically on startup. |

---

## Table of Contents

1. [Overview](#overview)
2. [Execution States](#execution-states)
3. [The Execution Pipeline Step by Step](#the-execution-pipeline-step-by-step)
   - [Scheduling](#phase-1-scheduling)
   - [Git Isolation](#phase-2-git-isolation)
   - [Pre-Execution Setup](#phase-3-pre-execution-setup)
   - [Worker Execution](#phase-4-worker-execution)
   - [QA (Optional)](#phase-5-qa-optional)
   - [Review](#phase-6-review)
   - [Approval](#phase-7-approval)
4. [AI Agent Involvement](#ai-agent-involvement)
5. [Concurrency and Scheduling](#concurrency-and-scheduling)
6. [Git Isolation and Worktrees](#git-isolation-and-worktrees)
7. [Revision Cycles](#revision-cycles)
8. [Recovery and Retry](#recovery-and-retry)
9. [Pause and Resume](#pause-and-resume)
10. [Human-in-the-Loop Controls](#human-in-the-loop-controls)
11. [Troubleshooting](#troubleshooting)
12. [Configuration Reference](#configuration-reference)

---

## Overview

When a task is ready to execute, RalphX automatically assigns a worker agent to implement it. The execution pipeline handles everything:

1. **Scheduling** — Picks the oldest Ready task when a concurrency slot is available
2. **Git isolation** — Creates a task branch and worktree for safe, conflict-free development
3. **Implementation** — A worker agent reads the task, decomposes it, and delegates to coder sub-agents
4. **QA (optional)** — A QA prep agent generates test criteria; a QA tester agent runs browser tests
5. **Review** — An AI reviewer analyzes the implementation, reports findings, and recommends approval or changes
6. **Human approval** — You review the AI's findings and approve or request changes

If anything goes wrong — agent timeout, provider error, test failure — RalphX automatically retries, surfaces the problem for you to resolve, or pauses the task.

### High-Level Flow

```
Task Ready
    |
    v
Executing ──── (worker + coders implement)
    |                   |               |
    v                   v               v
QaRefining         PendingReview     Failed
(qa enabled)            |           (retries exhausted)
    |               Reviewing
QaTesting               |
    |           ┌───────┴───────┐
QaPassed       ReviewPassed  RevisionNeeded
    |               |               |
    └──────> PendingReview     ReExecuting ───> PendingReview
                    |
                Approved ──> (Merge Pipeline)
```

---

## Execution States

Every task in the execution pipeline is in one of these states. Understanding them helps you know what's happening and what action to take.

### State Diagram

```
                         ┌──────────────────────────────────────────────┐
                         │                                              │
 Ready ──> Executing ──> PendingReview ──> Reviewing ──> ReviewPassed ──┤
             │   ^             ^               │              │         │
             │   │             │           RevisionNeeded  Escalated   │
             │   │             └────────────── │              │        │
             │   │                         ReExecuting ───────┘        │
             │   └──── (auto-restart on                                │
             │          agent failure)                                  │
             │                                                          │
             └──> QaRefining ──> QaTesting ──> QaPassed ───────────────┘
                       │                │
                    (agent)          QaFailed ──> RevisionNeeded
```

### State Details

| State | What's happening | What you see | Action needed? |
|-------|-----------------|--------------|----------------|
| **Ready** | Waiting for a concurrency slot | Task in Ready column; QA prep agent may start in background | No — wait for slot |
| **Executing** | Worker agent is implementing the task | Agent conversation active; step progress updating | No — agent is working |
| **QaRefining** | QA refiner agent is adapting test criteria to the implementation | Agent activity in QA tab | No — agent is working |
| **QaTesting** | QA tester agent is executing browser tests against the UI | Browser test output in QA tab | No — agent is working |
| **QaPassed** | All QA tests passed; auto-transitioning to review | Brief flash before PendingReview | No — auto-transition |
| **QaFailed** | QA tests failed | Failure count shown; task moves to RevisionNeeded | No — triggers re-execution automatically |
| **PendingReview** | Queued for AI review; reviewer being spawned | Brief flash before Reviewing | No — auto-transition |
| **Reviewing** | AI reviewer is analyzing the code | Reviewer conversation active | No — AI is working |
| **ReviewPassed** | AI approved; waiting for you | "Awaiting your approval" notification | **Click Approve or Request Changes** |
| **Escalated** | AI couldn't decide; needs your judgment | Escalation reason shown | **Review findings and decide** |
| **RevisionNeeded** | Reviewer requested changes; auto-transitioning to ReExecuting | Brief flash | No — auto-transition |
| **ReExecuting** | Worker agent is revising based on review feedback | Agent conversation active | No — agent is working |
| **Failed** | Execution failed after max retries | Error shown in task detail | Click **Retry** or investigate |
| **Paused** | Execution paused (user or provider error) | "Paused" badge; reason in detail | Click **Resume** when ready |
| **Stopped** | Task was manually stopped | "Stopped" badge | Click **Restart** to try again |

---

## The Execution Pipeline Step by Step

### Phase 1: Scheduling

When a task enters **Ready**, RalphX checks if there is room to run it:

1. **Concurrency check** — Compares the current running count against `max_concurrent` (per-project) and `global_max_concurrent` (all projects). If at capacity, the task waits in Ready.
2. **Provider rate-limit gate** — If any previous agent hit a provider rate limit, all new spawns are blocked until the `retry_after` timestamp expires.
3. **Active project scope** — If you have a project selected as active, only its Ready tasks are scheduled. Tasks from other projects wait.
4. **Oldest-first ordering** — When multiple tasks are Ready, the one that entered Ready first is scheduled (FIFO within concurrency limits).
5. **Ready settle delay** — A brief delay (`ready_settle_ms`, default 300ms) before scheduling so the task visibly "settles" in the Ready column in the UI before transitioning.

Once a slot is available:
- The running count is incremented
- The task transitions to **Executing**

> **QA background prep:** When a task enters Ready and QA is enabled, a lightweight `qa-prep` agent starts running in the background concurrently — so acceptance criteria are ready by the time execution finishes. This does not consume a concurrency slot.

### Phase 2: Git Isolation

Before spawning the worker agent, RalphX creates an isolated git environment:

1. **Branch creation** — Creates a task branch named `ralphx/{project-slug}/task-{task-id}` from the base branch (or the plan branch if the task is part of a plan).
2. **Worktree creation** — Creates a dedicated git worktree at `{worktree_parent}/{project-slug}/task-{task-id}`. The worker agent runs entirely inside this worktree — **never in your main checkout**.
3. **Existing branch handling** — If the task already has a branch (from a prior execution attempt), RalphX checks out the existing branch into a fresh worktree rather than creating a new one.

If git setup fails for any reason, the task transitions immediately to **Failed** with an `ExecutionBlocked` error. This is a hard failure — RalphX will not run agents in an uncontrolled environment.

> **Your working directory is always safe.** No execution ever touches your main checkout or any staged changes you have in progress.

### Phase 3: Pre-Execution Setup

With the worktree ready, RalphX runs install commands before spawning the agent:

- **Install commands** — Runs `worktree_setup` commands from the project analysis (e.g., `npm install`) inside the worktree. Results are stored as `execution_setup_log` in task metadata.
- **Setup failure handling** — Depends on the project's `merge_validation_mode`:
  - **Block / AutoFix**: If install commands fail, execution is blocked and the task transitions to Failed.
  - **Warn**: A warning is stored in metadata but execution continues.
  - **Off**: Setup is skipped entirely.

### Phase 4: Worker Execution

The worker agent is spawned via the ChatService with `ChatContextType::TaskExecution`:

1. **Prompt** — The worker receives `"Execute task: {task_id}"`. If there's a `restart_note` in the task metadata (set by the user), it's appended to the prompt and then cleared (one-shot).
2. **Agent behavior** — The worker reads the system card (`system-card-worker-execution-pattern.md`) and the task's implementation plan, decomposes work into sub-scopes, and delegates to parallel `ralphx-execution-coder` sub-agents. Each coder has exclusive file ownership within its scope.
3. **Step tracking** — The worker creates and updates task steps via MCP tools (`start_step`, `complete_step`). These are visible in the task detail view as a progress timeline.
4. **Supervisor monitoring** — Thresholds for loop, stall, repeated-error, and token-usage detection belong to the supervisor service. There is no dedicated supervisor agent spawned alongside the worker today.
5. **Completion** — When the agent stream ends:
   - If the agent produced output → task transitions to **PendingReview** (or **QaRefining** if QA is enabled)
   - If the agent produced no output → task transitions to **Failed**
6. **Auto-commit** — If `auto_commit` is enabled in execution settings, any uncommitted changes in the worktree are automatically committed after the worker finishes.

### Phase 5: QA (Optional)

QA is an optional phase that runs between execution and review. It is enabled per-task (via the `qa_enabled` flag).

#### QaRefining

The QA refiner agent (`qa-refiner`) adapts the acceptance criteria generated during QA prep to match the actual implementation:

1. **Wait for QA prep** — If the background `qa-prep` agent (started during Ready) hasn't finished yet, QaRefining waits for it to complete before spawning the refiner.
2. **Spawn QA refiner** — The refiner reviews the diff and updates test steps to reflect what was actually built.

#### QaTesting

The QA tester agent (`ralphx-qa-executor`) executes browser-based tests:

1. **Phase 2A (Refinement)** — Analyzes the git diff to update test steps one final time.
2. **Phase 2B (Testing)** — Executes tests via `agent-browser`, captures screenshots, and records pass/fail results.
3. **Outcome**:
   - All tests pass → **QaPassed** → auto-transitions to **PendingReview**
   - Any test fails → **QaFailed** → auto-transitions to **RevisionNeeded** → **ReExecuting**

### Phase 6: Review

#### PendingReview

An auto-transition state — the reviewer is spawned almost immediately.

1. **Review record created** — A review record is created in the database.
2. **Reviewer spawned** — The `ralphx-execution-reviewer` agent is launched via ChatService with `ChatContextType::Review`.
3. Pre-execution setup also runs here (same as Phase 3) so the reviewer can run validation commands during review.

#### Reviewing

The reviewer agent reads the task context, artifact, git diff, and prior review history:

1. **What it checks** — Code quality, test coverage, security, performance, adherence to task spec, regressions, and validation (typecheck, tests, lint via `get_project_analysis`).
2. **Outcome**: The reviewer MUST call `complete_review` with one of:
   - `approved` → task transitions to **ReviewPassed**
   - `needs_changes` (with structured issues) → task transitions to **RevisionNeeded**
   - `escalate` (with reason) → task transitions to **Escalated**

#### ReviewPassed

The AI approved the implementation. RalphX:
1. Emits a `review:ai_approved` notification to alert you.
2. The task sits in ReviewPassed waiting for your decision.

#### Escalated

The AI reviewer couldn't make a confident decision. RalphX:
1. Emits a `review:escalated` notification with the escalation reason.
2. You review the AI's findings and decide.

### Phase 7: Approval

From ReviewPassed or Escalated, you take action:

| Action | What happens |
|--------|-------------|
| **Approve** | Task transitions to **Approved** → auto-transitions to **PendingMerge** → merge pipeline begins |
| **Request Changes** | Task transitions to **RevisionNeeded** → auto-transitions to **ReExecuting** |

Once a task is **Approved**, control passes entirely to the [Merge Pipeline](merge-pipeline.md).

---

## AI Agent Involvement

### Agent Roles in the Execution Pipeline

| Agent | When spawned | What it does |
|-------|-------------|-------------|
| **ralphx-execution-worker** | On `Executing` entry | Orchestrates implementation: reads task, decomposes into sub-scopes, delegates to coders |
| **ralphx-execution-coder** | Dispatched by worker | Executes a single file-scoped sub-task with strict ownership boundaries |
| **qa-prep** | Background when task enters Ready | Generates acceptance criteria and test steps from task spec |
| **qa-refiner** | On `QaRefining` entry | Adapts test criteria to the actual implementation |
| **qa-tester** | On `QaTesting` entry | Executes browser-based acceptance tests |
| **ralphx-execution-reviewer** | On `Reviewing` entry | Automated code review; must call `complete_review` |

### Worker → Coder Pattern

The worker agent decomposes the task into 1–3 waves of parallel coder work:

```
Worker
  │
  ├── Wave 1: Coder A (creates new files)
  │           Coder B (creates test files)  ← parallel dispatch
  │
  └── Wave 2: Coder A (wires implementation)  ← after Wave 1 gate passes
              Coder B (updates tests)
```

- Each coder gets **exclusive write access** to specific files (no overlap within a wave).
- Multiple Task calls in a **single response** = true parallel execution.
- After each wave, the worker runs a gate: typecheck + tests + lint on modified files.
- Only if the gate passes does the next wave begin.

### Supervisor Detection Patterns

The supervisor service thresholds are intended to monitor the worker for:

| Pattern | Threshold | Response |
|---------|-----------|----------|
| Same tool called with similar args | 3+ times | Inject guidance |
| No git diff changes | ~2.5 minutes (default) | Inject guidance or escalate |
| Same error repeating | 3+ times (guidance), 4+ times (pause) | Inject guidance or pause |
| Token usage with no progress | High | Pause + notify (High severity) |
| Critical loop detected | — | Kill + analyze (Critical severity) |

### Re-Execution (Revision) Workflow

When a task enters **ReExecuting**, the worker agent receives the full revision context:

1. Fetches review notes and structured issues via `get_review_notes` + `get_task_issues`
2. Prioritizes issues by severity (critical → high → medium → low)
3. Tracks issue progress with `mark_issue_in_progress` / `mark_issue_addressed`
4. Re-runs pre-completion validation before finishing

The worker's conversation history is **preserved across execution cycles** — the agent can see its prior work and the reviewer's feedback in the same conversation thread.

---

## Concurrency and Scheduling

### How Slots Work

RalphX enforces two concurrency limits:

| Limit | Scope | Default | Max |
|-------|-------|---------|-----|
| `max_concurrent` | Per-project | 10 | No system-enforced cap — set to any value, but constrained in practice by `global_max_concurrent` |
| `global_max_concurrent` | All projects | 20 | 50 (validated/clamped by the system) |

A task can only start if **both** limits have capacity. Tasks that would exceed either limit wait in **Ready**.

Agent-active states that consume a slot:
- Executing, ReExecuting, QaRefining, QaTesting, Reviewing, Merging

States that do **not** consume a slot: Ready, QaPassed, PendingReview, ReviewPassed, Paused, Stopped, Failed.

### Scheduling Trigger Points

The scheduler runs at:
- Every time an agent-active task exits a slot (a task completes, fails, stops, or is paused)
- When a new task enters Ready
- On app startup (after resumption)

A mutex prevents duplicate simultaneous scheduling — at most one scheduling decision runs at a time.

### Provider Rate Limit Backpressure

If any agent receives a rate-limit error from the AI provider:
- The global `provider_blocked_until` timestamp is set to the `retry_after` time.
- **All** agent spawns are blocked until that time passes.
- Affected tasks are automatically paused with a `ProviderError` pause reason and auto-resume when the rate limit clears.

---

## Git Isolation and Worktrees

### Branch Naming

Every task gets its own branch:
```
ralphx/{project-slug}/task-{task-id}
```

For tasks in a plan, the base branch is the **plan branch** (not `main`). For standalone tasks, the base branch is the project's configured base branch (default: `main`).

### Worktree Location

Task worktrees are created at:
```
{worktree_parent_directory}/{project-slug}/task-{task-id}
```

Default worktree parent: `~/ralphx-worktrees`. Configurable in project settings.

### Why Worktrees

Each task's agent runs in its own isolated copy of the repository. This means:
- Multiple tasks can execute concurrently without interfering with each other.
- Your main checkout is never modified during agent execution.
- Your staged changes and working directory are always preserved.
- If an agent makes a mistake, it only affects the isolated worktree.

### Lifecycle

| Event | Git action |
|-------|-----------|
| Task enters Executing | Create branch + worktree from base |
| Task enters ReExecuting | Re-use existing branch + worktree |
| Task enters Reviewing | Reviewer reads worktree (no write) |
| Task is Merged | Branch and worktree are deleted (by merge pipeline) |
| Task is Stopped/Failed | Branch and worktree are **preserved** for inspection |

---

## Revision Cycles

When a reviewer requests changes, the task enters a revision cycle:

```
Reviewing → RevisionNeeded → ReExecuting → PendingReview → Reviewing → ...
```

### Revision Cap

To prevent infinite loops, RalphX enforces a maximum number of revision cycles per task (`max_revision_cycles`, default: 5). If the cap is reached:

1. Instead of transitioning to **ReExecuting**, the task transitions to **Failed**.
2. A failure reason is recorded in the task metadata.
3. You can manually retry (which resets the cycle count) or cancel the task.

### What the Worker Sees in Re-Execution

The worker receives a prompt of `"Re-execute task (revision): {task_id}"` plus any `restart_note` you've added. The full conversation history from previous execution cycles is visible in the same chat thread, giving the worker complete context about what was tried and what feedback was given.

---

## Recovery and Retry

RalphX runs a reconciliation loop that continuously checks for stuck or failed agents and takes corrective action.

### Automatic Recovery by State

#### Executing / ReExecuting Recovery

| Condition | Action |
|-----------|--------|
| Agent running + PID alive | No action — let it work |
| Agent run not running, PID missing | Auto-restart (within retry budget) |
| DB/registry mismatch within 30s grace period | Skip — registration may be catching up |
| DB/registry mismatch after grace period | Auto-restart (E7 conflict recovery) |
| Retry limit reached (`executing_max_retries`, default 5) | Transition to Failed |
| Wall-clock timeout exceeded (`executing_max_wall_clock_minutes`, default 60m) | Transition to Failed with `is_timeout: true` |

#### Reviewing Recovery

| Condition | Action |
|-----------|--------|
| Agent running + healthy | No action |
| Agent died or mismatch | Auto-restart (within retry budget) |
| Retry limit reached (`reviewing_max_retries`, default 3) | Transition to Escalated (for human review) |
| Wall-clock timeout (`reviewing_max_wall_clock_minutes`, default 30m) | Transition to Escalated |

#### QaRefining / QaTesting Recovery

| Condition | Action |
|-----------|--------|
| Stale (no progress for `qa_stale_minutes`) | Auto-restart |
| Retry limit reached (`qa_max_retries`, default 3) | Transition to QaFailed |
| Wall-clock timeout (`qa_max_wall_clock_minutes`, default 15m) | Transition to QaFailed |

### Startup Recovery

If you restart RalphX while tasks are running:

| State at shutdown | Recovery action |
|------------------|----------------|
| **Executing** | Agent is re-spawned automatically |
| **ReExecuting** | Agent is re-spawned automatically |
| **QaRefining** | Agent is re-spawned automatically |
| **QaTesting** | Agent is re-spawned automatically |
| **Reviewing** | Agent is re-spawned automatically |
| **QaPassed** | Auto-transition re-triggered → PendingReview |
| **PendingReview** | Auto-transition re-triggered → Reviewing |
| **RevisionNeeded** | Auto-transition re-triggered → ReExecuting |
| **Paused** | Stays paused (requires manual resume) |
| **Stopped** | Stays stopped (requires manual restart) |

Additionally, RalphX runs `recover_timeout_failures` on startup: tasks that failed with `is_timeout: true` and have fewer than 3 timeout attempts are automatically re-queued to **Ready**.

### Orphaned Process Cleanup

On startup, RalphX kills any OS processes left over from the previous session (tracked in the running agent registry). This prevents zombie processes from consuming resources.

---

## Pause and Resume

### Types of Pause

| Pause type | Triggered by | Auto-resumes? |
|------------|-------------|--------------|
| **User pause** (global) | Clicking "Pause" in execution panel | No — you must click Resume |
| **User pause** (per-task) | Clicking "Pause" on a specific task | No — you must click Resume |
| **Provider error pause** | AI provider returns an error (rate limit, overload, etc.) | Yes — after `retry_after` window passes (up to 5 attempts) |

### What Happens When Paused

1. The currently running agent is stopped.
2. The task transitions to **Paused** with metadata capturing:
   - `previous_status` — which state to resume to
   - `scope` — "global" or "task"
   - `paused_at` — timestamp
   - For provider errors: `category`, `message`, `retry_after`, `auto_resumable`
3. The running count is decremented, freeing a slot for other tasks.

### Resuming

When you click **Resume** (or the reconciler auto-resumes after a provider error):
1. The task transitions back to its `previous_status` (e.g., Executing, Reviewing).
2. The entry actions for that state re-run — the agent is re-spawned.
3. The conversation history is preserved — the agent continues from where it left off (session resumption).

### Provider Error Auto-Resume Limit

If a task is auto-resumed and hits a provider error again, up to `max_resume_attempts` (default: 5) times. After that, the task transitions to **Failed** instead of being auto-resumed.

---

## Human-in-the-Loop Controls

### Available Actions

| Action | When to use | What it does |
|--------|------------|--------------|
| **Approve** | ReviewPassed or Escalated | Approves the task → enters merge pipeline |
| **Request Changes** | ReviewPassed or Escalated | Sends the task back for revision |
| **Retry** | Failed or Stopped | Re-queues the task to Ready; resets timeout failure metadata |
| **Restart** | Stopped | Re-triggers entry actions; agent is re-spawned |
| **Pause** | Any agent-active state | Stops the current agent; task moves to Paused |
| **Resume** | Paused | Re-spawns the agent in the pre-pause state |
| **Stop** | Any agent-active state | Stops the agent; task moves to Stopped (terminal until restarted) |
| **Cancel** | Any state | Cancels the task entirely; unblocks dependent tasks |
| **Add restart note** | Any state | Appends a user note to the next agent's prompt (one-shot, cleared after use) |

### Settings You Can Configure

| Setting | Location | Options | Default |
|---------|----------|---------|---------|
| Max concurrent tasks | Execution settings | 1–∞ (per-project; no system cap — bounded in practice by global max) | 10 |
| Global max concurrent | Execution settings | 1–50 (system-clamped; prevents runaway parallelism across all projects) | 20 |
| Auto-commit | Execution settings | On / Off | On |
| Pause on failure | Execution settings | On / Off | On |
| AI review enabled | Review settings | On / Off | On |
| Require human review | Review settings | On / Off | Off |
| Max revision cycles | Review settings | number | 5 |
| Worktree directory | Project settings | Any absolute path | ~/ralphx-worktrees |
| Base branch | Project settings | Any branch name | main |

---

## Troubleshooting

### Task stuck in Executing for a long time

**What it means:** The worker agent may be in a loop, waiting on something, or the process died silently.

**What to do:**
1. Open the task's conversation to see the latest agent output.
2. If the conversation shows no recent activity, the reconciler will auto-restart the agent.
3. If it keeps happening, check the task's metadata for error details.
4. Click **Stop** then **Restart** to force a clean restart with a fresh agent spawn.

The reconciler enforces a wall-clock limit of 60 minutes — after that the task moves to Failed automatically.

### Task failed after too many retries

**What it means:** The agent hit the `executing_max_retries` limit (default: 5). Each restart attempt failed.

**What to do:**
1. Open the conversation to see what errors were produced.
2. Fix any underlying issue (dependency missing, test infrastructure broken, etc.).
3. Click **Retry** — this re-queues the task to Ready and resets the retry counter.

### Review keeps escalating

**What it means:** The AI reviewer consistently can't make a confident decision. This usually means the task is complex, ambiguous, or requires judgment beyond the reviewer's confidence threshold.

**What to do:** Read the escalation reason in the task detail view. Approve or request changes yourself. If the work looks correct, Approve. If not, Request Changes with notes on what to fix.

### "QA tests failed" — task keeps failing QA

**What it means:** The worker's implementation doesn't pass the acceptance criteria.

**What to do:**
1. Check the QA test results in the task's QA tab.
2. The task auto-transitions to RevisionNeeded → ReExecuting. The worker receives the QA failure details.
3. If the QA criteria seem wrong, you can disable QA for the task and approve manually.

### Provider rate limit — tasks paused automatically

**What it means:** The AI provider returned a rate limit. All spawns are gated until the `retry_after` time.

**What to do:** Nothing — RalphX automatically resumes when the rate limit clears. Paused tasks will auto-resume (up to 5 times). Monitor the execution panel for the "provider blocked" indicator.

### "Git isolation failed: could not create worktree"

**What it means:** RalphX couldn't create the task's worktree. Common causes: disk space, permissions, or a stale lock file.

**What to do:**
1. Check `~/ralphx-worktrees/{project}/task-{id}` — if it exists and is empty/corrupt, delete it manually.
2. Click **Retry** — the next execution attempt will recreate the worktree.
3. Check disk space and permissions on `~/ralphx-worktrees`.

### "Pre-execution setup failed"

**What it means:** The install commands (e.g., `npm install`) failed in the task worktree before the agent started.

**What to do:**
1. Check `execution_setup_log` in the task metadata for the specific failure.
2. Common causes: network issues, missing lockfile, incompatible Node/Rust version.
3. Click **Retry** — if it was a transient network issue, it will succeed on the next attempt.

### Revision cap reached — task Failed after N revision cycles

**What it means:** The task has gone through the maximum number of review-revision cycles (default: 5) without the reviewer approving.

**What to do:**
1. Inspect the conversation history to understand what feedback the reviewer keeps giving.
2. Consider whether the task spec is clear enough for the agent to execute correctly.
3. Add a **restart note** with specific guidance, then click **Retry** to reset the revision count and try again.
4. Alternatively, override the reviewer by approving the task manually if the implementation is acceptable.

---

## Configuration Reference

### Project-Level Settings

| Setting | Description | Options | Default |
|---------|-------------|---------|---------|
| `base_branch` | Branch new task branches are created from | Any branch name | `main` |
| `worktree_parent_directory` | Parent directory for task worktrees | Any absolute path | `~/ralphx-worktrees` |
| `git_mode` | Git isolation mode | `worktree` | `worktree` |
| `merge_validation_mode` | Controls pre-execution setup failure behavior | `Off`, `Warn`, `Block`, `AutoFix` | `Block` |
| `custom_analysis` | Override auto-detected install/validate commands | JSON (worktree_setup + validate arrays) | Auto-detected |

### Execution Settings (Per-Project)

| Setting | Description | Default |
|---------|-------------|---------|
| `max_concurrent_tasks` | Max tasks executing simultaneously in this project | 10 |
| `auto_commit` | Auto-commit uncommitted changes after execution | `true` |
| `pause_on_failure` | Pause execution when a task fails | `true` |

### Global Execution Settings

| Setting | Description | Range | Default |
|---------|-------------|-------|---------|
| `global_max_concurrent` | Hard cap on total concurrent agents across all projects | 1–50 | 20 |

### Review Settings

| Setting | Description | Default |
|---------|-------------|---------|
| `ai_review_enabled` | Whether AI review runs after execution | `true` |
| `require_human_review` | Require human sign-off even after AI approval | `false` |
| `max_revision_cycles` | Max revision cycles before task is Failed | 5 |
| `max_fix_attempts` | Max AI-created fix task attempts | 3 |

### Reconciliation Timing (ralphx.yaml)

| Setting | Description | Default |
|---------|-------------|---------|
| `executing_max_retries` | Max agent respawn attempts for Executing/ReExecuting | 5 |
| `executing_max_wall_clock_minutes` | Max minutes before wall-clock timeout → Failed | 60 |
| `reviewing_max_retries` | Max agent respawn attempts for Reviewing | 3 |
| `reviewing_max_wall_clock_minutes` | Max minutes before review timeout → Escalated | 30 |
| `qa_max_retries` | Max agent respawn attempts for QaRefining/QaTesting | 3 |
| `qa_max_wall_clock_minutes` | Max minutes before QA timeout → QaFailed | 15 |
| `qa_stale_minutes` | Minutes of no QA progress before stale detection | 5 |
| `ready_settle_ms` | Milliseconds to wait before scheduling a newly-Ready task | 300 |

### Environment Variable Overrides

Key reconciliation settings can be overridden via environment variables at runtime (useful for testing or temporary adjustments):

| Variable | Overrides |
|----------|-----------|
| `RALPHX_RECONCILIATION_EXECUTING_MAX_RETRIES` | `executing_max_retries` |
| `RALPHX_RECONCILIATION_EXECUTING_MAX_WALL_CLOCK_MINUTES` | `executing_max_wall_clock_minutes` |
| `RALPHX_RECONCILIATION_REVIEWING_MAX_RETRIES` | `reviewing_max_retries` |
| `RALPHX_RECONCILIATION_QA_MAX_RETRIES` | `qa_max_retries` |
| `RALPHX_LIMITS_MAX_RESUME_ATTEMPTS` | `max_resume_attempts` (provider error auto-resume cap) |
| `RALPHX_DISABLE_STARTUP_RECOVERY` | Disables startup task resumption (testing only) |

---

## See Also

- [Merge Pipeline](merge-pipeline.md)
- [Task State Machine](task-state-machine.md)
