# "Agent Ended Without Completing All Task Steps" — False-Negative Completion Detection

## Summary

A task that genuinely finished its work — called `execution_complete`, captured a GREEN
`validation_cache`, then called `complete_agent_task` — was nonetheless marked **Failed** with
`last_agent_error: "Agent ended without completing all task steps"`, then auto-retried into
`max_retries_exceeded`.

The root cause is a **false-negative completion check** in
`handle_stream_success` (`src-tauri/src/application/chat_service/chat_service_handlers.rs`).
When the worker stream ends, the backend decides PendingReview-vs-Failed using
`execution_completion_action(...)`. The "are steps being tracked?" argument is passed as
`task_step_repo.is_some()` (chat_service_handlers.rs:918) — which is **always `true` in
production** because the repo is always wired. The predicate then requires *all steps completed*
and ignores `has_output`. For a task with **zero steps**, `all_steps_completed()` returns `false`
by design (it has an explicit `!steps.is_empty()` guard, chat_service_handlers.rs:176), so the
branch resolves to `Failed` even though the agent produced output and explicitly signalled
completion.

This is a **completion-DETECTION** bug (the predicate misclassifies a successful run as
incomplete), and is distinct from the separate "failed-step-is-terminal" state-machine issue.
Here there were *no steps at all* and validation was GREEN — nothing was actually incomplete.

## Affected Tasks

| Task | Symptom |
|------|---------|
| `801e1660-e6c6-434b-8440-ae52aa30a5ef` ("CI pass 90%") | ZERO rows in `task_steps`; called `execution_complete` + `complete_agent_task`; GREEN `validation_cache` ("Granola coverage above 90%, clippy clean"); `last_agent_error = "Agent ended without completing all task steps"`, `last_agent_error_context = "execution"`; retries all `failure_source: "unknown"` / `reason_code: "unknown"` → `stop_retrying: true`, `max_retries_exceeded`. |
| `945c762d` | Same `last_agent_error: "Agent ended without completing all task steps"` string — same detection path. |

## Evidence

- **Error string** is emitted at `chat_service_handlers.rs:976`
  (`serde_json::json!("Agent ended without completing all task steps")`), inside the
  "else → Failed" branch of `handle_stream_success` (the only emit site; the other hit is the
  test at `chat_service_handlers_tests.rs:1150`).
- **Zero steps** → `all_steps_completed()` returns `false` because of the `!steps.is_empty()`
  guard (chat_service_handlers.rs:176).
- **GREEN validation** was captured by `execution_complete_http` (steps.rs:671-730) but is
  **never consulted** by the completion decision.
- **`execution_complete` does not transition the task** — it stores the validation cache, closes
  stdin via the IPR so the agent gets EOF, and defers the transition (steps.rs:642-645, 732-751,
  794). The doc comment is explicit: "State transition happens in `handle_stream_success` when
  the process exits." So the task is still in `Executing` when the stream ends, and
  `handle_stream_success` is the sole authority for the final transition.
- **`unknown` reason_code/failure_source**: the `Failed` on_enter handler finds no pre-written
  recovery metadata (because the success path only wrote `last_agent_error`, not
  `failure_error`/`execution_recovery`) and falls through to the fallback branch
  (`outcomes.rs:70-80`) which hard-codes `ExecutionRecoveryReasonCode::Unknown` +
  `ExecutionFailureSource::Unknown` with state `Retrying`.

## Root Cause Analysis

### The decision path (success / stream-end)

`handle_stream_success` (chat_service_handlers.rs:790) only runs its logic when the task is still
`Executing`/`ReExecuting` (chat_service_handlers.rs:825). Since `execution_complete` does **not**
transition (steps.rs:732-751), the task *is* still `Executing` when the worker exits — so this
block runs and owns the outcome.

```
chat_service_handlers.rs:915  let all_steps_done = all_steps_completed(task_step_repo, &task_id).await;
chat_service_handlers.rs:916  let completion_action = execution_completion_action(
chat_service_handlers.rs:917      has_output,
chat_service_handlers.rs:918      task_step_repo.is_some(),   // <-- BUG: "steps_tracked" == "repo exists", NOT "task has steps"
chat_service_handlers.rs:919      all_steps_done,
chat_service_handlers.rs:920  );
```

### The predicate

```
chat_service_handlers.rs:218  fn should_transition_task_execution_to_pending_review(
chat_service_handlers.rs:219      has_output: bool,
chat_service_handlers.rs:220      steps_tracked: bool,
chat_service_handlers.rs:221      all_steps_done: bool,
chat_service_handlers.rs:222  ) -> bool {
chat_service_handlers.rs:223      if steps_tracked {
chat_service_handlers.rs:224          all_steps_done      // <-- taken in prod (steps_tracked always true)
chat_service_handlers.rs:225      } else {
chat_service_handlers.rs:226          has_output          // <-- the path that WOULD have passed
chat_service_handlers.rs:227      }
chat_service_handlers.rs:228  }
```

`execution_completion_action` (chat_service_handlers.rs:236-247) returns `PendingReview` when the
above is true, else `Failed`.

### Why zero steps can never be "done"

```
chat_service_handlers.rs:174  match repo.get_by_task(task_id).await {
chat_service_handlers.rs:175      Ok(steps) => {
chat_service_handlers.rs:176          !steps.is_empty()                 // <-- zero steps => false
chat_service_handlers.rs:177              && steps.iter().all(|s| { ... Completed | Skipped })
```

### The fatal combination

For task 801e1660 (zero steps, GREEN validation, `execution_complete` called):

| Input | Value | Source |
|-------|-------|--------|
| `has_output` | `true` (agent produced output; success path) | chat_service_handlers.rs:794, and the cancellation success paths pass `true` at 1472 / 1552 |
| `steps_tracked` | `true` (repo is always `Some` in prod) | chat_service_handlers.rs:918 |
| `all_steps_done` | `false` (`!steps.is_empty()` fails) | chat_service_handlers.rs:176 |

Predicate takes the `steps_tracked` branch → returns `all_steps_done` = `false` → ignores
`has_output` → `ExecutionCompletionAction::Failed` → else-branch at chat_service_handlers.rs:950
runs → writes `last_agent_error` (chat_service_handlers.rs:967-991) and transitions the task to
`InternalStatus::Failed` (chat_service_handlers.rs:993).

**The intended meaning of `steps_tracked` is "this task is tracking steps" (i.e. it has at least
one step), but the call site passes "a step repository exists" (`task_step_repo.is_some()`),
which is unconditionally true.** That single mismatch turns "no steps + has output" from a
PendingReview into a Failed.

### Why `failure_source` / `reason_code` are "unknown" (not "agent_crash")

The success path transitions to `Failed` but writes only `last_agent_error*` — it does **not**
pre-write `failure_error` or an `execution_recovery` block. The `Failed` on_enter handler
(`src-tauri/src/domain/state_machine/transition_handler/on_enter_states/outcomes.rs:48-85`) checks
for pre-written recovery metadata; finding none and seeing no `"structural:"` marker, it falls
into the generic fallback:

```
outcomes.rs:71  recovery.append_event_with_state(
outcomes.rs:72      ExecutionRecoveryEvent::new(
outcomes.rs:73          ExecutionRecoveryEventKind::Failed,
outcomes.rs:74          ExecutionRecoverySource::System,
outcomes.rs:75          ExecutionRecoveryReasonCode::Unknown,                         // reason_code: "unknown"
outcomes.rs:76          "Failed without pre-written recovery metadata (fallback)",
outcomes.rs:77      )
outcomes.rs:78      .with_failure_source(ExecutionFailureSource::Unknown),            // failure_source: "unknown"
outcomes.rs:79      ExecutionRecoveryState::Retrying,                                 // -> drives auto-retry
outcomes.rs:80  );
```

By contrast, real crash/timeout paths classify before transitioning (e.g. the `AgentExit` error
path at chat_service_handlers.rs:2333-2351, and the git-isolation classifier at
task_transition_service.rs:3640-3647), so those tasks show `agent_crash` / specific codes. This
task gets `unknown`/`unknown` precisely because the success path treated a *successful* run as a
failure and routed it to `Failed` without any failure classification.

(Enum definitions: `ExecutionFailureSource` incl. `Unknown` at
`src-tauri/crates/ralphx-domain/src/entities/task_metadata.rs:667-682`;
`ExecutionRecoveryReasonCode` incl. `Unknown` at task_metadata.rs:732-757.)

## Why It Loops

1. `handle_stream_success` misclassifies the successful zero-step run → transitions to `Failed`
   (chat_service_handlers.rs:993).
2. `Failed` on_enter writes fallback recovery metadata with state `Retrying`
   (outcomes.rs:79).
3. The reconciler sees `Retrying` and auto-retries ("Auto-retrying execution (attempt 1/3) —
   previous failure: Unknown").
4. Each retry re-executes, the worker again finishes with zero steps + GREEN validation + calls
   `execution_complete`, and `handle_stream_success` again computes `all_steps_done == false`
   → `Failed` again. The misclassification is **deterministic**, so every retry reproduces it.
5. After 3 attempts → `MaxRetriesExceeded` → `stop_retrying: true`. Task ends permanently Failed.

Note: the `AgentExit`→PendingReview override (chat_service_handlers.rs:2335-2351) does not help —
it only fires on the `StreamError::AgentExit` *error* path, and it *also* gates on
`all_steps_completed()` (chat_service_handlers.rs:2338), so it would equally reject a zero-step
task.

## Recommended Fix

The fix should make completion detection treat a zero-step task that produced output and signalled
completion as **complete**, and should classify the failure reason rather than emitting `unknown`.

1. **Fix the `steps_tracked` semantics (primary).** `steps_tracked` must mean "this task has at
   least one tracked step", not "a step repo exists". Replace `task_step_repo.is_some()` at
   chat_service_handlers.rs:918 with an actual step-count check (e.g. a
   `task_has_tracked_steps(task_step_repo, &task_id)` helper returning
   `repo.get_by_task(...).map(|s| !s.is_empty())`). Then for a zero-step task the predicate
   (chat_service_handlers.rs:223-227) falls to the `else` branch and returns `has_output` →
   `PendingReview`. This is the minimal, surgical fix and matches the predicate's original intent.

2. **Honor GREEN `validation_cache` as a completion signal (defense in depth).** In
   `execution_completion_action` (or before it), if the worker captured a GREEN
   `ValidationCacheMetadata` matching HEAD (steps.rs:671-730 writes it; the structure is
   `task_metadata.rs` `ValidationCacheMetadata`), treat the run as complete even if step
   bookkeeping is empty/partial. This makes the decision robust to agents that validate-and-finish
   without per-step tracking.

3. **Record that `execution_complete` was called.** Because `execution_complete_http`
   (steps.rs:645) already knows the agent signalled completion, persist a marker
   (e.g. `execution_complete_called_at` in metadata, or set a flag the success path reads) so
   `handle_stream_success` can prefer `PendingReview` when completion was explicitly signalled,
   instead of inferring solely from step rows.

4. **Classify instead of emitting `unknown`.** When the success path *does* decide a run is
   genuinely incomplete, it should pre-write a meaningful `execution_recovery` classification
   (e.g. a dedicated `IncompleteSteps` reason) rather than leaving the `Failed` on_enter handler
   to fall back to `ExecutionRecoveryReasonCode::Unknown` / `ExecutionFailureSource::Unknown`
   (outcomes.rs:70-80). A non-transient incomplete-steps classification would also stop the
   pointless 3x auto-retry loop for cases that retrying cannot fix.

5. **Tests (TDD).** Add coverage in `chat_service_handlers_tests.rs` for:
   `execution_completion_action(has_output=true, has_steps=false, all_steps_done=false)` →
   `PendingReview`; zero-step + GREEN validation → `PendingReview`; and a regression that the
   `Failed` path (if still reachable) writes a classified reason, not `Unknown`.

## Open Questions

- Should a zero-step worker run *ever* be considered a failure on the success path, or should the
  absence of steps always defer to `has_output` / validation state? (Affects whether fix #1 alone
  is sufficient or whether the `Failed` branch should be unreachable for zero-step + has_output.)
- Is the zero-step condition itself expected for "CI pass 90%"-style tasks (worker completes
  validation work without registering steps), or should the worker prompt require at least one
  `start_step`/`complete_step`? If steps are expected, there is a *second* upstream bug (worker
  never created steps) worth a separate handoff.
- The `AgentExit` override (chat_service_handlers.rs:2335-2351) shares the same
  `all_steps_completed()` gate — should it also adopt the has_output / validation-cache fallback,
  so a zero-step agent that exits via signal is not double-penalized?
- Does any caller pass `has_output=false` on a real success (e.g. a worker that produces no
  assistant text but still calls `execution_complete`)? If so, fix #2/#3 (validation cache /
  completion marker) are required in addition to #1.
