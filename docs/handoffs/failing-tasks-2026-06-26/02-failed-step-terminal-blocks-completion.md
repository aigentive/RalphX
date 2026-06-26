# Failed Step Is Terminal — Blocks Task Completion Despite GREEN Validation

## Summary

When a worker agent hits a `failed` task_step (e.g. a transient disk-full ENOSPC), it cannot clear that step: the MCP step tools (`complete_step` / `skip_step`) reject any transition out of `failed` with `400 Bad Request`, and `failed` is a terminal step status. Workers work around this by adding NEW steps and completing those, but the original `failed` step persists. The execution-completion gate in `handle_stream_success` requires that **every** task_step be `Completed` or `Skipped` (`all_steps_completed`); a single lingering `failed` step forces the gate to route the task to `InternalStatus::Failed` instead of `pending_review` — and the GREEN `validation_cache` captured by `execution_complete` is **never consulted** in that decision. The reconciliation auto-retry loop then re-runs the task; each attempt either re-fails or leaves a non-completed step, so the gate fails again until `stop_retrying=true` ("Max retries exceeded") and the task is permanently stuck in `failed`.

## Affected tasks

| Task | Title | Symptom |
|------|-------|---------|
| `945c762d-6da4-4bc7-9f2e-811e91e2996f` | Regression Testing | Steps mostly `completed`, two ENOSPC-failed steps superseded by new `completed` steps; `execution_complete` stored GREEN cache (commit `4b2b44ee`, 11/11 integration tests, clippy clean). Task still `failed`. Agent note cites memory `[[failed-step-terminal-no-cleanup]]`. |
| `801e1660` | CI pass 90% | GREEN validation_cache (`tests_ran=true`, `tests_passed=true`), called `execution_complete`, still `failed`. |
| `40f6acc4` | P6 | Captured validation_cache has `tests_ran=false`, `tests_passed=true`; this is not proof of completed work and should not be rescued by a validation-cache override. Its final failures are primarily the worktree/dependency loop covered in reports 01 and 04. |

The true validation-cache completion-gate fingerprint applies to `945c762d` and `801e1660`: agent did the work + recorded a HEAD-matched `tests_ran=true/tests_passed=true` cache, but the task is parked in `internal_status='failed'` with `stop_retrying=true`. P6 shares the dirty-step/no-progress symptoms but intentionally fails the stricter cache-proof test.

## Evidence

1. **GREEN validation cache IS captured and stored** — `execution_complete_http` writes the cache to task metadata, keyed by HEAD SHA:
   - `src-tauri/src/http_server/handlers/steps.rs:671-730` — builds `ValidationCacheMetadata { tests_ran, tests_passed, test_summary, captured_by: "execution_complete", ... }` and calls `task_repo.update_metadata(...)`.
2. **`execution_complete` does NOT itself transition the task** — it only closes the worker's stdin via the interactive process registry; the real transition happens later in the stream-success handler:
   - `src-tauri/src/http_server/handlers/steps.rs:732-751` + comment at `:642-644` ("State transition happens in `handle_stream_success` when the process exits").
3. **The transition handler ignored the validation cache** — before the current patch, `handle_stream_success` read only step statuses + `has_output`; there was no read of `validation_cache` anywhere in the completion decision (see Root cause). A real green cache (`tests_ran=true`, `tests_passed=true`) on HEAD had zero effect while a step was `failed`.
4. **Persisting `failed` + max-retries** — the reconciliation loop sets `stop_retrying=true` with reason `"Max retries exceeded — stopping auto-retry"` once the per-status retry budget is exhausted (see "Why it loops forever").

## Root cause analysis

### A. The completion gate requires ALL steps Completed/Skipped

`handle_stream_success` (defined in `src-tauri/src/application/chat_service/chat_service_handlers.rs`, TaskExecution branch ~`:821-1010`) decides the post-execution state with three helpers:

- **`all_steps_completed`** — `chat_service_handlers.rs:167-190`:
  ```rust
  Ok(steps) => {
      !steps.is_empty()
          && steps.iter().all(|s| {
              s.status == TaskStepStatus::Completed || s.status == TaskStepStatus::Skipped
          })
  }
  ```
  Any step that is `Failed` (or even `Pending`/`InProgress`) makes this return `false`.

- **`should_transition_task_execution_to_pending_review`** — `:218-228`: when steps are tracked (`task_step_repo.is_some()`), the result is **exactly** `all_steps_done` — output/validation are irrelevant.

- **`execution_completion_action`** — `:236-246`: `PendingReview` when the above is true, else `ExecutionCompletionAction::Failed`.

The dispatch — `:915-1001`:
```rust
let all_steps_done = all_steps_completed(task_step_repo, &task_id).await;   // :915
let completion_action = execution_completion_action(has_output, task_step_repo.is_some(), all_steps_done); // :916-920
...
} else {   // :950  -> Failed branch
    // writes last_agent_error = "Agent ended without completing all task steps"  :973-986
    transition_service.transition_task(&task_id, InternalStatus::Failed).await   // :993-994
}
```
So a single `failed` (or non-`Completed`/`Skipped`) step → `executing → failed`. **The validation_cache is never read in this path.**

> Note: a second instance of this gate exists for another completion route at `chat_service_handlers.rs:2335-2338` (`all_steps_completed` re-checked when `target_status == Failed`) — same semantics, same blind spot.

### B. A `failed` step is immutable via the MCP step tools

`failed` is a terminal step status:
- `src-tauri/crates/ralphx-domain/src/entities/task_step.rs:28-38` — `TaskStepStatus::is_terminal()` returns true for `Completed | Skipped | Failed | Cancelled`.

The HTTP step handlers reject any move out of `failed` because they validate the *incoming* status, and `failed` matches none of the allowed source states — all in `src-tauri/src/http_server/handlers/steps.rs`:

| Handler | Guard | Effect on a `failed` step |
|---------|-------|---------------------------|
| `start_step_http` | `if step.status != TaskStepStatus::Pending → BAD_REQUEST` (~`:73-74`) | rejected (not Pending) |
| `complete_step_http` | `if step.status != TaskStepStatus::InProgress → BAD_REQUEST` (~`:128-129`) | rejected (not InProgress) |
| `skip_step_http` | `if status != Pending && status != InProgress → BAD_REQUEST` (~`:241-242`) | rejected (neither) |
| `fail_step_http` | `if step.status != TaskStepStatus::InProgress → BAD_REQUEST` (~`:354-355`) | rejected (already failed) |

`add_step_http` (~`:391-456`) simply `create()`s a brand-new step; it does **not** mark, link, or resolve the prior `failed` step. So "superseding" leaves the old `failed` row in place, which is exactly what keeps `all_steps_completed` returning `false`.

### C. The only "corrective" clearing is gated on a manual restart

There IS a clearing routine, but it is reachable only by the manual failed-task restart command, not by `execution_complete` and not as a per-step correction:
- `src-tauri/src/commands/task_commands/mutation.rs:23-49` — `clear_failed_steps_for_failed_restart()` resets `Failed` steps to `Pending`.
- Called only when `old_status == InternalStatus::Failed` during a status mutation (`mutation.rs:396-414`), and pairs with a `preserve_steps` one-shot flag.

The auto-retry path (Section "Why it loops forever") does NOT set `preserve_steps`, so on re-entry it instead goes through the blanket reset:
- `src-tauri/src/domain/state_machine/transition_handler/on_enter_states/execution.rs:203-259` — `reset_stale_steps_on_entry`: if `preserve_steps` is set, returns early (manual restart); otherwise calls `step_repo.reset_all_to_pending(...)`. Invoked on Executing/ReExecuting entry at `execution.rs:662` and `:712`.
- `src-tauri/src/infrastructure/sqlite/sqlite_task_step_repo.rs:264-276` — `reset_all_to_pending` runs `UPDATE task_steps SET status='pending' ... WHERE task_id=?2 AND status != 'pending'` (resets Failed AND Completed alike).

This is the subtle part: on each auto-retry the failed steps DO get reset to `Pending` — but that does not save the task, because (a) `Pending` is still not `Completed`/`Skipped`, so if the re-run worker calls `execution_complete` without re-completing every step the gate fails on `Pending`, and (b) it also wipes the previously-`Completed` step progress, so the work has to be re-done from scratch each attempt, re-creating the same failure (e.g. recurring disk-full or the same supersede-with-failed-step pattern).

### D. validation_cache is never a gate input

Searches across the completion/transition path showed the cache was write-only here: stored at `steps.rs:671-730`, never read by `all_steps_completed` / `execution_completion_action` / the dispatch, and never read in `reconciliation/` retry decisions. A real green cache on the exact HEAD SHA provided no override.

## Why it loops forever

1. Worker finishes; one or more steps are `failed` (or left non-`Completed`). `execution_complete` stores a GREEN cache and closes stdin.
2. Process exits → `handle_stream_success`: `all_steps_completed == false` → `execution_completion_action == Failed` → `transition_task(Failed)` (`chat_service_handlers.rs:915-994`). `last_agent_error = "Agent ended without completing all task steps"` is written.
3. The reconciliation loop picks up the `failed` task and auto-retries (all in `src-tauri/src/application/reconciliation/handlers/execution.rs`):
   - `reconcile_failed_execution_task` skips early if `stop_retrying` is already set (`~:939`).
   - It counts prior auto-retries and compares against the budget (`retry_count >= max_retries`, `~:1089`, and the git-isolation variant `~:1065`).
   - Under budget → cleans git state and `transition_task(Failed → Ready)` (`~:1308`); the scheduler then drives `Ready → Executing`, which fires `reset_stale_steps_on_entry` (Section C) and respawns the worker.
   - Per-status retry caps: `default_reconciliation_executing_max_retries()` etc. in `src-tauri/src/application/harness_runtime_registry.rs:878-900` (executing = 5, reviewing/qa = 3).
4. Each re-run re-hits the same wall (re-fails a step, or leaves reset `Pending` steps non-completed) → back to `failed` (step 2).
5. When `retry_count >= max_retries`, the loop sets `recovery.stop_retrying = true` (`execution.rs:~1096`; for the executing-status path the message `"Max retries exceeded — stopping auto-retry"` is emitted at `~:785` with the flag set at `~:787`). The early-skip guard at `~:939` then makes the task permanently inert in `failed`.

Net: a task whose work is genuinely complete and validated GREEN is condemned to burn its retry budget and end permanently `failed`, purely because one immutable `failed` step keeps the all-steps gate red.

## Recommended fix

Pick (preferably) both a gate fix and a step-clearing fix; cite points map to the code above.

1. **Treat superseded `failed` steps as non-blocking in the completion gate.** In `all_steps_completed` (`chat_service_handlers.rs:167-190`), a `failed` step should not veto completion when there is later evidence the work was redone. Concrete options:
   - Add a `superseded`/`resolved` marker on steps and have `add_step` (or a new `supersede_step`) set it on the prior `failed` step; treat `superseded` like `Skipped` in the gate predicate. This requires a schema field + a way for the worker to declare the supersede relationship.
   - Or change the predicate to ignore `Failed` steps that have a newer `Completed` step covering the same scope.
2. **Let only a real GREEN `validation_cache` on HEAD override the step gate.** The cache is already keyed by `commit_sha` (`steps.rs:676-684`). In the `else` (Failed) branch at `chat_service_handlers.rs:950-994`, before transitioning to `Failed`, check for a `ValidationCacheMetadata` whose `commit_sha == current HEAD`, `tests_ran=true`, and `tests_passed=true`; if present, route to `pending_review` instead. This makes the validated-but-step-dirty case converge while excluding P6-style no-test self-blocks.
3. **Provide a real per-step corrective path out of `failed`.** Today clearing only happens via the manual restart command (`mutation.rs:23-49`) or the blanket `reset_all_to_pending` (`sqlite_task_step_repo.rs:264-276`, which also destroys completed progress). Add a corrective MCP/HTTP action (analogous to `transition_task_corrective`) that resets a specific `failed` step to `pending`/`skipped` without wiping sibling `completed` steps, so a worker can legitimately clear a transient ENOSPC failure mid-run rather than orphaning it.
4. **Stop wiping completed progress on auto-retry.** `reset_stale_steps_on_entry` (`on_enter_states/execution.rs:203-259`) only preserves steps on the manual `preserve_steps` flag. The reconciliation Failed→Ready→Executing path should also preserve `Completed` steps (reset only `Failed`/`InProgress`), matching `clear_failed_steps_for_failed_restart` semantics, so retries build on prior work instead of redoing (and re-failing) it.

Fix 2 resolves the validated dirty-step class for 945c762d going forward; combined with the zero-step fix in report 03, it also resolves 801e1660. P6 still needs the worktree/dependency-loop fixes because its cache has `tests_ran=false`.

## Open questions

1. **Manual unstick for the 3 live tasks:** does the manual restart command (`mutation.rs` Retry: `failed → ready`, which fires `clear_failed_steps_for_failed_restart`) currently work for these, or does `stop_retrying=true` also block the manual `Retry` event? Need to confirm whether the early-skip at `execution.rs:~939` and the `stop_retrying` flag affect the user-initiated `Retry` path or only the auto-retry path.
2. **Scope-equivalence for supersede detection (fix 1 option b):** is there enough metadata on a step to know a later `Completed` step "covers" an earlier `failed` one, or must supersession be explicit?
3. **Cache trust window (fix 2):** the cache is keyed by HEAD SHA at `execution_complete` time, but `reset_stale_steps_on_entry` + retries can advance/clean the branch. How do we guarantee the GREEN cache's `commit_sha` still matches the branch HEAD at the moment the override is evaluated? (`auto_commit_on_execution_done` on exit may also move HEAD.)
4. **`has_output` semantics for untracked-step tasks:** for tasks with no steps (`task_step_repo` empty / returns empty), the gate falls back to `has_output` (`should_transition_task_execution_to_pending_review:225-226`). Should the validation-cache override (fix 2) also cover that branch for consistency?
5. **Reviewing/QA parity:** the same "terminal step blocks gate" reasoning may apply to the reviewing/QA retry caps (`execution.rs:~1455`, `~1563`). Worth confirming those lanes don't have an analogous immutable-state trap.
