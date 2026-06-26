# Current Branch Investigation Tracker

## Scope
- Branch: `fix/execution-completion-gate-validation-cache`
- Base: `origin/main` merge-base `342a35d9f32beac21e845fc51dfac230a68eca41`
- Goal: run 20 focused subagents against current-branch changes, aggregate debate, and report discovered issues.
- Aggregate report: `.artifacts/specs/current-branch-investigation/report.md`

## Files Under Review
- `agents/ralphx-execution-coder/codex/prompt.md`
- `agents/ralphx-execution-worker/codex/prompt.md`
- `docs/handoffs/failing-tasks-2026-06-26/*.md`
- `src-tauri/CLAUDE.md`
- `src-tauri/crates/ralphx-domain/src/entities/task_metadata.rs`
- `src-tauri/crates/ralphx-domain/src/entities/task_metadata_tests.rs`
- `src-tauri/src/application/chat_service/chat_service_handlers.rs`
- `src-tauri/src/application/chat_service/chat_service_handlers_tests.rs`
- `src-tauri/src/application/reconciliation/metadata.rs`
- `src-tauri/src/application/reconciliation/metadata_tests.rs`
- `src-tauri/src/domain/state_machine/transition_handler/on_enter_states/execution.rs`
- `src-tauri/src/domain/state_machine/transition_handler/tests/transitions_agents.rs`

## Subagent Roster
- Completed:
  - 01 Russell: execution state entry/completion gate
  - 02 Epicurus: chat-service completion/setup handling
  - 03 Zeno: domain task metadata
  - 04 Ampere: reconciliation metadata
  - 05 Mendel: validated transition discipline
  - 06 Locke: chat-service tests
  - 07 Dalton: state-machine transition tests
  - 08 Hooke: Codex prompt contracts
  - 09 Laplace: handoff docs
  - 10 Huygens: concurrency/idempotency
  - 11 Kuhn: path safety
  - 12 Kierkegaard: provider neutrality
  - 13 Planck: validation-cache semantics
  - 14 Heisenberg: failed-step behavior
  - 15 Newton: worktree setup
  - 16 Pascal: dependency deadlock docs
  - 17 Peirce: test strategy
  - 18 Ptolemy: Rust quality
  - 19 Lagrange: generated artifact hygiene
  - 20 Volta: adversarial whole-branch review

## Findings
- 20 Volta/adversarial whole-branch:
  - Pro-merge argument: branch addresses the false-negative completion loop by distinguishing task step existence, allowing green validation cache rescue, classifying incomplete runs as `AgentIncomplete`, and moving pre-exec setup before worker spawn.
  - HIGH: zero-step tasks can advance to review without real `execution_complete`; `has_output && !steps_tracked` is enough, while stream outcome does not carry the known completion-tool flag. Reported refs: `chat_service_handlers.rs:1010`, `:1037`; `chat_service_send_background.rs:1304`; `chat_service_streaming.rs:912`.
  - HIGH duplicate/expanded: worktree setup still does not prove usability before agent start because analysis can be absent/empty/malformed, custom analysis can omit symlink setup, and setup-phase failures are ignored. Reported refs: `merge_validation/install.rs:333`, `:351`, `:388`, `:422`; `merge_validation/setup.rs:440`; `agents/ralphx-execution-worker/codex/prompt.md:13`.
  - MEDIUM: no-test validation caches can combine badly with broadened zero-step path; completion override rejects `tests_ran=false`, but output-only zero-step can still advance and reviewer context may receive `skip_test_validation`. Reported ref: `src-tauri/src/http_server/helpers.rs:1575`.
  - MEDIUM duplicate: existing `stop_retrying=true` failed tasks remain stuck; startup/failed reconciliation do not repair them. Reported refs: `docs/handoffs/failing-tasks-2026-06-26/02-failed-step-terminal-blocks-completion.md:11`; `reconciliation/handlers/execution.rs:212`, `:938`; `00-OVERVIEW.md:43`.
- 18 Ptolemy/Rust quality:
  - HIGH duplicate/confirmation: validation-cache override is not scoped to current execution attempt; stale green cache at same HEAD can move later zero-output/no-step run to review. Reported refs: `chat_service_handlers.rs:231`, `:329`; `chat_service_handlers_tests.rs:493`.
  - HIGH duplicate/confirmation: step-repo query errors collapse into `steps_tracked=false` and fall back to `has_output`; should use tri-state or one fetch so errors fail closed. Reported refs: `chat_service_handlers.rs:210`, `:212`, `:313`, `:1010`.
  - MEDIUM duplicate: new transition test asserts on error string; match `AppError::ExecutionBlocked(_)`. Reported ref: `transitions_agents.rs:208`.
- 17 Peirce/test strategy:
  - Test targeting risk: `task_metadata_tests.rs` belongs to separate `ralphx-domain` package, so root `src-tauri/Cargo.toml --lib` filters will not run those enum tests; use domain manifest.
  - File-stem filters like `chat_service_handlers_tests` / `metadata_tests` likely miss because sidecars compile as parent `tests` modules; `transitions_agents` is directly declared and discoverable.
  - No obvious import/visibility compile blocker found for `Path`, `GitService`, or `pub(crate) run_and_store_pre_execution_setup`.
  - LOW duplicate: `execution_failure_source_all_variants_is_transient_coverage` omits new `AgentIncomplete` despite name.
- 19 Lagrange/generated artifact hygiene:
  - LOW: dirty `plugins/app/ralphx-mcp-server/build/__tests__/cross-project-guide.test.js.map` is unrelated local generated churn, not required branch output. Evidence: only dirty file in `git status --short`, branch diff has no plugin build outputs, diff is one mappings-only JSON line. Recommendation: do not commit; restore/leave out unless plugin source changes are made and coherent generated outputs are rebuilt.
- 16 Pascal/dependency deadlock docs:
  - MEDIUM docs/repair risk: `04-p6-p5-dependency-deadlock.md` says marking P6 complete via `transition_task_corrective` “should auto-unblock” P8/merge task, but corrective transitions skip normal entry/exit actions; unblock/schedule side effect lives in `on_enter(Merged)`. Following doc literally can leave dependents blocked. Reported refs: `docs/handoffs/failing-tasks-2026-06-26/04-p6-p5-dependency-deadlock.md:177`; `src-tauri/src/application/task_transition_service.rs:2656`, `:2797`; `src-tauri/src/domain/state_machine/transition_handler/on_enter_states/outcomes.rs:322`.
  - No scheduler/state-machine production bug found behind the dependency claim; `Merged` is satisfied, `Failed` blocks, scheduler reblocks only unsatisfied blockers, task context filters resolved blockers.
- 14 Heisenberg/failed-step behavior:
  - HIGH: `execution_complete_http` removes IPR, emits `execution:completed`, persists `task:execution_completed`, publishes webhook, and returns before `handle_stream_success` later applies failed/incomplete-step gate and may transition to Failed. External observers can see completion for a run backend later fails. Reported refs: `src-tauri/src/http_server/handlers/steps.rs:732`, `:753`, `:764`; `src-tauri/src/application/chat_service/chat_service_handlers.rs:167`, `:1010`, `:1130`.
  - HIGH: Claude worker prompt gives invalid no-tests payload `test_result: { tests_ran: false }`, but MCP/Rust schema requires `tests_passed` when `test_result` is present; TS proxy drops undefined. Reported refs: `agents/ralphx-execution-worker/claude/prompt.md:182`; `plugins/app/ralphx-mcp-server/src/step-tools.ts:199`, `:218`; `src-tauri/src/http_server/types.rs:1941`; `plugins/app/ralphx-mcp-server/src/index.ts:944`; `tauri-client.ts:280`.
  - MEDIUM: backend treats `Skipped` as done, but worker/coder prompts say early-exit only if all steps are “completed,” not “completed or skipped.” Reported refs: `src-tauri/crates/ralphx-domain/src/entities/task_step.rs:20`; `chat_service_handlers.rs:177`; `chat_service_handlers_tests.rs:1186`; `agents/ralphx-execution-worker/claude/prompt.md:112`; `agents/ralphx-execution-coder/claude/prompt.md:78`.
  - Test gaps: no E2E assertion that failed-step completion does not emit/compensates external completion; no proxy/schema prompt-contract test for no-tests payload; no prompt-contract test for skipped-step guidance.
- 15 Newton/worktree setup:
  - HIGH: `worktree_setup` failures still do not block before worker spawn because `run_setup_phase` records setup failures but `run_pre_execution_setup` discards `_setup_had_failures` and computes success only from install failures; contradicts new setup contract/comment. Reported refs: `src-tauri/CLAUDE.md:118`; `src-tauri/src/domain/state_machine/transition_handler/on_enter_states/execution.rs:69`, `:143`, `:663`; `merge_validation/setup.rs:440`; `merge_validation/install.rs:388`, `:422`.
  - HIGH: pre-exec setup uses analysis `path` values and parsed symlink target parents as filesystem/process sinks without containment; absolute paths or `../` can escape the task worktree, and this matters more now because Off mode also runs setup. Reported refs: `merge_validation/install.rs:72`, `:128`; `merge_validation/setup.rs:348`, `:427`; `merge_validation/mod.rs:88`.
  - MEDIUM duplicate/confirmation: validation-cache override feeds DB-derived `task.worktree_path` into git `current_dir` without sink-local validation, affecting state if cache matches. Reported refs: `chat_service_handlers.rs:262`, `:1012`, `:2476`; `git_service/commit.rs:259`; `git_cmd.rs:104`.
  - Ordering otherwise correct: `Executing`/`ReExecuting` ensure branch/worktree, freshness, then setup before send; `ExecutionBlocked` routes to Failed.
- 13 Planck/validation-cache semantics:
  - HIGH duplicate/expanded: a green cache is only `commit_sha == current HEAD && tests_ran && tests_passed`; `RevisionNeeded -> ReExecuting` and execution entry do not clear it, and completion treats it as sufficient even when tracked steps are not done. Reported refs: `chat_service_handlers.rs:231`, `:323`, `:1010`; `task_transition_service.rs:3179`; `on_enter_states/execution.rs:707`.
  - HIGH: cache is HEAD-only and checked before execution exit auto-commit, so stale cache can validate HEAD `H`, then exit action auto-commits dirty/unvalidated work to `H2`. Reported refs: `src-tauri/src/http_server/handlers/steps.rs:670`; `chat_service_handlers.rs:1049`; `transition_handler/mod.rs:367`; `exit_actions.rs:191`.
  - MEDIUM: failed/manual/auto restart paths preserve stale `validation_cache`; no explicit removal found. Reported refs: `src-tauri/src/commands/task_commands/mutation.rs:340`; `src-tauri/src/application/reconciliation/metadata.rs:736`; `src-tauri/src/application/reconciliation/handlers/execution.rs:1209`, `:1282`.
  - Lifecycle note: cache set only by `execution_complete` with `test_result`; read by completion override, AgentExit override, and task context; only incidentally dropped by full metadata overwrites in merge outcome paths.
- 12 Kierkegaard/provider-neutrality:
  - HIGH duplicate/confirmation: stale task-level `validation_cache` can complete a later failed/no-output run because it stores only HEAD SHA/test fields and is not cleared or attempt-scoped across execution/re-execution. Reported refs: `src-tauri/src/http_server/handlers/steps.rs:670`; `src-tauri/crates/ralphx-domain/src/entities/task_metadata.rs:800`; `on_enter_states/execution.rs:199`, `:653`, `:703`; `chat_service_handlers.rs:1010`, `:1037`, `:2476`.
  - No new Claude-only defaults or provider-session regressions found in changed completion/chat paths; provider metadata remains additive.
  - Residual: branch reported behind local `origin/main` by two commits.
- 10 Huygens/concurrency:
  - CRITICAL: current-attempt guard depends on `get_status_entered_at`, but SQLite and memory repos return the earliest time the task entered the status (`ORDER BY created_at ASC LIMIT 1` / `min()`), so stale attempt A can pass the guard after attempt B re-enters the same status and transition B to review. Reported refs: `src-tauri/src/application/chat_service/chat_service_handlers.rs:935`, `:1049`, `:2476`; `src-tauri/src/infrastructure/sqlite/sqlite_task_repo/mod.rs:523`; `src-tauri/src/infrastructure/memory/memory_task_repo/mod.rs:207`; `src-tauri/src/application/task_transition_service.rs:1724`.
  - HIGH duplicate/confirmation: task-level `validation_cache` survives re-execution and can rescue a later failed attempt at the same commit. Reported refs: `chat_service_handlers.rs:231`, `:1012`, `:1037`; `task_metadata.rs:800`; `on_enter_states/execution.rs:199`.
  - MEDIUM: `execution_complete` endpoint is not idempotent/status-gated by active run; duplicate/stale calls can overwrite cache and emit duplicate completion events. Reported refs: `src-tauri/src/http_server/handlers/steps.rs:653`, `:670`, `:732`, `:753`; `src-tauri/src/http_server/types.rs:1952`.
  - No same-entry path found where Block/AutoFix setup failure spawns a worker afterward; setup runs before `send_task_execution_message`.
- 11 Kuhn/path safety:
  - HIGH/CodeQL risk: DB-backed `task.worktree_path` is used as `exec_cwd`, checked only with `exists()`, then passed to pre-exec setup that eventually runs `sh -c` with `.current_dir(cwd)` and may create symlink parent dirs. This branch expands the sink to `merge_validation_mode=Off`, so it needs sink-local validation that the path is the expected contained task worktree. Reported refs: `src-tauri/src/domain/state_machine/transition_handler/on_enter_states/execution.rs:89`, `:106`; `src-tauri/src/domain/state_machine/transition_handler/merge_validation/mod.rs:88`; `merge_validation/setup.rs:353`.
  - HIGH/CodeQL risk: DB-backed `task.worktree_path` flows to `GitService::get_head_sha`, which launches git with `.current_dir(cwd)`, without containment validation; a corrupted/stale task row could make the completion gate run git in an arbitrary repo and match a cache SHA. Reported refs: `src-tauri/src/application/chat_service/chat_service_handlers.rs:265`; `src-tauri/src/application/git_service/git_cmd.rs:107`.
  - No path-safety issues found in changed docs/prompts or dirty sourcemap.
- 09 Laplace/handoff docs:
  - HIGH docs blocker: overview still presents completion-gate work as unresolved/current even though branch already added `has_tracked_steps`, validation-cache proof, success-path override, and AgentExit override. Reported refs: `docs/handoffs/failing-tasks-2026-06-26/00-OVERVIEW.md:27`, `:44`; code refs `chat_service_handlers.rs:203`, `:231`, `:1010`, `:2470`.
  - HIGH docs blocker: worktree handoff says `merge_validation_mode == Off` skips all pre-exec setup and recommends decoupling, but current code already runs setup whenever worktree cwd exists and uses validation mode only for block/warn behavior. Reported refs: `01-worktree-environment-incompleteness.md:38`, `:72`, `:84`; code refs `on_enter_states/execution.rs:106`, `:143`; `merge_validation/install.rs:372`.
  - MEDIUM docs issue: zero-step handoff still says success path writes only `last_agent_error` and falls through to unknown/retrying recovery, but branch now writes `IncompleteSteps` / `AgentIncomplete` recovery metadata. Reported refs: `03-agent-ended-without-completing-steps.md:47`, `:121`, `:194`; code refs `chat_service_handlers.rs:1098`, `:1116`.
- 08 Hooke/Codex prompts:
  - HIGH: `agents/ralphx-execution-worker/codex/prompt.md` tells the Codex worker to use native delegation for coder sub-scopes, and the coder prompt assumes worker-owned task/worktree setup, but live `delegate_start` is still described/implemented as ideation-family coordination and delegated CWD resolves from ideation/project context rather than task execution worktree. Reported refs: `agents/ralphx-execution-worker/codex/prompt.md:17`; `agents/ralphx-execution-coder/codex/prompt.md:13`; `plugins/app/ralphx-mcp-server/src/ideation-tools.ts:577`; `src-tauri/src/http_server/handlers/coordination/mod.rs:194`; `src-tauri/src/application/chat_service/chat_service_context.rs:1549`.
  - Changed setup sentence is otherwise supported for top-level execution.
- 07 Dalton/transition tests:
  - MEDIUM: new tests call `run_and_store_pre_execution_setup` directly instead of real `on_enter(Executing/ReExecuting)`, so they do not prove setup happens before worker spawn or that Block prevents chat-service spawn. Reported refs: `src-tauri/src/domain/state_machine/transition_handler/tests/transitions_agents.rs:98`, `:150`, `:198`.
  - MEDIUM: `ReExecuting` setup coverage is missing; existing revision-loop test asserts transition/chat call only, without repo/worktree/setup fixture. Reported refs: `transitions_agents.rs:657`, `:697`.
  - LOW: Block failure assertion checks error text instead of typed `AppError::ExecutionBlocked`. Reported ref: `transitions_agents.rs:207`.
- 04 Ampere/reconciliation metadata:
  - MEDIUM: stale failed tasks with green `validation_cache` are not repaired by startup/recurring reconciliation because recovery only admits legacy timeout rows or `execution_recovery.last_state == Retrying`, and failed/max-retry rows return before any cache check. Reported refs: `src-tauri/src/application/reconciliation/handlers/execution.rs:212`, `:938`, `:1015`.
  - LOW: startup recovery collapses non-git retrying execution failures to timeout metadata, bypassing new `AgentIncomplete -> IncompleteSteps` mapping except where source survives into the mapper. Reported refs: `src-tauri/src/application/reconciliation/metadata.rs:925`; `handlers/execution.rs:254`, `:388`.
  - LOW: `execution_setup_warning` is not cleared after later successful setup; success path overwrites log but leaves stale warning flag. Reported refs: `src-tauri/src/domain/state_machine/transition_handler/on_enter_states/execution.rs:172`, `:117`.
  - Missing coverage: startup/reconciliation with `AgentIncomplete`, stale failed-task repair using green cache, composite metadata preservation, clearing stale setup warnings.
- 01 Russell/execution entry:
  - HIGH duplicate/confirmation: stale `validation_cache` can complete a later re-execution attempt at unchanged HEAD after review requested changes because re-entry resets steps but does not clear/re-scope the cache. Reported refs: `chat_service_handlers.rs:231`, `:1010`, `:2479`; `on_enter_states/execution.rs:199`.
  - MEDIUM duplicate/confirmation: step repo query errors fail open when `has_output=true`, because errors become indistinguishable from zero-step tasks. Reported refs: `chat_service_handlers.rs:203`, `:305`, `:1010`; test ref `chat_service_handlers_tests.rs:1063`.
  - No concrete issue found in `on_enter_states/execution.rs` setup ordering; setup runs before spawn in both `Executing` and `ReExecuting`.
- 06 Locke/chat-service tests:
  - HIGH test/production risk: AgentExit validation-cache rescue test asserts only final status, while production appears to write `last_agent_error`, `failure_error`, and `execution_recovery` before computing the override; a task could end in `pending_review` while still carrying failure/retry metadata. Reported refs: `src-tauri/src/application/chat_service/chat_service_handlers_tests.rs:3129`; production refs `chat_service_handlers.rs:2285`, `:2476`.
  - MEDIUM: several handler tests use hard-coded run ids and direct `task.internal_status = Executing` without seeding matching `AgentRun` or status history, so they do not prove current-attempt behavior. Reported refs: `chat_service_handlers_tests.rs:1385`, `:1468`, `:1543`, `:3171`.
  - MEDIUM: zero-step output transition test only asserts the task left executing; should assert `PendingReview | Reviewing` and absence of incomplete-step failure metadata. Reported ref: `chat_service_handlers_tests.rs:1424`.
  - LOW: git-backed test helper is brittle against ambient git config/signing. Reported ref: `chat_service_handlers_tests.rs:516`.
  - LOW: SHA mismatch coverage exists only for pure predicate; no git-backed wrapper/finalizer stale-SHA test. Reported ref: `chat_service_handlers_tests.rs:574`.
- 02 Epicurus/chat-service handling:
  - HIGH: stale `validation_cache` can falsely complete a later execution attempt at the same `commit_sha` because `validated_completion_override()` checks only `commit_sha`, `tests_ran`, and `tests_passed`, while `ValidationCacheMetadata` has no attempt/run/status-entry marker and reconciliation cleanup preserves the cache. Reported refs: `src-tauri/src/application/chat_service/chat_service_handlers.rs:231`, `:249`, `:1012`, `:2479`; `src-tauri/crates/ralphx-domain/src/entities/task_metadata.rs:800`; `src-tauri/src/application/reconciliation/metadata.rs:736`; `src-tauri/src/http_server/handlers/steps.rs:676`.
  - MEDIUM/HIGH duplicate of 05: task-step repo errors are treated as “no tracked steps,” allowing output-only completion. Reported refs: `chat_service_handlers.rs:203`, `:323`, `:1011`.
- 05 Mendel/transition discipline:
  - HIGH: `has_tracked_steps()` returns `false` on step-repository query errors, and the caller treats `steps_tracked=false` as permission to fall back to `has_output`, so a transient step repo/DB failure can advance `executing`/`re_executing` to `pending_review` without verifying step completion. Reported refs: `src-tauri/src/application/chat_service/chat_service_handlers.rs:202`, `:212`, `:305`, `:1010`, `:1049`.
  - No new direct production `internal_status` mutation found in touched runtime files.
- 03 Zeno/domain metadata:
  - No blocking issue found in new `ExecutionFailureSource::AgentIncomplete` and `ExecutionFailureReasonCode::IncompleteSteps` enum additions.
  - Coverage gap: add/consider persisted JSON parse coverage for `execution_recovery.events[]` containing `failure_source:"agent_incomplete"` and `reason_code:"incomplete_steps"`.
  - Coverage/naming gap: `all_failure_source_variants_have_expected_transient_behavior` omits `AgentIncomplete`; either include it or narrow the test claim.

## Validation Notes
- 17 Peirce recommended targeted commands:
  - `cargo test --manifest-path src-tauri/crates/ralphx-domain/Cargo.toml 'entities::task_metadata::tests::execution_recovery_reason_code_serialization' --lib`
  - `cargo test --manifest-path src-tauri/crates/ralphx-domain/Cargo.toml 'entities::task_metadata::tests::execution_failure_source' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::reconciliation::metadata::tests::failure_source_to_reason_code_maps_agent_incomplete' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'domain::state_machine::transition_handler::tests::transitions_agents::pre_execution_setup_' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::test_execution_completion_action' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::test_validation_cache_' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::test_validated_completion_override' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::test_has_tracked_steps' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::test_success_finalizer_' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::test_zero_step_run_with_output_transitions_out_of_executing' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::test_incomplete_execution_success_finalizer_fails_current_attempt_with_metadata' --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::test_task_execution_agent_exit_uses_head_matched_validation_cache_for_failed_steps' --lib`
- 04 Ampere reported passing:
  - `cargo test --manifest-path src-tauri/Cargo.toml failure_source_to_reason_code_maps_agent_incomplete --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml pre_execution_setup_ --lib`
  - `cargo test --manifest-path src-tauri/Cargo.toml validation_cache --lib`
- 03 Zeno reported `cargo test --manifest-path src-tauri/Cargo.toml -p ralphx-domain task_metadata --lib` passed, 101 tests.

## Debate Notes
- Pending broader cross-agent synthesis.

## Final Convergence Round

### Independent pivot-skeptic review — verdict: pivot_sound_zero_blocking
- The episode-freshness gate (`captured_at >= get_status_last_entered_at`) survived independent adversarial attack. No reintroduced false-negative (recovery re-drive via `execute_entry_actions` appends no history row; self-transition guard makes a second same-status row structurally impossible), no cross-attempt false-positive (genuine re-execution always appends a new entry at `T_new`; a prior cache is captured before the prior episode's exit which strictly precedes `T_new`), and no compile/contradiction blocker.
- Two LOW (non-blocking) spec-accuracy gaps found and resolved surgically:
  - Gap A: `created_at` is SUB-second in production (`persist_status_change` → `Utc::now().to_rfc3339()`), not second-granular; corrected H1 rationale + §9 residual, downgraded the `rowid DESC` tiebreaker to defensive-only.
  - Gap B: `is_skip_test_validation` does not exist; real symbols are `compute_validation_cache` (helpers.rs:1606) + `compute_validation_hint` (:1562), on the SHARED get_task_context hint path (worker/reviewer/merger). Replaced throughout the spec.

### F5b path-containment grounding (its original verifier crashed; re-run) — stillReal: true, MEDIUM, prior treatment NOT adequate
- Confirmed real unvalidated path sinks across FOUR families fed by DB/analysis-JSON paths: `execution.rs` exec_cwd (exists()-only), shared `merge_validation/mod.rs:91` spawn cwd, `setup.rs` (cmd_cwd + symlink target/parent → create_dir_all :353 / read_link :128 / remove_file :136/:166 / spawn :427), `install.rs` (read_link :16 / remove_file :40 / spawn :128), and `git_cmd.rs:107` via `get_head_sha` from `validated_completion_override` (no check at all).
- Chosen helper: REUSE existing `crate::utils::path_safety` (`validate_absolute_non_root_path` + `checked_*`), add ONE shared `ensure_path_within(root, candidate)`; do NOT invent `ensure_worktree_path_contained`.
- Corrections applied to spec H7 + WI-5: full five-sink inventory (incl. per-entry `resolve(&entry.path)` joins, parsed symlink target/parent, and the omitted git_cmd.rs sink), CodeQL suppression moved to the actual sinks (not the call site), containment made UNCONDITIONAL across all merge_validation_modes, F5b reframed as hardening of pre-existing sinks (not a branch regression), and per-sink + path_safety unit test obligations added.

### Final convergence status
- Header set to "CONVERGED — merge-blocking spine survived an independent adversarial round with zero blocking gaps; F5b path-containment (non-blocking, WI-5) spec gap corrected this round. See §10."
- The single residual gap entering the round (F5b inadequate sink coverage) was a NON-merge-blocking spec-completeness gap in WI-5, now resolved at the SPEC level by the H7/WI-5 corrections. Pivot survived independent attack; both LOW gaps corrected. No CRITICAL/HIGH/MEDIUM gap remains unaddressed in the spec text. Spec is CONVERGED for implementation handoff; F5b rides WI-5 (low-risk hardening, not in the merge-blocking subset).
