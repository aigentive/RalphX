---
paths:
  - "src-tauri/src/domain/state_machine/transition_handler/merge_outcome_handler.rs"
  - "src-tauri/src/domain/state_machine/transition_handler/merge_helpers.rs"
  - "src-tauri/src/application/reconciliation/handlers/merge.rs"
  - "src-tauri/src/application/startup_jobs.rs"
  - "src-tauri/src/commands/git_commands.rs"
  - "src-tauri/src/application/task_transition_service.rs"
  - "frontend/src/components/tasks/detail-views/MergeIncompleteTaskDetail.tsx"
---

> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# Merge Recovery Consistency

## Purpose

Prevent drift between merge failure classification, manual recovery actions, runtime reconciliation, startup remediation, and the user-facing MergeIncomplete UI.

## Non-Negotiables

| Rule | Detail |
|---|---|
| One failure classification | If a merge failure class needs special handling, identify it through shared helpers, not ad-hoc string checks in each caller. |
| All entry points stay aligned | Any behavior change for merge recovery must audit `merge_outcome_handler`, `retry_merge`, merge reconciliation, startup recovery, and MergeIncomplete UI copy/CTA semantics together. |
| Corrective failures reroute, not loop | Failures that require code changes (for example commit-hook / commit-time validation rejection) must route to `RevisionNeeded -> ReExecuting`, not back to `PendingMerge`. |
| Environment failures escalate, not block | Hook/worktree environment failures are operator intervention states. UI must say `Escalated` / environment action required, not `Blocked`, `Merging`, or a generic retry promise. |
| Retryable failures stay retryable | Only genuinely retryable merge failures (lock contention, transient git state, deferred target branch busy) should auto-retry through merge reconciliation. |
| Startup matches live behavior | If runtime reconciliation/manual recovery can heal a merge failure class, startup recovery must also repair persisted rows for that same class. |
| Manual CTA semantics must match backend action | If the primary user action no longer retries merge literally, update the MergeIncomplete UI text/CTA so the app does not lie about what will happen. |
| One shared reroute helper | Metadata writes (`restart_note`, durable diagnostics) plus corrective transition logic belong in one shared helper/service, not duplicated across handlers/commands/startup. |
| TDD across all coupled paths | Changes here require focused tests for the direct merge outcome, manual action path, reconciliation path, and startup path. |

## First Places To Check

| Concern | Files |
|---|---|
| Merge failure classification | `src-tauri/src/domain/state_machine/transition_handler/merge_outcome_handler.rs`, `src-tauri/src/domain/state_machine/transition_handler/merge_helpers.rs` |
| Shared corrective reroute | `src-tauri/src/application/task_transition_service.rs` |
| Live auto-repair | `src-tauri/src/application/reconciliation/handlers/merge.rs` |
| Restart-time repair | `src-tauri/src/application/startup_jobs.rs` |
| Manual user recovery | `src-tauri/src/commands/git_commands.rs`, `frontend/src/components/tasks/detail-views/MergeIncompleteTaskDetail.tsx` |
| Regression coverage | `src-tauri/src/domain/state_machine/transition_handler/tests/merge_outcome_transient_retry_tests.rs`, `src-tauri/tests/suite_interactive_process/reconciliation_runner.rs`, `src-tauri/tests/suite_transition_git/startup_jobs_runner.rs`, `src-tauri/src/commands/git_commands.rs` |
