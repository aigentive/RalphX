# Failing Tasks Handoff — RalphX dev DB (2026-06-26)

Analysis of all failing/blocked tasks in the dev database (`src-tauri/ralphx.db`).
Initial investigation snapshot: HEAD `b9389f011`, branch `fix/clickup-ticket-filtering`, project `062abe49` (RalphX self-hosting).
Rechecked from checkout `fix/execution-completion-gate-validation-cache` at HEAD `1f240a652` on 2026-06-26; the DB task counts and task-level evidence below still match.

## What's failing

The DB has **3 `failed` tasks** and **3 `blocked` tasks**. The blocked tasks are *not* independent problems — each is only waiting on a failed task:

| Status | Task | id | Waiting on |
|--------|------|----|-----------|
| failed | P6 — "Has PR" indicators + open affordances (DB-only) | `40f6acc4` | — |
| failed | CI pass 90% | `801e1660` | — |
| failed | Regression Testing: full Rust + frontend suites | `945c762d` | — |
| blocked | P8 — Regression testing across all modified paths | `2a610da3` | P6 |
| blocked | Merge plan into main (plan ec6dc874) | `6e952493` | P6, P8 |
| blocked | Merge plan into main (plan 829560b6) | `d462493b` | Regression Testing |

So the real work is the **3 failed tasks**, which decompose into **4 distinct underlying issues**. Two of them (#2 and #3) are two triggers of the **same completion-gate defect** in `chat_service_handlers.rs`; the other two are an environment-provisioning defect (#1) and a per-task data/planning problem (#4).

## The four issues

| # | Report | Issue | Tasks affected | Layer |
|---|--------|-------|----------------|-------|
| 1 | [01-worktree-environment-incompleteness.md](01-worktree-environment-incompleteness.md) | Task worktrees are bare `git worktree add` checkouts; `frontend/node_modules` symlink + `.artifacts/specs/**`/`tracker.md` are never provisioned, so the worker's bash probes crash → `AgentExit` → retry on the same broken worktree forever. | 40f6acc4 (root); a latent hazard for every frontend/spec task | Execution env / git_service |
| 2 | [02-failed-step-terminal-blocks-completion.md](02-failed-step-terminal-blocks-completion.md) | `handle_stream_success` gates `executing → pending_review` on **all** `task_steps` being `Completed`/`Skipped` and historically never consulted the HEAD-matched `validation_cache`. A single lingering `failed` step is immutable (`failed` is terminal; step MCP tools return 400 for any move out), so the task is forced to `Failed` and auto-retries to `max_retries_exceeded`. | 945c762d (primary); 40f6acc4 has the same dirty-step/no-progress shape but only a `tests_ran=false` cache, so it must not be rescued by validation-cache override alone | State machine / completion gate |
| 3 | [03-agent-ended-without-completing-steps.md](03-agent-ended-without-completing-steps.md) | Same gate, different trigger: the `!steps.is_empty()` guard means a **zero-step** task can never satisfy `all_steps_completed()`, so a successful run with real GREEN validation is marked `Failed` with message "Agent ended without completing all task steps" and `reason_code: unknown`. | 801e1660 (zero steps), 945c762d | State machine / completion gate |
| 4 | [04-p6-p5-dependency-deadlock.md](04-p6-p5-dependency-deadlock.md) | P6 self-blocked ~30× on a "missing P5 PR-detail UI" prerequisite that is **already satisfied** (P5 `465e9fc0` is `merged`; its commit `dc8c54e5` is an ancestor of the P6 worktree HEAD; files physically present). Worker mis-reasoning + the #1 broken worktree drove the crash-retries. Feature likely already shipped via PR #481 (`19cf8a29b`). | 40f6acc4 (+ downstream 2a610da3, 6e952493) | Data / worker planning |

## Convergent root cause (issues 2 & 3)

Both #2 and #3 live in `src-tauri/src/application/chat_service/chat_service_handlers.rs`:
- The completion predicate `all_steps_completed()` (~`:176`) requires `!steps.is_empty()` **and** every step `Completed`/`Skipped`.
- It is invoked with `task_step_repo.is_some()` (always true in prod, ~`:918`), so both *empty step lists* (issue 3) and *lingering failed steps* (issue 2) fall through to the `Failed` branch (~`:976`, the "Agent ended without completing all task steps" string).
- Neither branch historically consulted the `validation_cache` that `execution_complete` already captured (`steps.rs:642-751`). DB recheck: `801e1660` and `945c762d` have `tests_ran=true/tests_passed=true` caches matching their worktree HEADs; `40f6acc4` has `tests_ran=false/tests_passed=true` and should not count as validated completion.
- The `Failed` on-enter fallback (`outcomes.rs:70-80`) sets `reason_code: unknown` and routes to `Retrying`, producing the 3× retry → `max_retries_exceeded` loop.

A single completion-gate fix here (honor only a HEAD-matched cache with `tests_ran=true/tests_passed=true`, and treat "has tracked steps" rather than "step repo exists") resolves the 801e1660/945c762d loop class. P6 still needs the worktree/dependency-loop fixes because its cache says no tests ran.

## Recommended sequencing

1. **Unblock now (data):** verify P6 equivalence vs PR #481, then close P6 (`40f6acc4`) via the canonical `TaskTransitionService` so P8 and the Merge task unblock (report 04). Do the same triage for 801e1660 / 945c762d once #2/#3 are fixed, or correct them manually.
2. **Fix the completion gate (issues 2 & 3):** one change in `chat_service_handlers.rs` — see reports 02 & 03. Highest leverage; stops the infinite-retry class of failure.
3. **Fix worktree provisioning (issue 1):** ensure `frontend/node_modules` symlink + required runtime dirs exist before execution regardless of `merge_validation_mode`, and stop instructing workers to read main-checkout-only paths (`.artifacts/specs/**`) from a branch worktree. See report 01.
4. **Harden:** dependency reconciliation so a satisfied/merged prerequisite cannot keep a task self-blocked (report 04).

## Method note

Findings were produced by four parallel read-only investigation subagents (one per issue), each citing `file:line` from the actual codebase / concrete DB query results. No code was modified. Per-issue detail, evidence, and fix proposals are in the linked reports.
