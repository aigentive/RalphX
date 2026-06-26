# Current Branch Investigation Report

Branch: `fix/execution-completion-gate-validation-cache`  
Base: `origin/main` merge-base `342a35d9f32beac21e845fc51dfac230a68eca41`  
Method: 20 read-only subagents reviewed disjoint slices, then findings were aggregated.

## Implementation Update - 2026-06-26

The merge-blocking completion-gate spine from `implementation-spec.md` is now implemented on this branch:

- Latest execution episode lookup: `TaskRepository::get_status_last_entered_at` now returns the newest status-history entry for a status in SQLite, memory, and test doubles.
- Attempt authority: completion/error finalizers now resolve `Current`, `Stale`, or `IdentityUnknown` attempts before acting. Stale finalizers return without mutating the task; identity-unknown paths proceed only through the fail-closed step gate and do not use validation-cache rescue.
- Validation cache scoping: green cache evidence is trusted only when it matches the current HEAD and `captured_at >=` the latest `Executing` / `ReExecuting` episode entry. This is applied to both finalizer rescue and the shared `get_task_context` validation hint reader.
- Step gate: step reads are tri-state (`NoSteps`, `AllComplete`, `Incomplete`, `Unknown`) so repository errors and missing step repos fail closed unless same-episode validation evidence proves completion.
- Zero-step completion: output alone no longer advances execution. Zero-step tasks require an `execution_complete` signal or same-episode validation evidence.
- Prompt/schema alignment: execution-worker prompts now tell agents to omit `test_result` when no tests ran, matching the Rust/MCP schema.
- Narrow path-safety hardening: `validated_completion_override` validates the DB-backed task worktree path before reading Git HEAD.

Still not included in this completion-gate commit:

- The broader WI-5 setup/path-containment work for merge-validation setup/install sinks and hard setup-failure surfacing.
- F6 external completion event ordering/compensation.
- F10 lifecycle cache clears and recovery-source fidelity cleanup.
- The unrelated dirty generated sourcemap remains excluded from this fix.

Validation run for this implementation:

- Passing: handler completion-gate suite, memory/SQLite latest-entry regressions, `http_helpers`, `chat_service_streaming`, `execution_types_serde`, runtime config tests, affected test-double integration targets, `steps_handlers`, doctests, and scoped lib clippy after allowing known unrelated lint categories.
- Not passing for unrelated branch health: full `ralphx-domain` suite is red on existing `WaitingOnPr` status expectations and `affected_paths` row fixtures; full clippy with `-D warnings` is red on existing warnings outside this change.

## Debate Summary

Strongest pro-merge argument:
- The branch is aimed at a real failure class: execution could be marked failed even when a completed worker had green validation evidence. It adds incomplete-run classification, validation-cache rescue paths, setup-before-spawn ordering, and tests around zero-step/cache behavior.

Strongest blocking argument:
- The branch currently replaces one false-negative failure mode with several false-positive completion paths. The completion gate is not attempt-scoped, output alone can satisfy zero-step completion, setup success is not a reliable worktree-readiness proof, and some emitted completion events happen before final backend enforcement.

Current recommendation:
- Treat the completion-gate blockers as fixed once the validation commands in this branch pass. Keep WI-5/F6/F10 tracked separately; do not claim those broader hardening items are fixed by this commit.

## Blocking Findings

1. Critical: stale execution attempts can complete newer attempts.
- `chat_service_handlers.rs` uses `get_status_entered_at` for current-attempt guarding, but SQLite/memory return the earliest time a task entered that status, not the latest.
- A stale finalizer from attempt A can pass the guard after attempt B re-enters `executing`/`re_executing`, then transition B to review.
- Refs: `src-tauri/src/application/chat_service/chat_service_handlers.rs:935`, `:1049`, `:2476`; `src-tauri/src/infrastructure/sqlite/sqlite_task_repo/mod.rs:523`; `src-tauri/src/infrastructure/memory/memory_task_repo/mod.rs:207`.

2. High: `validation_cache` is task-level, HEAD-only, and not current-attempt scoped.
- Multiple agents confirmed the same issue: a green cache from attempt A can rescue a later failed/no-output/incomplete attempt B at the same HEAD.
- It is not cleared on execution/re-execution/restart, has no run/attempt id, and can be checked before exit auto-commit, so dirty unvalidated work can be committed after stale validation.
- Refs: `src-tauri/src/application/chat_service/chat_service_handlers.rs:231`, `:1010`, `:2476`; `src-tauri/src/http_server/handlers/steps.rs:670`; `src-tauri/crates/ralphx-domain/src/entities/task_metadata.rs:800`; `src-tauri/src/domain/state_machine/transition_handler/on_enter_states/execution.rs:199`.

3. High: step repository errors fail open.
- `has_tracked_steps()` collapses repo/query errors into `false`; completion then treats the task like a zero-step task and falls back to `has_output`.
- A transient DB error can advance a tracked-step task to review without proving steps are complete.
- Refs: `src-tauri/src/application/chat_service/chat_service_handlers.rs:203`, `:305`, `:1010`.

4. High: zero-step output can advance without `execution_complete`.
- `has_output && !steps_tracked` can produce `PendingReview`; the finalizer does not require a persisted/propagated completion-tool signal.
- A worker that exits 0 with explanatory text but never calls `execution_complete` can reach review.
- Refs: `src-tauri/src/application/chat_service/chat_service_handlers.rs:1010`, `:1037`; `src-tauri/src/application/chat_service/chat_service_send_background.rs:1304`; `src-tauri/src/application/chat_service/chat_service_streaming.rs:912`.

5. High: worktree setup readiness is incomplete.
- Setup is ordered before spawn, but `worktree_setup` failures are ignored when computing pre-exec setup success; Block/AutoFix only block install failures.
- Analysis `path` values and symlink target parents flow into filesystem/process sinks without containment validation; DB-backed `task.worktree_path` also flows into setup/git `current_dir` without sink-local validation.
- Refs: `src-tauri/src/domain/state_machine/transition_handler/merge_validation/install.rs:72`, `:128`, `:388`, `:422`; `merge_validation/setup.rs:348`, `:440`; `src-tauri/src/domain/state_machine/transition_handler/on_enter_states/execution.rs:89`, `:106`; `src-tauri/src/application/chat_service/chat_service_handlers.rs:262`.

6. High: completion is emitted before the failed-step gate.
- `execution_complete_http` removes IPR, emits/persists completion, publishes webhook, and returns before `handle_stream_success` can fail the task for failed/incomplete steps.
- External observers can see completion for a run the backend later marks failed.
- Refs: `src-tauri/src/http_server/handlers/steps.rs:732`, `:753`, `:764`; `src-tauri/src/application/chat_service/chat_service_handlers.rs:167`, `:1010`, `:1130`.

7. High: prompt/tool contract mismatches.
- Claude worker prompt tells agents to send `test_result: { tests_ran: false }`, but MCP/Rust require `tests_passed` when `test_result` is present.
- Codex worker prompt advertises native delegation for execution coder sub-scopes, but live `delegate_start` is still ideation-scoped and may not preserve the task execution worktree.
- Refs: `agents/ralphx-execution-worker/claude/prompt.md:182`; `plugins/app/ralphx-mcp-server/src/step-tools.ts:199`; `src-tauri/src/http_server/types.rs:1941`; `agents/ralphx-execution-worker/codex/prompt.md:17`; `plugins/app/ralphx-mcp-server/src/ideation-tools.ts:577`.

## Documentation And Test Issues

- The new handoff docs still describe several branch-implemented fixes as future work; rewrite them as historical failure analysis plus remaining recovery work.
- `04-p6-p5-dependency-deadlock.md` says corrective `Merged` should auto-unblock dependents, but corrective transitions skip normal entry actions.
- Several new tests prove helper behavior instead of production entry paths; they do not prove setup-before-spawn, Block preventing spawn, or ReExecuting setup.
- Some tests direct-set `internal_status` and use hard-coded run ids, bypassing current-attempt behavior.
- The dirty `plugins/app/ralphx-mcp-server/build/__tests__/cross-project-guide.test.js.map` is unrelated generated sourcemap churn and should not be committed.

## Validation Notes

Subagents reported these passing targeted checks:
- `cargo test --manifest-path src-tauri/Cargo.toml -p ralphx-domain task_metadata --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml failure_source_to_reason_code_maps_agent_incomplete --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml pre_execution_setup_ --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml validation_cache --lib`

Recommended next validation after fixes:
- Use the `ralphx-domain` manifest for task metadata tests; root `src-tauri/Cargo.toml --lib` filters will not run those.
- Use parent module filters for sidecar tests, e.g. `application::chat_service::chat_service_handlers::tests::...`.
- Add focused regressions for stale attempts, stale validation cache across re-entry/restart, step repo errors, output-only zero-step completion, setup failure blocking, and path containment.
