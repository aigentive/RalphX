# Merge Pipeline User Guide

RalphX automates the entire process of merging your task's code changes into the target branch. This guide explains what happens behind the scenes, what you'll see in the UI, and how to handle any issues that come up.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| How do I start a merge? | Approve the task. It automatically enters the merge pipeline. |
| What merge strategy should I use? | **RebaseSquash** (default) gives the cleanest history. Use **Merge** if you need to preserve individual commits. |
| What changes in GitHub PR Mode? | The final plan merge happens through a GitHub PR. RalphX opens/monitors the PR and finishes the task after GitHub merges it. |
| My merge is stuck — what do I do? | Check the merge progress panel. If it says "Needs Attention", click **Retry Merge**. |
| What does "Deferred" mean? | Another task is merging to the same branch. Yours will start automatically when the other finishes. |
| Validation failed — now what? | In **Block** mode: fix your code and retry (or use **Retry (Skip Validation)** to bypass validation once). In **AutoFix** mode: a fixer agent attempts the fix automatically — the "Fixing Validation Errors" banner shows while it works. |
| Can I merge manually? | RalphX handles merging automatically, but you can pause, stop, or retry at any time. |
| What happens if I restart the app mid-merge? | RalphX recovers automatically. PendingMerge tasks restart the merge. Merging tasks are picked up by the reconciler. |

---

## Table of Contents

1. [Overview](#overview)
2. [Merge States](#merge-states)
3. [Merge Strategies](#merge-strategies)
4. [The Merge Pipeline Step by Step](#the-merge-pipeline-step-by-step)
5. [Branch Management](#branch-management)
   - [Branch Freshness Conflicts](#branch-freshness-conflicts)
6. [Validation](#validation)
7. [AI Agent Involvement](#ai-agent-involvement)
8. [Recovery and Retry](#recovery-and-retry)
   - [Recovery Timeline](#recovery-timeline)
   - [Recovery Status Badges](#recovery-status-badges)
9. [Human-in-the-Loop Controls](#human-in-the-loop-controls)
10. [Progress UI](#progress-ui)
11. [Troubleshooting](#troubleshooting)
12. [Configuration Reference](#configuration-reference)

---

## Overview

When a task is approved, RalphX automatically merges its code changes into the target branch. The merge pipeline handles everything:

1. **Preparation** — Resolves branches, checks preconditions
2. **Branch freshness** — Ensures branches are up-to-date
3. **Programmatic merge** — Attempts the merge using your chosen strategy
4. **Validation** — Runs your project's test/lint commands to verify the merge
5. **Finalization** — Commits the merge, cleans up branches and worktrees

If anything goes wrong — merge conflicts, validation failures, git errors — RalphX automatically retries, spawns AI agents to fix issues, or surfaces the problem for you to resolve.

For projects using **GitHub PR Mode**, the final plan-merge task uses a different settlement path:

- RalphX still handles the normal task -> plan-branch merges
- RalphX creates and updates a GitHub PR for the **plan branch**
- The final plan-merge task waits for GitHub to merge that PR instead of merging the plan branch directly

See the [GitHub PR Mode User Guide](github-pr-mode.md) for the plan-PR flow and user-facing UI differences.

### High-Level Flow

```
Task Approved
     |
     v
 PendingMerge ──── (programmatic merge attempt)
     |                    |                  |
     v                    v                  v
  Merged           Merging (agent)    MergeIncomplete
  (done!)          resolves conflicts   (retry or fix)
                         |
                    ┌────┴────┐
                    v         v
                 Merged   MergeConflict
                 (done!)  (needs attention)
```

---

## Merge States

Every task in the merge pipeline is in one of five states. Understanding these helps you know what's happening and what action to take.

### State Diagram

```
                         ┌──────────────────────────────────────────┐
                         │                                          │
 Approved ──> PendingMerge ──────────────> Merged (done)            │
                  │  │                       ^   ^                  │
                  │  │  (conflicts)          │   │                  │
                  │  └──> Merging ───────────┘   │                  │
                  │         │                    │                  │
                  │         └──> MergeConflict ──┘                  │
                  │                  │                              │
                  └──> MergeIncomplete <───────────────────────────┘
                           │                  (auto-retry)
                           └──> PendingMerge ─────────────────> ...
```

### State Details

| State | What's happening | What you see | Action needed? |
|-------|-----------------|--------------|----------------|
| **PendingMerge** | RalphX is attempting the merge automatically | Progress phases updating in real-time | No — wait for it to complete |
| **Merging** | Either an AI agent is resolving conflicts/fixing validation, or RalphX is waiting for a GitHub plan PR to merge | Agent activity in the merge chat, or a PR status card with "Waiting for PR Merge" | Usually no — let the agent work, or merge the PR in GitHub if this is a PR-backed plan |
| **MergeIncomplete** | The merge attempt failed (git error, timeout, or validation failure) | "Needs Attention" badge | Click **Retry Merge** or investigate the error |
| **MergeConflict** | The AI agent couldn't resolve the conflicts | "Needs Attention" badge with conflict details; click any conflict file to open the inline **ConflictDiffViewer** | Click **Retry Merge** (if code changed) or resolve manually |
| **Merged** | Merge is complete. Code is on the target branch. | Green checkmark, merge commit SHA shown | None — task is done |

### What Satisfies Dependencies

When other tasks depend on this one, only **Merged** and **Cancelled** unblock them. A task stuck in MergeIncomplete or MergeConflict does **not** unblock dependent tasks — you need to resolve the merge first.

---

## Merge Strategies

RalphX supports four merge strategies. You choose one per project in the project settings.

| Strategy | How it works | History style | Best for |
|----------|-------------|---------------|----------|
| **Merge** | Creates a merge commit combining source and target | Non-linear (merge commits visible) | Teams that want full commit history preserved |
| **Rebase** | Replays source commits on top of target, then fast-forwards | Linear (no merge commits) | Clean linear history with individual commits |
| **Squash** | Combines all source commits into a single commit on target | Linear (one commit per task) | Clean history, one commit per feature |
| **RebaseSquash** (default) | Rebases source onto target, then squashes into one commit | Cleanest linear history | Most projects — clean and simple |

### How Strategy Selection Works

The merge strategy is a project-level setting. All tasks in the project use the same strategy. You can change it at any time in the project settings — the new strategy applies to future merges.

### Checkout-Free vs. Worktree Isolation

> **Working Directory Safety:** RalphX never modifies your main checkout. Merge operations always happen in isolated worktrees or via checkout-free git plumbing — your working directory and staged changes are never touched.

All four merge strategies support a checkout-free path:

- **If the target branch is checked out in the main repo**: RalphX uses fast git plumbing commands ("checkout-free" mode). Each strategy maps to a specific checkout-free operation: Merge → `checkout_free_merge`, Rebase → `checkout_free_fast_forward` (no rebase — fast-forward only), Squash → `checkout_free_squash_merge`, RebaseSquash → `checkout_free_squash_merge` (rebase step skipped).
- **Otherwise**: RalphX creates a temporary **worktree** — an isolated copy of the repo — to perform the merge safely. The worktree is cleaned up after the merge completes.

For Rebase and RebaseSquash strategies, when running via the **worktree path** (not checkout-free), RalphX creates **two worktrees**: one for the rebase operation and one for the final merge. This two-worktree isolation does not apply to the checkout-free path.

---

## The Merge Pipeline Step by Step

When a task enters PendingMerge, here's exactly what happens:

> **Note:** The detailed phases below describe the standard direct-merge pipeline. In GitHub PR Mode, the final **plan-merge task** switches to a PR-backed flow: RalphX pushes the plan branch, marks the PR ready, waits in `Merging` while polling GitHub, and completes the task after GitHub merges the PR.

### Phase 1: Preparation

1. **Deduplication check** — Prevents the same merge from running twice simultaneously
2. **Load context** — Fetches task and project details from the database
3. **Branch discovery** — If the task lost track of its branch (rare), attempts to find and re-attach it
4. **Precondition check** — For plan-merge tasks, validates that the plan branch is in the correct state

### Phase 2: Branch Resolution

5. **Resolve branches** — Determines the source and target branches:
   - Regular tasks: task branch → project base branch (usually `main`)
   - Plan tasks: task branch → plan branch
   - Plan-merge tasks: plan branch → base branch

6. **Main-merge deferral** — If this task is merging to the main branch and other tasks from the same plan are still running, the merge is deferred until they complete. This prevents partial plan merges.

### Phase 3: Branch Freshness

7. **Plan branch update** — If merging to a plan branch, updates it from main first (to prevent false validation failures from stale code). **Errors are fatal** — an error here moves the task to MergeIncomplete immediately.
8. **Source branch update** — If the source branch is behind the target, merges the target into the source to bring it up-to-date. **Errors are non-fatal** — an error here is logged and the merge continues to Phase 4.

If either update produces **conflicts** (distinct from errors), the task transitions to **Merging** and an AI agent is spawned to resolve them (see [Branch Freshness Conflicts](#branch-freshness-conflicts)).

### Phase 4: Early-Exit Checks

9. **Already-merged check** — If the source branch's changes are already on the target (from a prior agent run that died before completing), skips straight to finalization
10. **Deleted source branch recovery** — If the source branch was deleted but its commits exist on the target, recovers and completes the merge

### Phase 5: Concurrency Control

11. **Concurrent merge guard** — Under a lock, checks if another task is already merging to the same target branch. If so, defers this task (the earlier task has priority). This prevents conflicts between simultaneous merges to the same branch.

### Phase 6: Cleanup

12. **Pre-merge cleanup** — Removes artifacts from any prior failed attempts:
    - Cancels any in-flight validation
    - Stops any running agents (reviewer, merger) from prior attempts
    - Kills processes in the task worktree
    - Removes stale git lock files
    - Deletes old worktrees (task, merge, rebase, plan-update, source-update)
    - Cleans up orphaned worktrees from other completed tasks

### Phase 7: Merge Execution

13. **Build commit message** — Creates the squash commit message (for Squash/RebaseSquash strategies)
14. **Execute merge strategy** — Runs the selected strategy (Merge/Rebase/Squash/RebaseSquash) with a deadline timeout

### Phase 8: Post-Merge Validation

15. **Run validation commands** — If configured, runs your project's test/lint/typecheck commands against the merged code (see [Validation](#validation))

### Phase 9: Finalization

16. **Complete merge** — Records the merge commit SHA, transitions the task to Merged
17. **Cleanup** — Deletes the task branch and worktree, marks plan branches as merged
18. **Settle delay** — Waits `merge_settle_ms` before unblocking dependent tasks (allows git state to propagate), then schedules newly-ready tasks

---

## Branch Management

### Branch Hierarchy

RalphX uses a three-level branch structure:

```
main (base branch)
  └── plan/feature-auth (plan branch — groups related tasks)
        ├── ralphx/myproject/task-abc123 (task branch)
        ├── ralphx/myproject/task-def456 (task branch)
        └── ralphx/myproject/task-ghi789 (task branch)
```

- **Main branch** — Your project's base branch (default: `main`, configurable)
- **Plan branches** — Created for ideation plans. Multiple tasks can share a plan branch as their merge target.
- **Task branches** — One per task, named `ralphx/{project-slug}/task-{task-id}`. Created automatically when a task starts executing.

### How Merge Targets Are Determined

| Task type | Source branch | Target branch |
|-----------|--------------|---------------|
| Standalone task | Task branch | Base branch (main) |
| Task in a plan | Task branch | Plan branch |
| Plan-merge task | Plan branch | Base branch (main) |

### Branch Freshness

Before merging, RalphX ensures branches are current:

1. **Plan branch from main** — If a plan branch is behind main (e.g., hotfixes were applied to main), RalphX merges main into the plan branch first. This prevents validation failures from missing fixes.

2. **Source branch from target** — If the source (task) branch is behind the target, RalphX merges the target into the source. This ensures the merged code includes all recent changes.

Both updates happen in isolated worktrees. If conflicts arise, an AI agent is spawned to resolve them (see [Branch Freshness Conflicts](#branch-freshness-conflicts) below).

### Branch Freshness Conflicts

When a branch update produces merge conflicts (not errors), the task enters a sub-lifecycle inside Merging state.

#### Plan-Update Conflict Sub-Lifecycle

```
PlanUpdateResult::Conflicts detected
  → metadata["plan_update_conflict"]=true
    (+ target_branch, base_branch, conflict_files, source_branch stored)
  → worktree_path set to the merge worktree
  → task transitions to Merging
  → merger agent spawned (plan-update prompt variant)
  → on agent exit: auto-complete checks if plan branch is now up-to-date with base
      if yes  → clears conflict flags → transitions back to PendingMerge (full retry)
      if still behind → MergeIncomplete
```

UI: The Merging detail view shows a **main→plan** branch flow (BranchFlow) to indicate the direction of the freshness update.

#### Source-Update Conflict Sub-Lifecycle

```
SourceUpdateResult::Conflicts detected
  → new merge worktree created with source branch
  → metadata["source_update_conflict"]=true
  → task transitions to Merging
  → merger agent spawned (source-update prompt variant)
  → on agent exit: auto-complete checks if source branch is now up-to-date with target
      if yes  → clears conflict flag → transitions back to PendingMerge (full retry)
      if still behind → MergeIncomplete
```

UI: The Merging detail view shows a **source→target** branch flow.

**Key asymmetry:** Source update *errors* are non-fatal (merge continues). Plan update *errors* ARE fatal (MergeIncomplete immediately).

#### Existing Worktree Conflict Routing

`update_plan_from_main` uses a 3-path decision for the target branch:

| Condition | Path |
|-----------|------|
| Target checked out in main repo | Checkout-free merge (no new worktree) |
| Target in an **existing worktree** | Merges main directly inside that existing worktree |
| Target not checked out anywhere | Creates a new temporary worktree |

For the existing-worktree path: if a `MERGE_HEAD` file is present from a prior attempt, the operation routes to `Conflicts` instead of `Error` — preserving any partial conflict resolution already in progress.

### Branch Cleanup After Merge

When a merge completes:
1. Any worktree processes are terminated
2. The worktree is deleted
3. The task branch is force-deleted
4. The task's branch and worktree references are cleared (prevents stale references if the task is reopened)
5. For plan merges: the plan branch is marked as merged and its feature branch is deleted

---

## Validation

After a successful merge, RalphX can run your project's validation commands (tests, linting, type checking) to verify the merged code works correctly.

### Validation Modes

| Mode | What happens on validation failure | When to use |
|------|-----------------------------------|-------------|
| **Off** | Validation is skipped entirely | Small projects, manual testing preferred |
| **Warn** | Merge completes, warnings stored in metadata | Non-critical validation (advisory) |
| **Block** (default) | Merge is reverted, task moves to MergeIncomplete | Production projects — ensures passing code |
| **AutoFix** | A fixer agent attempts to fix the failures | Projects where most failures are auto-fixable |

### How Validation Works

Validation runs in two phases:

**Setup Phase** (worktree_setup commands)
- Creates symlinks for shared directories (node_modules, build caches)
- Runs install commands (e.g., `npm install`)
- Non-fatal: setup failures produce warnings but don't block validation

**Validate Phase** (validate commands)
- Runs each configured command (e.g., `npm run typecheck`, `cargo clippy`, `cargo test`)
- In **Block/AutoFix** mode: stops on the first failure (fail-fast)
- In **Warn** mode: runs all commands regardless of failures
- **Automatic retry**: Failed commands are retried once after a 2-second delay (handles transient issues like filesystem locks or compilation timeouts)
- **Caching**: If the source branch SHA hasn't changed since the last successful validation, previously-passed commands are skipped

### What Happens When Validation Fails

**Block mode:**
1. The merge commit is reverted (`git reset --hard HEAD~1`)
2. The task transitions to MergeIncomplete
3. `validation_revert_count` is incremented (clicking **Retry Merge** resets this counter to 0)
4. If `git reset` itself fails, the `merge_commit_unrevertable` flag is set — the task moves to MergeIncomplete but the merge commit remains on the target branch and requires manual cleanup
5. The reconciler may auto-retry (with exponential backoff)

**AutoFix mode:**
1. The merge commit is **kept** (not reverted)
2. A dedicated fixer worktree is created on the target branch
   - If the original merge was checkout-free: a new worktree is created explicitly for the fixer
   - If fixer worktree creation fails: the merge is reverted and the task moves to MergeIncomplete
3. The task transitions to Merging; the UI shows a **"Fixing Validation Errors"** banner with a wrench icon
4. A fixer agent is spawned with the validation failure details
5. The agent reads the failures, fixes the code, and commits
6. On agent completion, validation is re-run to confirm the fix
7. If re-validation passes: merge completes. If it fails: merge is reverted and task moves to MergeIncomplete.

**Warn mode:**
1. Validation failures are stored as warnings in metadata
2. The merge proceeds to completion
3. Warnings are visible in the task detail view

### Validation Revert Loop Protection

If validation keeps failing and reverting (indicating a code problem, not a transient issue), RalphX stops auto-retrying after `validation_revert_max_count` reverts and surfaces the task for manual attention.

---

## AI Agent Involvement

RalphX uses AI agents to handle merge conflicts and validation failures automatically.

The merge lane follows the configured multi-harness runtime. RX-native Team coordination remains provider-neutral; harness adapters own provider-specific process details.

### Merger Agent

**When it's spawned:** When the programmatic merge detects conflicts that can't be resolved automatically.

**What it does:**
- Receives the conflict files and worktree path
- Resolves merge conflicts in the worktree
- For rebase strategies: runs `git add <file>` then `git rebase --continue` for each conflict
- Commits the resolution

**Prompt variants:**
- **Standard conflicts**: "Resolve merge conflicts for task: {task_id}"
- **Plan-update conflicts**: "Resolve conflicts between main and the plan branch" (when updating a plan branch from main produced conflicts)
- **Source-update conflicts**: "Resolve conflicts when updating source branch from target" (when updating the source branch from the target produced conflicts)
- **Validation recovery**: "Fix validation failures for task: {task_id}. The merge succeeded but post-merge validation commands failed..." (AutoFix mode fixer agent)

### Fixer Agent

**When it's spawned:** When post-merge validation fails in AutoFix mode.

**What it does:**
- Receives the validation failure details (command, exit code, stderr)
- Reads the failing code in the fixer worktree
- Fixes the code to make validation pass
- Commits the fix
- On completion, RalphX re-runs validation to confirm

The fixer agent is actually the same merger agent with a different prompt: "Fix validation failures for task: {task_id}. The merge succeeded but post-merge validation commands failed..."

### Agent Lifecycle

1. **Spawn**: The agent is started via `ChatService.send_message()` with the `Merge` context type
2. **Monitor**: The reconciler checks agent health via heartbeat-based effective age
3. **Completion**: When the agent exits, `attempt_merge_auto_complete()` runs:
   - Reads `target_branch` from task metadata (set at conflict-detection time) — not re-resolved from scratch, so it reflects the branch at the time the conflict was detected
   - Checks for stale rebase/merge state
   - Checks for conflict markers
   - Re-runs validation (if applicable)
   - Verifies the merge on the target branch
   - Completes or transitions to the appropriate failure state

### Pre-Spawn Cleanup

Before spawning a merger agent, RalphX cleans the worktree:
- Aborts any stale rebase or merge in progress
- Removes worktree symlinks (which can cause false conflicts)
- The agent's validation step re-creates symlinks as needed

---

## Recovery and Retry

RalphX has a robust reconciliation system that automatically detects and recovers from merge issues.

### Automatic Recovery by State

#### PendingMerge Recovery
- **Validation in progress**: Skip (let it finish)
- **Main-merge deferred**: Retry when all agents are idle
- **Deferred (concurrent merge)**: Retry when the blocking task finishes
- **Stale + deferred**: Re-trigger merge entry actions
- **Stale + not deferred**: Transition to MergeIncomplete

#### Merging Recovery
- **Agent running + healthy**: Let it work
- **Agent stale (timed out)**: Record timeout, check retry count
- **Max retries reached**: Transition to MergeIncomplete
- **DB/registry mismatch** (agent died silently): Auto-restart within retry budget
- **30s grace period**: New agents get 30 seconds before being considered stale

#### MergeIncomplete Recovery
- **Branch missing**: Surface to user (no auto-retry)
- **Validation in progress**: Skip
- **User retry in progress**: Skip
- **Rate limited**: Skip until limit expires (doesn't count toward retry budget)
- **Agent-reported failure**: No auto-retry (deliberate decision)
- **Validation revert loop**: No auto-retry (code needs fixing)
- **Otherwise**: Auto-retry with exponential backoff → PendingMerge

#### MergeConflict Recovery
- **Branch missing**: Surface to user
- **User retry in progress**: Skip
- **Agent-reported failure**: No auto-retry
- **Source SHA unchanged**: No auto-retry (same conflict would recur)
- **Source SHA changed**: Auto-retry → PendingMerge

### Exponential Backoff

Auto-retries use increasing delays between attempts to avoid overwhelming the system. Each retry waits longer than the previous one.

### Startup Recovery

If you restart RalphX while a merge is in progress:
- **PendingMerge tasks**: The merge is re-triggered automatically on startup
- **Merging tasks**: The reconciler detects the stale agent and re-spawns it
- **MergeIncomplete/MergeConflict tasks**: Normal reconciliation rules apply

### Merge Recovery Metadata

RalphX tracks the full history of merge attempts in structured metadata:
- **Events**: Deferred, AutoRetryTriggered, AttemptStarted, AttemptFailed, AttemptSucceeded, ManualRetry
- **Failure sources**: TransientGit (safe to retry), AgentReported (needs human), SystemDetected (safe to retry), ValidationFailed (needs code fix)
- **Recovery states**: Deferred, Retrying, Failed, Succeeded, RateLimited

### Recovery Timeline

The recovery timeline in the task detail view shows each event with:
- **Source badge**: System (automated pipeline), Auto (reconciler-triggered), or User (manual action)
- **Attempt number** and timestamp
- **Message** describing what happened at that step

### Recovery Status Badges

Tasks in the merge pipeline show one of these status badges based on recovery history:

| Badge | Meaning |
|-------|---------|
| **Auto-recovery attempted** | The reconciler has retried at least once automatically |
| **Deferred due to active merge** | Waiting for another merge on the same branch to complete |
| **Last attempt failed** | The most recent attempt ended in failure |

### Unblocking Dependents

After a merge completes, dependent tasks are not unblocked immediately — a `merge_settle_ms` delay is applied first to allow git state to propagate before scheduling newly-ready tasks.

---

## Human-in-the-Loop Controls

### Available Actions

| Action | When to use | What it does |
|--------|------------|--------------|
| **Retry Merge** | MergeIncomplete or MergeConflict | Transitions the task back to PendingMerge to re-attempt the merge; resets `validation_revert_count` to 0 |
| **Retry (Skip Validation)** | MergeIncomplete when validation caused the failure | Same as Retry Merge but skips post-merge validation on the next attempt |
| **Pause** | Any merge state | Pauses the merge (and any running agent). Resume to continue. |
| **Stop** | Any merge state | Stops the merge. The task moves to Stopped. Restart to try again. |
| **Cancel** | Any merge state | Cancels the task entirely. Unblocks dependent tasks. |
| **Restart** | Stopped state | Validates git state and resumes the merge (or force-restart to skip validation) |

### Settings You Can Configure

| Setting | Location | Options | Default |
|---------|----------|---------|---------|
| Merge strategy | Project settings | Merge, Rebase, Squash, RebaseSquash | RebaseSquash |
| Validation mode | Project settings | Off, Warn, Block, AutoFix | Block |
| Base branch | Project settings | Any branch name | main |
| Worktree directory | Project settings | Any path | ~/ralphx-worktrees |
| Validation commands | Project settings (custom analysis) | Any shell commands | Auto-detected |

### Review and Approval Flow

Before a task reaches the merge pipeline, it goes through the full review cycle:

```
Executing → QA → Review → Approved → PendingMerge
```

The merge only begins after the task is **Approved**. The Approved → PendingMerge transition is automatic.

---

## Progress UI

When a merge is running, the UI shows a progress timeline with phases updating in real-time.

### Structural Phases

These phases appear for every merge:

| Phase | What it means |
|-------|--------------|
| **Preparation** | Loading task/project context and discovering branches |
| **Preconditions** | Validating that the merge can proceed (plan state, branch existence) |
| **Branch Freshness** | Updating branches from main/target to prevent stale code |
| **Cleanup** | Removing artifacts from prior failed attempts |
| **Worktree Setup** | Creating an isolated environment for the merge |
| **Merge** | Running the actual git merge/rebase operation |
| **Finalize** | Publishing the merge commit and cleaning up |

### Phase Descriptions

Each phase in the progress timeline has an always-visible subtitle describing its current activity (not just a tooltip). This updates as the phase progresses.

The full phase list is emitted in two batches: early structural phases appear first, then the complete list (including validation phases) is emitted when validation begins.

### Dynamic Validation Phases

If validation is enabled, additional phases appear — one per validation command:

| Example command | Phase label shown |
|----------------|------------------|
| `npm run typecheck` | Type Check |
| `cargo clippy` | Clippy |
| `cargo test` | Tests |
| `npm run lint` | Lint |
| Custom command | Derived from command name |

### Phase Statuses

Each phase shows one of these statuses:

| Status | Icon | Meaning |
|--------|------|---------|
| **Started** | Spinner | Phase is currently running |
| **Passed** | Green check | Phase completed successfully |
| **Failed** | Red X | Phase failed |
| **Skipped** | Gray dash | Phase was skipped (earlier phase failed in fail-fast mode) |

### Merge Pipeline View

The merge pipeline panel groups tasks into three categories:

- **Active** — Tasks currently being merged (Merging state)
- **Waiting** — Tasks waiting to merge (PendingMerge state, possibly deferred)
- **Needs Attention** — Tasks that need your intervention (MergeConflict, MergeIncomplete)

Each task shows its source/target branches, any blocking information, conflict files, and error context.

The merge pipeline popover also shows:
- **Elapsed time counter** — how long the current merge attempt has been running
- **Deferred agent count** — number of tasks in main-merge-deferred state waiting for sibling tasks to finish
- **Stop button** — immediately stops the active merge

### Merge Conversation Navigation

When multiple merge attempts have occurred (each spawning its own agent conversation), the UI auto-selects the most recent merge conversation by `createdAt`, ensuring you always see the latest agent activity. Each merge conversation is linked in the task detail view.

### Pending and Merging Detail Views

Both the PendingMerge and Merging detail views show the **source→target** branch flow (BranchFlow) so you can see exactly which branches are involved in the current merge. For freshness-conflict sub-lifecycles, the direction reflects the conflict: **main→plan** for plan-update conflicts, **source→target** for source-update conflicts.

### Merged Task Detail

Completed merges show:
- **Merge commit SHA** (with copy/link actions)
- **Branch tag** — "Deleted" label on the source branch after cleanup
- **Commit history** for all commits included in the merge
- **Validation history** across all attempts (commands run, pass/fail status)

### Progress Hydration

If you navigate away and come back, the progress UI recovers its state from an in-memory store. You won't lose progress information during normal navigation.

---

## Troubleshooting

### "Merge Deferred" — Task is waiting

**What it means:** Another task is currently merging to the same target branch. Only one merge to a given branch can run at a time to prevent conflicts.

**What to do:** Nothing — your task will automatically start merging when the other task finishes. The earliest task (by time entering PendingMerge) gets priority.

### "Main Merge Deferred" — Waiting for agents

**What it means:** This task is merging to the main branch, but other tasks from the same plan are still executing. RalphX defers the merge to avoid partial plan merges.

**What to do:** Nothing — the merge will start automatically when all sibling tasks complete.

### Validation keeps failing and reverting

**What it means:** The merged code doesn't pass your project's validation commands. RalphX has reverted the merge and may have auto-retried several times.

**What to do:**
1. Check the validation failures in the task metadata (available in the task detail view)
2. Look at the specific command that failed and the stderr output
3. Fix the issue in your task's code
4. Click **Retry Merge**

If using **AutoFix** mode, the fixer agent may have already attempted a fix. Check the agent's chat for details.

### "Branch Not Found"

**What it means:** The task's source branch or the target branch no longer exists in git.

**What to do:**
- If the source branch is missing: the task may need to be re-executed to recreate the branch
- If the target branch is missing: check if the plan branch was deleted prematurely

RalphX will not auto-retry when a branch is missing — this requires manual investigation.

### Agent timed out

**What it means:** The merger agent ran for too long without making progress (determined by heartbeat monitoring).

**What to do:** RalphX automatically restarts the agent within the retry budget. If it keeps timing out, the task moves to MergeIncomplete. Click **Retry Merge** to try again, or investigate whether the conflicts are too complex for automated resolution.

### Merge deadline exceeded

**What it means:** The entire merge attempt (cleanup + merge operation) took longer than the configured deadline.

**What to do:** This is usually caused by slow cleanup (stuck worktree processes) or complex merge operations. Click **Retry Merge** — the pre-merge cleanup on the next attempt will clear the stuck state.

### Worktree errors

**What it means:** Git couldn't create or delete a worktree. This can happen if a process is still using files in the worktree, or if the worktree directory is in an inconsistent state.

**What to do:** RalphX's cleanup automatically handles most worktree issues on retry. Click **Retry Merge**. If the problem persists, you can manually delete the worktree directory (found at `~/ralphx-worktrees/{project}/{task-id}`).

### Rate limited by provider

**What it means:** The AI provider (used for merge agents) returned a rate limit. RalphX pauses retries until the rate limit expires.

**What to do:** Nothing — RalphX automatically resumes when the rate limit clears. Rate-limited pauses don't count toward the retry budget.

---

## Configuration Reference

### Project-Level Settings

| Setting | Description | Options | Default |
|---------|-------------|---------|---------|
| `merge_strategy` | Git merge strategy for all tasks | `Merge`, `Rebase`, `Squash`, `RebaseSquash` | `RebaseSquash` |
| `merge_validation_mode` | What happens when validation fails | `Off`, `Warn`, `Block`, `AutoFix` | `Block` |
| `base_branch` | Target branch for standalone task merges | Any branch name | `main` |
| `worktree_parent_directory` | Parent directory for task worktrees | Any absolute path | `~/ralphx-worktrees` |
| `custom_analysis` | Override auto-detected validation commands | JSON (worktree_setup + validate arrays) | Auto-detected |

### Reconciliation Timing (ralphx.yaml)

These values control how aggressively RalphX retries and recovers:

| Setting | Description | Default |
|---------|-------------|---------|
| `attempt_merge_deadline_secs` | Maximum time for a single merge attempt | Configured in ralphx.yaml |
| `validation_deadline_secs` | Maximum time for validation commands | Configured in ralphx.yaml |
| `merger_timeout_secs` | How long before a merger agent is considered stale | Configured in ralphx.yaml |
| `merging_max_retries` | Max agent respawn attempts for Merging state | Configured in ralphx.yaml |
| `merge_incomplete_max_retries` | Max auto-retries for MergeIncomplete | Configured in ralphx.yaml |
| `merge_conflict_max_retries` | Max auto-retries for MergeConflict | Configured in ralphx.yaml |
| `validation_revert_max_count` | Max validation reverts before stopping auto-retry | Configured in ralphx.yaml |
| `pending_merge_stale_minutes` | Minutes before a PendingMerge task is considered stale | Configured in ralphx.yaml |

### Validation Command Format

Validation commands are defined in the project's analysis (auto-detected or custom). When `custom_analysis` is set, it takes **complete precedence** over `detected_analysis` — your customizations are trusted fully, including explicitly cleared fields like `worktree_setup: []`.

The format is:

```json
{
  "worktree_setup": [
    { "command": "npm install", "path": ".", "label": "Install dependencies" }
  ],
  "validate": [
    { "command": "npm run typecheck", "path": ".", "label": "Type Check" },
    { "command": "cargo clippy --all-targets", "path": "src-tauri", "label": "Clippy" },
    { "command": "cargo test --lib", "path": "src-tauri", "label": "Tests" }
  ]
}
```

- **worktree_setup**: Commands that prepare the worktree (install dependencies, create symlinks). Non-fatal failures produce warnings.
- **validate**: Commands that verify code quality. Failures are handled according to the validation mode.

---

## See Also

- [Execution Pipeline](execution.md)
- [Agent Orchestration](agent-orchestration.md)
- [Configuration](configuration.md)
