<!--
Branch: fix/execution-completion-gate-validation-cache
Generated as: debate-driven handoff
Convergence status: CONVERGED — merge-blocking spine (episode-freshness pivot) survived an independent adversarial round with zero blocking gaps; F5b path-containment (non-blocking, WI-5) spec gap corrected this round. See §10.
-->

# Implementation Spec Handoff — Execution Completion Gate Hardening (Final Revision)

Branch: `fix/execution-completion-gate-validation-cache` (merge-base `342a35d9f`, post fix commits `986369212`, `32439e8c1`, `fa5997ae2`, `98490627b`)
Spine architecture: **Run-Identity-First Completion Seam** = run-aware attempt-authority resolver + **episode-scoped (temporal) cache-evidence gate** + tri-state fail-closed step gate + positive-identity-gated cache rescue. All claims below were re-verified on disk against current HEAD, including the four round-2 blocking gaps.

---

## 0. Round-2 Resolution Summary (what changed from the prior revision and why)

The prior revision mandated (a) run/chain **stamping** of `ValidationCacheMetadata` (struct migration + ~6 test migrations) AND (b) an **unconditional on_enter(`Executing`/`ReExecuting`) `validation_cache` clear** as the "primary" reviewer-leak closure. Round-2 adversarial review proved BOTH unsafe/over-engineered, and I confirmed every claim on disk:

| Round-2 gap | Verified on disk | Verdict |
|---|---|---|
| Unconditional on_enter clear reintroduces the false-negative under recovery re-drive | `reconciliation/handlers/execution.rs:2274` calls `execute_entry_actions(&task.id, task, status)` with the task's CURRENT `Executing`/`ReExecuting` status after tagging `trigger_origin="recovery"` (`:2264`); `startup_jobs.rs:962/1080/1181` do likewise for AGENT_ACTIVE statuses. So `on_enter(Executing)` DOES fire on a still-Executing task → an unconditional clear wipes a same-attempt green cache. | **CONFIRMED CRITICAL** |
| Stamping + clear is contradictory / over-engineered | Reviewer reader `compute_validation_cache` (`http_server/helpers.rs:1606`) is HEAD/`commit_sha`-only and never consumes any run/chain stamp; stamping does nothing for it, so the spec leaned on the (unsafe) clear. | **CONFIRMED HIGH** |
| H1 trait path wrong + impl undercount | Trait lives at `crates/ralphx-domain/src/repositories/task_repository.rs:175` (the spec's `src/domain/repositories/...` path does not exist). `get_status_entered_at` has **9 impls** (2 production + 7 test doubles), not 3. | **CONFIRMED HIGH** |
| H6 "necessarily a PRIOR attempt's cache" invariant false under shutdown-resume | `handle_stream_success` shutdown guard (`chat_service_handlers.rs:925-933`) returns without finalizing, leaving the task `Executing` with its OWN green cache; `persist_shutdown_interrupted_metadata` (`:851`) sets only `shutdown_interrupted`, NOT `preserve_steps`, so the `preserve_steps` early-return (`on_enter_states/execution.rs:205`) does not protect the cache. | **CONFIRMED HIGH** |

**Unifying fix (resolves all four gaps with strictly LESS surface):** replace stamping + on_enter clear with a single **episode-freshness gate** consumed by BOTH readers:

> A green, HEAD-matched cache is trusted by a completion/reviewer reader **iff** `cache.captured_at >= get_status_last_entered_at(task, <current execution episode>)`.

Why this is correct and minimal:
- `ValidationCacheMetadata.captured_at: DateTime<Utc>` already exists (`task_metadata.rs:813`) — **no struct migration, no serde change, no run_id/chain test-arity migration.**
- `execute_entry_actions` (`task_transition_service.rs:2925`) runs entry actions **without a transition**, so it appends **no** status-history row. Therefore a shutdown-resume recovery re-drive leaves the latest `Executing` entry timestamp UNCHANGED, and the same-attempt cache (`captured_at >= that entry`) stays **trusted** → **no reintroduced false-negative** (gaps #1, #4).
- A genuine fresh re-execution DOES append a new `Executing`/`ReExecuting` history row at `T_new`; a prior attempt's cache (`captured_at < T_new`) is **rejected** → cross-attempt leak closed for BOTH readers (gap #2's reviewer reader included — temporal check is reader-agnostic, unlike a run stamp).
- **One** mechanism, built on `get_status_last_entered_at` (H1) which we add anyway for F1. **No** `on_enter` cache clear at all. **No** dependency on run-chain continuity across recovery re-drive.
- **H3 (run-resolution for the write) and H4-struct (agent_run_id/run_chain_id fields) are DELETED from the plan.**

Residual: a narrow same-wall-clock-second cross-attempt false-positive (prior cache captured in the exact second a new episode is entered). This errs toward the false-positive class the branch already accepts in narrow forms, NOT the false-negative the branch exists to fix. Documented as LOW residual risk (§9) with an optional `captured_by`/sequence tightening follow-up.

---

## 1. Executive Summary + Merge Verdict

The branch correctly fixes the original FALSE-NEGATIVE (a completed worker with green HEAD-matched validation evidence was marked `failed` because a lingering terminal `failed` step trapped it). It introduced `has_tracked_steps`, `validated_completion_override` / `validation_cache_proves_completion`, the `IncompleteSteps`/`AgentIncomplete` classification, and setup-before-spawn ordering.

In doing so it opened several FALSE-POSITIVE completion paths. The confirmed-real, merge-blocking set:

- **F2 (HIGH, blocks merge):** the validation cache is task-level and HEAD-only — no episode/attempt scoping and never cleared on re-entry. A green cache written by attempt A rescues a later attempt B at an unchanged HEAD (B did nothing / left work uncommitted / crashed before its own `execution_complete`). TWO un-scoped readers (the completion finalizers AND the reviewer test-skip path). **Fix:** episode-freshness gate (`captured_at >= get_status_last_entered_at`) on BOTH readers — NOT run-stamping (over-engineered, doesn't reach the reviewer reader) and NOT an on_enter clear (reintroduces the false-negative under recovery).
- **F1b (HIGH, blocks merge):** `validated_completion_override` is currently repo-free (reads `task.metadata` + `GitService::get_head_sha` only, `chat_service_handlers.rs:249-277`), so under a proposed `IdentityUnknown ⇒ proceed-as-current` flow a stale finalizer during a transient run-read DB error could self-rescue via a HEAD-matched cache, bypassing the fail-closed step gate. **Fix:** the cache rescue is consulted ONLY when `resolve_current_execution_attempt` returns `Current(task)`; under `IdentityUnknown` the override is NOT called. With episode-freshness, the override additionally needs the episode boundary (a repo-derived timestamp) — so during a DB outage the boundary read also fails → not trusted → fail closed. F1b is closed twice over.
- **F3 (MEDIUM, blocks merge):** `has_tracked_steps()` swallows a DB `Err` to `false`, routing a transient DB error on a task with genuinely incomplete tracked steps + output to `PendingReview` via `else has_output` (a NEW fail-OPEN regression vs. merge-base, which failed CLOSED). **Fix:** tri-state fail-closed gate.
- **F4 (MEDIUM, blocks merge, promoted):** zero-step run advances on `has_output` alone, and `completion_tool_called` is dropped at the `StreamOutcome` boundary. Plumbing touches all THREE `handle_stream_success` call sites; landing it half-way introduces a NEW Cancelled-zero-step false-negative. Must land with F2/F3.

The remaining still-real findings (F5a, F7, F8, F9, F10) are correctness/hardening/test/doc items that are NOT merge blockers but should ride the same spine. F6 is a verified-real LOW external-event-ordering issue (`steps.rs:764-792` DOES emit an external `task:execution_completed` `outcome=completed` event + webhook at call time, before the gate), tracked as a deferred follow-up.

### Merge verdict: NOT MERGE-READY

**Becomes merge-ready when the merge-blocking subset lands together (one coordinated change set):**
- H1 (`get_status_last_entered_at`, correct trait path + all 9 impls) + H2 (`resolve_current_execution_attempt`, also returns the episode-entry timestamp) + H4 (`validation_cache_fresh_for_episode` episode-freshness predicate, NO struct migration) + H5 (`fetch_step_completion_state` tri-state + `StreamOutcome.completion_tool_called`).
- F2 (episode-freshness gate threaded into BOTH the completion read sites AND the reviewer skip reader).
- F1b (cache rescue gated behind `Current(task)`; never fires under `IdentityUnknown`).
- F3 (tri-state fail-closed gate rewrite).
- F4 (`completion_tool_called` threaded through `StreamOutcome` AND into ALL THREE `handle_stream_success` call sites + required for zero-step).
- All success-block edits (`chat_service_handlers.rs:1010-1074`) + the AgentExit override (`:2476-2496`) + the two internal `handle_stream_error→handle_stream_success` calls (`:1605`/`:1685`) integrated as ONE change set.
- Existing rescue/override tests updated for the episode-freshness gate (set the mock's latest-entry BEFORE the cache `captured_at`); shutdown-resume regression test added; zero clippy warnings; zero test failures; all named tests below passing.

**Explicitly NOT in the plan anymore:** run/chain stamping of `ValidationCacheMetadata` (H3 + the `agent_run_id`/`run_chain_id` struct fields) and the unconditional on_enter `validation_cache` clear. Both were proven to reintroduce the false-negative or be redundant.

F1 (the latest-entry guard portion), F5a/F5b, F7, F8, F9, F10 may follow in subsequent commits/PRs on the same branch but must not regress the spine.

---

## 2. Findings Status Table

| F-id | Still real? | Severity | Blocks merge | One-line |
|------|-------------|----------|--------------|----------|
| F1-stale-attempt-guard | YES (pre-existing on main) | HIGH | No | Both finalizer guards compare run.started_at against the EARLIEST status entry; a stale first-attempt run passes the guard on any re-enterable status. Also fails OPEN on DB `Err`. |
| F1b-identity-unknown-cache-bypass | YES (design risk) | HIGH | **Yes** | `validated_completion_override` is repo-free (`chat_service_handlers.rs:249-277`); a stale finalizer whose run-read errors could self-rescue via a HEAD-matched cache. Gate the rescue behind `Current(task)`; under `IdentityUnknown` do not consult it. Episode-freshness adds a second close (boundary read also fails under outage). |
| F2-validation-cache-scoping | YES | HIGH | **Yes** | Cache has no episode/attempt scoping, never cleared on re-entry; a prior attempt's green cache rescues a different attempt at unchanged HEAD. TWO readers (completion finalizers + reviewer skip path). **Fix = episode-freshness gate (`captured_at >= latest exec entry`) on both readers.** |
| F3-step-repo-fail-open | YES (branch regression) | MEDIUM | **Yes** | `has_tracked_steps()` returns `false` on DB `Err`, collapsing `Ok(empty)` and `Err`; transient DB error advances incomplete-step work to `PendingReview` via `has_output`. |
| F4-zero-step-no-execution-complete | YES (branch broadened) | MEDIUM | **Yes** (promoted) | Zero-step run advances on `has_output` alone; `CompletionSignalTracker.was_called()` dropped at the `StreamOutcome` boundary. Plumbing touches all 3 `handle_stream_success` call sites; lands with F2/F3. |
| F5a-setup-failure-not-blocking | YES | MEDIUM | No | `run_pre_execution_setup` discards `_setup_had_failures`; `PreExecSetupResult.success` reflects only install. Contract/impl mismatch. |
| F6-emit-before-gate | YES (verified) | LOW | No | `execution_complete_http` emits external `outcome=completed` + webhook before the completion gate runs (`steps.rs:764-792`). |
| F7-prompt-tool-contract-mismatch | Sub-claim 1 YES / Sub-claim 2 NOT-A-BUG | MEDIUM | No | Worker prompt's no-tests case sends `test_result: { tests_ran: false }` → Rust deser 422, skipping clean-exit. Codex delegation sub-claim is not a defect. |
| F8-docs-stale | YES | LOW | No | Handoff docs frame already-shipped completion-gate fixes as TODO; doc 04 wrongly claims `transition_task_corrective→Merged` auto-unblocks dependents. |
| F9-test-quality-gaps | YES | MEDIUM | No | Setup-before-spawn covered only by helper-level tests bypassing `on_enter`; fragile `err.to_string().contains()` Block assertion; missing negative-metadata assertions on rescued tasks. |
| F10-reconciliation-stale-failed-repair | YES | LOW | No | No path clears `validation_cache` on restart/auto-retry/reset; startup recovery hardcodes `(TransientTimeout, Timeout)` dropping `AgentIncomplete→IncompleteSteps`. (Episode-freshness already prevents cross-attempt rescue; these clears are hygiene on no-in-flight-run paths.) |

---

## 3. Shared Safety Helpers (defined ONCE)

> Audit-all-paths rule: every completion/finalizer/review path that can reach `PendingReview`/`Failed` OR skip validation MUST route through these helpers. The destinations are: **success finalizer** (`handle_stream_success`, `chat_service_handlers.rs:1010-1074`), **error/AgentExit finalizer** (`handle_stream_error`, `:2476-2496`), the **two internal Cancelled-as-success calls** (`:1605` and `:1685`), and the **shared get_task_context hint reader** (`compute_validation_cache`, async, `http_server/helpers.rs:1606`, called once at `:1424`; emits via the pure `compute_validation_hint` at `:1562` which produces the `skip_tests`/`skip_test_validation`/`run_tests` strings). **Symbol correction (final round):** there is NO `is_skip_test_validation` function — the real surface is `compute_validation_cache` + `compute_validation_hint`, and it is the **shared `get_task_context` hint path consumed by worker/reviewer/merger**, not a reviewer-exclusive reader (threading the episode boundary there is benign for all three — a prior-episode cache correctly shows `run_tests`). Do not fix one and miss another.

### H1 — `TaskRepository::get_status_last_entered_at`
New trait method mirroring the existing `get_status_entered_at`, returning the **LATEST** entry instead of the earliest.

- **Trait (CORRECTED PATH):** `src-tauri/crates/ralphx-domain/src/repositories/task_repository.rs` (the existing `get_status_entered_at` is at line 175 here; the prior spec's `src-tauri/src/domain/repositories/...` path does NOT exist) — add
  `async fn get_status_last_entered_at(&self, task_id: &TaskId, status: InternalStatus) -> AppResult<Option<DateTime<Utc>>>;`
  No default body (no safe default exists for the production guard) → **every** impl must be added or the workspace won't compile.
- SQLite impl: `src-tauri/src/infrastructure/sqlite/sqlite_task_repo/mod.rs:515` (mirror `get_status_entered_at`). Query MUST be `... ORDER BY created_at DESC, rowid DESC LIMIT 1`. **Granularity correction (final round):** production rows are SUB-SECOND, not second-granular — the canonical live writer `persist_status_change` (`sqlite_task_repo/helpers.rs:31-40`) inserts `created_at = Utc::now().to_rfc3339()` (nanosecond precision); the second-granular `strftime(...)` at `v1_initial_schema.rs:193` is only the column DEFAULT, used when an INSERT omits `created_at`, which production never does. The `rowid DESC` tiebreaker is therefore **defensive-only** (keeps row ordering deterministic in the practically-impossible exact-nanosecond-tie case), NOT load-bearing for the second-granular reason the prior revision claimed. Keep the comparison on parsed `DateTime<Utc>` (do NOT weaken to a string compare).
- Memory impl: `src-tauri/src/infrastructure/memory/memory_task_repo/mod.rs:207` → `matching_timestamps.into_iter().max()`.
- **ALL test doubles MUST implement the method (verified by `grep "fn get_status_entered_at"` — 9 impls total: 2 production above + 7 doubles):**
  - `src-tauri/crates/ralphx-domain/src/repositories/task_repository_tests.rs:63`
  - `src-tauri/tests/apply_service.rs:915`
  - `src-tauri/tests/chat_service_context.rs:1029`
  - `src-tauri/tests/review_service.rs:252`
  - `src-tauri/src/application/task_context_service_tests.rs:103`
  - `src-tauri/src/application/chat_service/chat_service_handlers_tests.rs:69`
  - `src-tauri/src/infrastructure/agents/spawner_tests.rs:119`
  - Before coding, re-run `grep -rn "impl TaskRepository for" src-tauri/ --include="*.rs"` to confirm the full set (the prior spec undercounted at 3).

### H2 — `resolve_current_execution_attempt` (attempt-authority resolver + episode boundary)
Single resolver replacing BOTH `task_execution_attempt_matches_current_status` (`chat_service_handlers.rs:1419-1457`) and `load_current_task_execution_attempt` (`:1459-1500`).

Signature (new private fn in `chat_service_handlers.rs`):
```rust
enum AttemptResolution {
    Current { task: Task, episode_entered_at: DateTime<Utc> },
    Stale,
    IdentityUnknown,
}

async fn resolve_current_execution_attempt(
    task_id: &TaskId,
    completing_run_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
) -> AttemptResolution
```
Rules:
- Fetch task. If task missing → `Stale`. If `internal_status` ∉ {`Executing`,`ReExecuting`} → `Stale` (task moved on; preserves the existing correct behavior at `1434`/`1473`).
- `IdentityUnknown` only on a DB **error** reading task, run, or the latest-entry timestamp (`Err(_)` arms at `1428`/`1467`/`1453`/`1491` + the new H1 read). `IdentityUnknown` is treated downstream as **proceed-as-current FOR THE STEP-GATED PATH ONLY** (do NOT strand the task in Executing) — but the cache rescue (`validated_completion_override`) MUST NOT be consulted under `IdentityUnknown` (F1b). This is the deliberate no-strand FLIP of today's fail-OPEN-to-`true` arms, narrowed so it cannot self-rescue via cache.
- ACCEPT as `Current { task, episode_entered_at }` iff status ∈ {Executing,ReExecuting} AND `agent_run.started_at + tolerance >= H1(latest entry of that status)`. `episode_entered_at` = that H1 latest-entry timestamp (also handed to the cache rescue as the episode boundary — see H4).
- Run-chain layer is a **positive-evidence REJECT only**: if the completing run is provably in an OLDER `run_chain_id` than `agent_run_repo.get_active_for_conversation(...)`'s run → `Stale`. If chain resolution is ambiguous/errors → do NOT reject (avoids the queue-continuation false-negative).
- `tolerance` (spawn-before-history slack) MUST come from `runtime_config` (`config/ralphx.yaml`), NOT a hardcoded Rust `const` (src-tauri/CLAUDE.md "No Inline Timeout Consts"). Replaces the inline `chrono::Duration::seconds(1)` at `1456`/`1495`. Keep it small (~1s); it tolerates a run spawning slightly before its status-history row persists.

### H3 — REMOVED
The prior `resolve_active_execution_run_id` write-side run resolver is **deleted from the plan**. No stamping is written, so no write-side run/chain identity source is needed. The episode boundary used by the read predicate comes from H1/H2 (read-side), not from a write-side stamp.

### H4 — Episode-freshness cache predicate (NO struct migration)
`ValidationCacheMetadata` is **unchanged** — it already carries `captured_at: DateTime<Utc>` (`task_metadata.rs:813`, required field, present on every existing serialized cache, no migration). Add ONE predicate in `chat_service_handlers.rs` (and reuse it from the reviewer reader):

```rust
/// Trust a green, HEAD-matched cache only if it was captured during the CURRENT
/// execution episode (i.e., at/after the latest entry into Executing/ReExecuting).
/// Recovery-safe: a shutdown-resume re-drive runs on_enter WITHOUT a transition,
/// so it appends no status-history row → episode_entered_at is unchanged → a
/// same-attempt cache stays trusted. A fresh re-execution appends a new entry →
/// a prior-attempt cache (captured before it) is rejected.
pub(crate) fn validation_cache_fresh_for_episode(
    cache: &ValidationCacheMetadata,
    current_head_sha: &str,
    episode_entered_at: DateTime<Utc>,
) -> bool {
    cache.commit_sha == current_head_sha
        && cache.tests_ran
        && cache.tests_passed
        && cache.captured_at >= episode_entered_at
}
```
- Keep the existing SHA-only `validation_cache_proves_completion(cache, head_sha)` (`chat_service_handlers.rs:231-236`) as a private building block, but **no caller may use it without the episode check**; the public completion path uses `validation_cache_fresh_for_episode`.
- `chat_service_handlers.rs:249-277` — change `validated_completion_override(&task)` to `validated_completion_override(&task, episode_entered_at: DateTime<Utc>)`, resolve HEAD as today, and gate trust on `validation_cache_fresh_for_episode`. **Critical (F1b):** the override is consulted ONLY when `resolve_current_execution_attempt` returned `Current { task, episode_entered_at }`. Under `IdentityUnknown` the override is NOT called (treated as `false`); only the step-gated path runs, which fails closed during the same DB outage.

### H5 — Tri-state step gate + `completion_tool_called`
- New enum + helper in `chat_service_handlers.rs` (replacing the separate `all_steps_completed`+`has_tracked_steps` calls at `1010-1011` **ONLY at the success-path call site**):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  enum StepCompletionState { NoSteps, AllComplete, Incomplete, Unknown }

  async fn fetch_step_completion_state(
      task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
      task_id: &TaskId,
  ) -> StepCompletionState   // ONE get_by_task; Unknown on None or Err
  ```
  Mapping from `repo.get_by_task`: `Ok(empty)`→`NoSteps`; `Ok(all Completed/Skipped)`→`AllComplete`; `Ok(some incomplete)`→`Incomplete`; `None`/`Err`→`Unknown`.
  **RETENTION:** `all_steps_completed` MUST be RETAINED — the AgentExit override at `chat_service_handlers.rs:2479` still calls it and is explicitly left unchanged. `has_tracked_steps` may be removed only if it has no remaining callers after the success-path rewrite; grep before deleting. Only the success-path calls at `:1010-1011` are replaced by `fetch_step_completion_state`.
- `StreamOutcome` (`chat_service_streaming.rs:911-932`) — add `pub completion_tool_called: bool;`. Set from `completion_signal_tracker.was_called()` at BOTH construction sites: Claude `~:2945`, Codex `~:3588`. Codex MUST be set or zero-step Codex completions silently regress to `Failed`.
- Propagate `completion_tool_called` alongside `effective_has_output` from `chat_service_send_background.rs` (`~:1304`/`:1371`) into `handle_stream_success` and into the rewritten gate (see WI-3 for the complete call-site list).

### H6 — `clear_validated_completion_cache` (F10 lifecycle ONLY — NOT on_enter)
Metadata clear via the established `MetadataUpdate::new().with_null("validation_cache").merge_into(...)` pattern (same path as `restart_note`/`preserve_steps`). Metadata-only — no `internal_status` mutation.

**NON-NEGOTIABLE scope change from the prior revision:** this helper is wired ONLY into definitively-no-in-flight-run lifecycle points, where clearing is unambiguously safe:
- `move_task` terminal→Ready (`commands/task_commands/mutation.rs:340-415`).
- `reconcile_failed_execution_task` auto-retry (`reconciliation/handlers/execution.rs ~:1224-1232`).
- `reset_execution_recovery_metadata` (`metadata.rs:760-791`).

**It is NOT wired into `on_enter(Executing)`/`on_enter(ReExecuting)`.** The previously-mandated unconditional on_enter clear is DELETED: it fires during recovery re-drive (`execute_entry_actions(Executing)` at `reconciliation/handlers/execution.rs:2274` and `startup_jobs.rs:962/1080/1181`) on a still-Executing task that holds its OWN green cache (shutdown-interrupted resume, `handle_stream_success` guard `:925-933`; `persist_shutdown_interrupted_metadata` `:851` does NOT set `preserve_steps`), wiping same-attempt evidence and reintroducing the exact false-negative this branch removes. The cross-attempt leak (both readers) is instead closed by the episode-freshness gate (H4), which is non-destructive and recovery-safe.

### H7 — `PreExecSetupResult` extension + path containment via existing `path_safety` (F5)
- `merge_validation/mod.rs:331` — extend `PreExecSetupResult { success, log }` to `{ success, setup_had_failures: bool, hard_setup_failure: bool, log }`.
- **Path containment — REUSE the existing app-owned helper; do NOT invent `ensure_worktree_path_contained`.** The app already has `crate::utils::path_safety` (`src-tauri/src/utils/path_safety.rs`), which exposes `validate_absolute_non_root_path(path, context)` (rejects relative, `ParentDir`, `CurDir`, `Prefix`/`RootDir`-only via `normal_components == 0`) plus a `checked_*` family (`checked_exists`, `checked_is_symlink`, `checked_read_to_string`, `checked_remove_file`, `checked_remove_dir_all`, `checked_read_dir`). This is the SAME helper already used throughout `git_service/worktree.rs` (e.g. `:31`, `:154`, `:197`, `:296-303`).
  - `validate_absolute_non_root_path` is a LEXICAL single-path guard (absolute + no `..`); it does NOT canonicalize-a-trusted-root-and-assert-candidate-under-it. There is currently NO `ensure_path_within`/`validate_path_within` helper. So add **ONE** shared `ensure_path_within(trusted_root, candidate, context) -> AppResult<PathBuf>` to `path_safety.rs` that (a) runs `validate_absolute_non_root_path` on `candidate`, (b) canonicalizes the trusted root, (c) asserts the candidate (or its canonical form) is prefixed by the canonical root — rejecting (not sanitizing) otherwise. Reuse `validate_absolute_non_root_path` as the lexical core. Use it at every analysis-derived join sink below; use the `checked_*` helpers for `read_link`/`remove_file`.
- **Containment is UNCONDITIONAL across `merge_validation_mode`.** Setup/spawn runs in Block/AutoFix/Warn/Off alike (mode only gates whether a setup/install FAILURE blocks vs. warns); the path-sink exposure is identical in Off. Containment MUST NOT be mode-gated (unlike the F5a blocking decision, which is mode-gated). Note: the branch does NOT widen any of these sinks to Off — `merge_validation/*` and `git_service/*` are unchanged vs. main, and the Warn/Off proceed-with-warning at `execution.rs:157-191` is pre-existing. F5b is hardening of pre-existing unvalidated sinks, NOT a branch regression.
- **Full sink inventory (all four families launch processes / touch the FS with DB- or analysis-derived paths and currently have NO containment):**
  1. `execution.rs run_and_store_pre_execution_setup` — `exec_cwd` built from `task.worktree_path` (`~:89-90`), only `exists()`-checked at `~:100`, handed to `run_pre_execution_setup` (`~:106`) → shared spawn sink.
  2. `merge_validation/mod.rs:91 spawn_cancellable_command(... ).current_dir(cwd)` — the shared process-launch sink for pre-exec, setup, install, and validate. `sh` already resolves via `tool_paths` (`resolve_shell_cli_path`); only the cwd is unvalidated.
  3. `merge_validation/setup.rs run_setup_phase` — `cmd_cwd = merge_cwd.join(resolve(&entry.path))` (`~:281-285`) where `entry.path` comes from `project.custom_analysis`/`detected_analysis` JSON (DB/agent-derived); `parse_symlink_command` yields `target`/`parent`. Sinks: `create_dir_all(parent)` `:353`; `read_link(&target_path)` `:128`; `remove_file(&target_path)` `:136`/`:166`; spawn `cmd_cwd` `:427`.
  4. `merge_validation/install.rs run_install_phase` — `cmd_cwd = exec_cwd.join(resolve(&entry.path))` (`:74-78`); `nm_path = cmd_cwd.join("node_modules")` (`:81`). Sinks: `read_link(nm_path)` `:16`; `remove_file(nm_path)` `:40`; spawn `cmd_cwd` `:128`.
  5. `git_cmd.rs build_git_command .current_dir(cwd)` (`:107`) reached via `GitService::get_head_sha` (`commit.rs:260`); `validated_completion_override` (`chat_service_handlers.rs:262-265`) passes `Path::new(task.worktree_path)` straight in with NO check (not even `exists()`).
- **CodeQL suppression placement:** put `// codeql[rust/path-injection]` on the blank line immediately ABOVE each REAL sink AFTER validation — `mod.rs:91`, `setup.rs:353`/`:427`, `install.rs:40`/`:128`, `git_cmd.rs:107` — NOT at the `execution.rs` call site. Because `mod.rs:91` is shared by validated (pre-exec, now-validated `cmd_cwd`) AND merge-validation paths, the validation MUST be applied to `cmd_cwd` at the `setup.rs`/`install.rs` construction sites; suppressing at the shared spawn sink without validating the construction sites would mask still-tainted merge-validation cwds.

---

## 4. Per-Finding Work Items

> All transitions stay on `TaskTransitionService::transition_task*` / `update_metadata` (DbConnection). No direct `internal_status` writes. Run targeted tests per `.claude/rules/rust-test-execution.md`.

### WI-1 — F2 episode-scoped cache evidence + F1b identity-gated rescue (BLOCKS MERGE) — uses H1, H2, H4

**ROOT CAUSE (F2):** The rescue path proves a property of the ARTIFACT ("code at HEAD has a green test run on record") and treats it as proof of the ATTEMPT. `ValidationCacheMetadata` is HEAD-only and never cleared on re-entry, so for any later attempt at unchanged HEAD the prior attempt's green cache satisfies the HEAD-only predicate and overrides `Failed→PendingReview`. The override at `1012` also runs BEFORE `on_exit auto_commit_on_execution_done()` commits dirty work, so HEAD still equals the stale cache's SHA at check time. There are TWO un-scoped readers: the completion finalizers (`validated_completion_override`) AND the shared get_task_context hint path (`compute_validation_cache` async at `http_server/helpers.rs:1606` + the pure `compute_validation_hint` at `:1562`; NOTE there is no `is_skip_test_validation` symbol), the latter HEAD/`commit_sha`-only and shared by worker/reviewer/merger.

**ROOT CAUSE (F1b — confirmed on disk):** `validated_completion_override` (`chat_service_handlers.rs:249-277`) touches NO repository — it reads `task.metadata` plus `GitService::get_head_sha`. A stale finalizer for attempt A firing while `agent_run_repo` errors (`IdentityUnknown`) could, if allowed to consult the override, match a HEAD-matched cache and drive a live attempt B to `PendingReview`, bypassing the fail-closed step gate.

**DECIDED FIX (episode-freshness gate, both readers; identity-gated rescue):**
1. **No `ValidationCacheMetadata` change** — `captured_at` already exists. No struct migration, no serde change, no run_id/chain fields.
2. `chat_service_handlers.rs:231-277` — add `validation_cache_fresh_for_episode` (H4); change `validated_completion_override` to take `episode_entered_at: DateTime<Utc>` and gate on the new predicate.
3. Route both completion read sites through `resolve_current_execution_attempt` (H2): success path `935`/`1012` and AgentExit `~2255`/`:2481`. Call `validated_completion_override(&task, episode_entered_at)` **ONLY when the resolver returned `Current { task, episode_entered_at }`. Under `IdentityUnknown`, do NOT call the override; pass `validation_complete=false` so only the step-gated path (fail-closed) decides.**
4. **Shared get_task_context hint reader (the second un-scoped reader) — episode-freshness too:** thread the execution episode boundary into `compute_validation_cache` (async, `http_server/helpers.rs:1606`, called at `:1424` inside the get_task_context builder) and the pure `compute_validation_hint` (`:1562`). There is NO `is_skip_test_validation` symbol — do not grep for it. The boundary read is feasible because `state`/`task_repo` are in scope at the caller (`:1427` `load_task_followup_sessions(state, ...)`), so pass the `task_repo` through. This reader is shared by worker/reviewer/merger via get_task_context, not reviewer-only — threading the boundary is benign for all three (a prior-episode cache correctly resolves to `run_tests`). Resolve the boundary as `max(get_status_last_entered_at(task, Executing), get_status_last_entered_at(task, ReExecuting))` for the task under review, and require `cache.captured_at >= boundary` before emitting a skip/green hint. This is reader-agnostic (works where run-stamping would not) and has NO false-negative cost on the reviewer path — a wrongly-rejected cache merely means the reviewer re-runs its own validation (the safe default). If the boundary read errors → do NOT skip (run validation). Record the decision in a code comment replacing the prior incorrect "reviewer keys on commit_sha so leave untouched" justification.
5. **No on_enter cache clear** (H6 scope change). Cross-attempt protection is entirely the episode-freshness gate. F10's lifecycle clears (WI-9) are independent hygiene on no-in-flight-run paths.

**WHY episode-freshness is recovery-safe (proof obligation, must be covered by tests):**
- `execute_entry_actions` (`task_transition_service.rs:2925`) re-runs `on_enter` side-effects WITHOUT calling `transition_task*` → appends NO status-history row. A shutdown-interrupted resume (`startup_jobs.rs:1080`, `reconciliation/handlers/execution.rs:2274`) therefore leaves `get_status_last_entered_at(Executing)` unchanged at the original episode entry `T_b`. The same-attempt cache (`captured_at = T_c >= T_b`) stays trusted → rescue still works → no reintroduced false-negative.
- A genuine fresh re-execution (revision loop, freshness requeue, Failed→Ready→Executing) DOES append a new entry at `T_new > T_b`; a prior-attempt cache (`captured_at < T_new`) is rejected → cross-attempt leak closed.

**TEST UPDATES (lighter than the prior stamping migration — NO arity change for run ids):**
- The existing rescue/override tests are the regression guard for the false-negative fix and remain GREEN, but must set the mock `get_status_last_entered_at` to a timestamp BEFORE the fixture cache's `captured_at` so the fresh cache passes the episode gate:
  - `validation_cache_fixture` (`chat_service_handlers_tests.rs:500`) — ensure it sets `captured_at` to a known, controllable instant (add an optional `captured_at` parameter or a `fresh_validation_cache_fixture` variant). Do NOT add `agent_run_id`/`run_chain_id` parameters (not needed).
  - `test_success_finalizer_uses_head_matched_validation_cache_for_failed_steps` (`:1452`) and `test_task_execution_agent_exit_uses_head_matched_validation_cache_for_failed_steps` (`:3144`) — configure the mock so the latest `Executing`/`ReExecuting` entry is BEFORE the cache `captured_at` ⇒ still rescues to `PendingReview`/`Reviewing`.
  - Direct-call override tests `..._false_when_no_metadata`/`..._no_validation_cache_key`/`..._malformed`/`..._worktree_path_missing`/`..._head_sha_unresolvable` (`:1071`/`:1077`/`:1084`/`:1092`/`:1101`) — pass an `episode_entered_at` arg (the only signature change).
  - Keep the SHA-only predicate tests at `:565-598` on the UNCHANGED `validation_cache_proves_completion` (do not migrate those).

**NEW-RISK GUARDRAILS:**
- Same-wall-clock-second residual: a prior cache captured in the exact second a new episode is entered would have `captured_at == T_new` and pass `>=`. Narrow and errs toward the already-accepted false-positive class, NOT the false-negative the branch fixes (§9). Optional tightening (deferred): require `captured_at > episode_entered_at` only when `captured_by == "execution_complete"` and a monotonic episode sequence is available — out of scope here.
- The episode boundary must be the latest `Executing`/`ReExecuting` entry; using the earliest (today's `get_status_entered_at`) would defeat the gate. H1 is mandatory.

**TESTS-FIRST:**
- `chat_service_handlers_tests.rs`:
  - `test_validation_cache_fresh_for_episode_rejects_cache_captured_before_latest_entry` (green + HEAD-match + `captured_at < episode_entered_at` ⇒ false).
  - `test_validation_cache_fresh_for_episode_accepts_cache_captured_after_latest_entry` (⇒ true).
  - `test_validation_cache_fresh_for_episode_rejects_sha_mismatch_and_red` (SHA mismatch / `tests_ran=false` / `tests_passed=false` ⇒ false regardless of freshness).
  - `test_validated_completion_override_cross_attempt_stale_cache_does_not_rescue` (cache captured before a fresh re-execution entry, matching HEAD ⇒ Failed) and `..._same_episode_cache_rescues` (captured after ⇒ PendingReview).
  - **F1b:** `test_identity_unknown_does_not_consult_validation_cache_rescue` — `resolve_current_execution_attempt` returns `IdentityUnknown` (run-read or latest-entry-read errors) AND a HEAD-matching cache exists ⇒ override NOT consulted ⇒ step-gated fail-closed ⇒ NOT `PendingReview`.
  - **Shutdown-resume regression (gap #4):** `test_shutdown_interrupted_resume_preserves_same_attempt_cache_rescue` — task stuck `Executing` holding its own green cache; re-driven via `execute_entry_actions(Executing)`/finalizer with the latest entry UNCHANGED (no new history row) and `captured_at >= entry` ⇒ STILL rescues to `PendingReview` (cache NOT lost).
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::' --lib`
- Reviewer-leak: `test_reviewer_does_not_skip_validation_when_only_cross_attempt_cache_before_latest_exec_entry` and `test_reviewer_skips_validation_for_same_episode_cache`.
  - `cargo test --manifest-path src-tauri/Cargo.toml --test http_helpers` (or the parent-module filter if the sidecar stays in `src/`).

### WI-2 — F3 step-repo fail-open (BLOCKS MERGE) — uses H5

**ROOT CAUSE:** The branch replaced merge-base `steps_tracked = task_step_repo.is_some()` (fail CLOSED) with `has_tracked_steps()` whose `Err` arm returns `false` (`203-221`), conflating `Ok(empty)` (legit zero steps → has_output fallback is the intended fix) with `Err` (unknown → must fail closed). With `steps_tracked=false`, `should_transition_task_execution_to_pending_review` (`305-315`) falls into `else { has_output }`, so a transient DB error on an incomplete-steps task that produced output advances to `PendingReview`.

**DECIDED FIX (tri-state single fetch):**
1. Add `StepCompletionState` + `fetch_step_completion_state` per H5.
2. `chat_service_handlers.rs:1010-1011` — replace the two `get_by_task` calls (`all_steps_completed` + `has_tracked_steps`) at the SUCCESS-PATH call site with one `fetch_step_completion_state`. Retain `all_steps_completed` (AgentExit `:2479` still calls it — H5 RETENTION).
3. Rewrite the pure `execution_completion_action` (`323-340`) over the enum (also consumes `completion_tool_called` from F4/WI-3):
   - `AllComplete` → `PendingReview`.
   - `NoSteps` → `PendingReview` iff (`completion_tool_called` OR `validation_complete`) else `Failed`.
   - `Incomplete` → `PendingReview` iff `validation_complete` else `Failed`.
   - `Unknown` → `PendingReview` iff `validation_complete` else `Failed`.
   - **Critical:** `Unknown` and `Incomplete` MUST NOT consult `has_output` (fail CLOSED). Core regression assertion. `validation_complete` is already gated to `false` under `IdentityUnknown` (F1b), so a same-outage scenario (run-read error AND step-read error) yields `Failed`.
4. Keep the episode-gated `validated_completion_override` (WI-1) as the rescue so genuinely-green same-episode HEAD work still advances.
5. The AgentExit site (`2476-2496`) is already fail-closed (`all_steps_done || validation_complete`) — leave the step logic unchanged; only the F1b identity gate (WI-1 step 3) and F4 plumbing apply.

**NEW-RISK GUARDRAILS:**
- Mapping MUST preserve the two intended additions: `NoSteps` + signal/cache → PendingReview; `AllComplete` → PendingReview. Guard with explicit per-state tests.
- Failing closed on `Unknown` → `Failed` → auto-retry (matches merge-base; lesser evil; memory note "Failed step is terminal — no cleanup" — confirm Failed→retry recovers).
- Remove the redundant double `get_by_task` at the success path (one fetch). Reads stay inside `db.run` via the repo impl.

**TESTS-FIRST (pure fn over the enum):**
- `test_execution_completion_action_unknown_with_output_no_validation_is_failed` (CORE regression).
- `test_execution_completion_action_unknown_with_validation_is_pending_review`.
- `test_execution_completion_action_no_steps_with_output_is_pending_review` and `..._no_output_is_failed` (has_output gated behind signal per F4).
- `test_execution_completion_action_incomplete_with_output_no_validation_is_failed` and `..._incomplete_with_validation_is_pending_review`.
- `test_execution_completion_action_all_complete_is_pending_review` (regardless of has_output).
- Integration with existing `StubErrorTaskStepRepo` (`chat_service_handlers_tests.rs:873-915`): `test_fetch_step_completion_state_returns_unknown_on_err_and_none`; `test_handle_stream_success_incomplete_steps_db_error_does_not_advance`.
- **Invert/replace** `test_has_tracked_steps_false_on_repo_error` (`1064-1068`) — currently codifies fail-open; replace with the `Unknown`-fails-closed assertion (or delete if `has_tracked_steps` is removed for having no callers).
- `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::' --lib`

### WI-3 — F4 zero-step no-execution-complete (BLOCKS MERGE) — uses H5

**ROOT CAUSE:** Switching `steps_tracked` from `task_step_repo.is_some()` (always true in prod) to `has_tracked_steps()` (true only with real step rows) newly enabled the dormant `else has_output` branch. The faithful `CompletionSignalTracker.was_called()` (set at `streaming.rs ~:1734`/`:3469`, consumed `~:2986`/`:3635`) is dropped at the `StreamOutcome` boundary (`911-932` has no field), so a worker that exits 0 with chatter, registers no steps, and never calls `execution_complete` reaches review.

**DECIDED FIX (thread the in-stream signal through ALL call sites):**
1. Add `StreamOutcome.completion_tool_called` per H5; set at both construction sites (Claude `~:2945`, Codex `~:3588`).
2. **Enumerate and update ALL THREE `handle_stream_success` call sites:**
   - **Production path** (`chat_service_send_background.rs ~:1304`/`:1371`): thread `StreamOutcome.completion_tool_called`.
   - **Internal `handle_stream_error` Cancelled+turns_finalized>0** (`chat_service_handlers.rs:1605`): supply `StreamError::Cancelled.completion_tool_called` (in scope at `:1574`-`:1576`). ❌ Do NOT pass literal `false` — that regresses a zero-step `TaskExecution` worker Cancelled-after-TurnComplete (output, no steps, no green cache) to `Failed`, a NEW false-negative for a path the merge-base routed to `PendingReview`.
   - **Internal `handle_stream_error` Cancelled+completion_tool_called=true+turns_finalized=0** (`chat_service_handlers.rs:1685`): pass `true` (guard at `:1666` guarantees it). Carries a `debug_assert!` that the context is `Ideation` (`:1667-1671`); does not affect the TaskExecution gate but must compile with the correct value.
3. Propagate `completion_tool_called` into `execution_completion_action`.
4. Zero-step rule (in WI-2's rewrite): `NoSteps` → PendingReview iff (`completion_tool_called` OR `validation_complete`).
5. Update the 6 existing `handle_stream_success` call sites in `chat_service_handlers_tests.rs` for the new arity.
6. Leave the steps-tracked path, validation-cache path, and the AgentExit override (`2476-2496`) step logic unchanged.

**NEW-RISK GUARDRAILS:**
- Narrow new false-negative: a zero-step worker killed in the completion grace window before its `tool_use` block is observed → Failed. Mitigated: the in-stream marker is normally set when `tool_use` appears (precedes the HTTP call that closes stdin); supplying `test_result` still triggers `validation_complete` rescue independent of the tracker.
- Codex backend MUST set the field or zero-step Codex silently regresses to Failed.
- The `:1605` Cancelled-as-success internal call is the one most likely to be missed; its test is mandatory.

**TESTS-FIRST:**
- Pure: `test_zero_step_output_without_signal_or_validation_is_failed`; `test_zero_step_output_with_signal_is_pending_review`; `test_zero_step_validation_rescue_without_signal_is_pending_review`; `test_steps_tracked_all_done_ignores_completion_signal`.
- Internal-caller: `test_cancelled_turns_finalized_zero_step_with_signal_is_pending_review` (drives `:1605` with `StreamError::Cancelled { turns_finalized: 1, completion_tool_called: true }`, no steps, no cache ⇒ PendingReview, NOT regressed).
- Streaming-level: `test_stream_outcome_completion_tool_called_mirrors_tracker_claude` and `..._codex` (incl. no-call ⇒ false).
- `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::' --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml --test chat_service_streaming`

### WI-4 — F1 stale-attempt guard (HIGH, not a blocker on its own; F1b sub-part IS a blocker, handled in WI-1) — uses H1, H2

**ROOT CAUSE:** Both guards derive the attempt boundary from `get_status_entered_at` (EARLIEST entry: SQLite `ORDER BY created_at ASC LIMIT 1` at `sqlite_task_repo/mod.rs:523-528`; memory `.min()` at `memory_task_repo/mod.rs:215-222`). `Executing`/`ReExecuting` are re-enterable (revision loop, freshness requeue, pause/resume, failed-retry), so the boundary stays pinned at the first entry T_a; a stale first-attempt run (`started_at~T_a`) satisfies `T_a+1s >= T_a` and passes while the live attempt B (entered at T_b>T_a) is the real one. Both guards also fail OPEN, returning `true`/`Some` on DB `Err` (`1428`/`1453`/`1467`/`1491`). Pre-existing on main; the branch lets a slipped-through stale finalizer reuse the live attempt's green cache — which is why the F1b identity gate (WI-1) is the merge-blocking part.

**DECIDED FIX:** Latest-entry timestamp as the ACCEPT gate (surgical) + run-chain positive-evidence REJECT (layered).
1. Add H1 `get_status_last_entered_at` (trait at corrected path + sqlite DESC,rowid DESC + memory `.max()` + all 7 test doubles).
2. Replace both `task_execution_attempt_matches_current_status` and `load_current_task_execution_attempt` with H2 `resolve_current_execution_attempt`. Route both success (`935`/incomplete `1060`) and AgentExit (`~2255`/`2476`) through it.
3. Source `tolerance` from `runtime_config` (not an inline const).

**NEW-RISK GUARDRAILS:**
- Preserve the spawn-before-history tolerance (named runtime config, not removed) or a legit current run whose `started_at` slightly precedes its status-history row is wrongly rejected → stranded task.
- `IdentityUnknown` (DB error) ⇒ proceed-as-current FOR THE STEP-GATED PATH (no strand) but NEVER consult the cache rescue (F1b). The F3 tri-state step gate fails closed, so a same-outage scenario yields `Failed`+auto-retry, never a false-complete and never a stuck-Executing.
- Run-chain layer must be reject-only on POSITIVE older-chain evidence; ambiguous/errored chain resolution must NOT reject.
- The same `episode_entered_at` H2 returns for the accept gate is reused by the cache rescue (single source of truth — the boundary that gates acceptance also gates evidence freshness).

**TESTS-FIRST:**
- `memory_task_repo/tests.rs`: `test_get_status_last_entered_at_returns_latest` (mirror earliest test at `tests.rs:305`), plus None-when-never-entered + nonexistent-task.
  - `cargo test --manifest-path src-tauri/Cargo.toml 'infrastructure::memory::memory_task_repo::tests::' --lib`
- `sqlite_task_repo`: `test_get_status_last_entered_at_returns_most_recent` incl. a same-second double-entry case proving the `rowid` tiebreaker picks the truly latest row.
  - `cargo test --manifest-path src-tauri/Cargo.toml 'infrastructure::sqlite::sqlite_task_repo::tests::' --lib`
- `chat_service_handlers_tests.rs`: `test_resolve_attempt_rejects_stale_reexecuting_first_attempt` (status_entered reflects T_b re-entry; completing run started~T_a<T_b ⇒ Stale) and `..._accepts_current_run` (started~T_b ⇒ Current). Cover both ReExecuting (revision loop) and Executing (resume/requeue). Add `test_stale_finalizer_cannot_reach_validation_cache_rescue` (live-status case) plus the F1b `test_identity_unknown_does_not_consult_validation_cache_rescue` (WI-1, DB-error case).
  - `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::' --lib`

### WI-5 — F5a setup-failure surfacing + F5b path containment — uses H7

**ROOT CAUSE:** `run_pre_execution_setup` (`install.rs:317-441`) binds `run_setup_phase`'s failure bool to `_setup_had_failures` (`388`) and computes `success = !install_had_failures` (`422`), so worktree_setup failures never reach the `run_and_store_pre_execution_setup` block/warn decision (`execution.rs:143-191`), whose error string even reads "install command(s) failed". Contract `src-tauri/CLAUDE.md:118` says mode gates setup-failure blocking, but only install is mode-gated.

**DECIDED FIX (narrow surfacing — NOT fold-all-into-block):**
1. Extend `PreExecSetupResult` per H7 with `setup_had_failures` + derived `hard_setup_failure` (`phase=="setup" && status==failed && exit_code.is_none()` ⇒ SpawnError; exclude `Cancelled`, soft non-zero symlink exits, `skipped`/`cached`).
2. `install.rs:388/422/440` — capture the bool, populate the fields; keep `success = !install_had_failures` as the readiness gate.
3. `execution.rs:143-191` — keep install-failure block; ADD a `Block/AutoFix`-only branch returning `AppError::ExecutionBlocked` on `hard_setup_failure`; warn (`execution_setup_warning` metadata) on soft failures; NEVER block on `Cancelled` or in `Warn/Off`. Fix the misleading "install command(s) failed" string.
4. Reconcile `src-tauri/CLAUDE.md:118` to: "mode gates install AND hard setup failure, not whether setup runs."
5. F5b (UNCONDITIONAL across all modes, NOT just one sink — corrected in the final round): add `ensure_path_within` to `path_safety.rs` (H7) and validate EVERY analysis-derived join AND parsed symlink target/parent at ALL FIVE sink families, reusing the existing `crate::utils::path_safety` helpers (`validate_absolute_non_root_path` + `checked_*`); reject, do not sanitize:
   - **`execution.rs` (~:89-106):** before `run_pre_execution_setup`, validate `exec_cwd` against the expected worktree root (`compute_task_worktree_path(&project, task_id)`) via `ensure_path_within`; on `Err`, skip/return (containment is UNCONDITIONAL — applies in Off too, NOT mode-gated). Replace the bare `exists()` at `~:100` with the checked path.
   - **`merge_validation/setup.rs`:** validate each per-entry `cmd_cwd` (`merge_cwd.join(resolve(&entry.path))`) AND each parsed symlink `target`/`parent` against the worktree/merge root via `ensure_path_within` BEFORE `create_dir_all(parent)` `:353`, `read_link` `:128`, `remove_file` `:136`/`:166`, and spawn `:427`. This is the real injection surface (analysis JSON `entry.path` with `../`). Use `checked_*` for `read_link`/`remove_file`.
   - **`merge_validation/install.rs`:** validate `cmd_cwd` (`exec_cwd.join(resolve(&entry.path))`) BEFORE `nm_path` `read_link` `:16`, `remove_file` `:40`, and spawn `:128`.
   - **`chat_service_handlers.rs` `validated_completion_override` (:262-265):** validate `task.worktree_path` via `validate_absolute_non_root_path` (or `ensure_path_within` against the expected worktree root) BEFORE `GitService::get_head_sha`; on `Err` return `false` (matches the existing safe-fallback contract). Co-locates with the WI-1/F1b change already touching this fn.
   - **CodeQL:** suppress `rust/path-injection` only AFTER validation, on the blank line above each ACTUAL sink — `mod.rs:91`, `setup.rs:353`/`:427`, `install.rs:40`/`:128`, `git_cmd.rs:107` — NOT at the `execution.rs` call site (the shared `mod.rs:91` sink stays tainted via the merge path unless `cmd_cwd` is validated at the construction sites).
6. Mirror `_setup_had_failures` discard at `mod.rs:451` only if that flow's `success` should reflect it — out of scope unless it changes pre-exec gating; document the decision.

**NEW-RISK GUARDRAILS:** Do NOT fold `Cancelled`/benign `target/` symlink/`skipped`/`cached` into a hard block (reintroduces the false-negative class this branch removes). Changing `PreExecSetupResult`'s shape touches pre-exec + merge-validation construction sites and any field-asserting tests. Validating ONLY `exec_cwd` (the worktree root) is INSUFFICIENT — the per-entry `resolve(&entry.path)` joins and parsed symlink target/parent are the real path-injection components and need their own `ensure_path_within` check against the root.

**TESTS-FIRST (transition_handler tests, `transitions_agents.rs` style):**
- `test_pre_exec_block_mode_soft_setup_failure_warns_not_blocks`.
- `test_pre_exec_block_mode_hard_setup_spawn_error_blocks` (ExecutionBlocked).
- `test_pre_exec_warn_and_off_modes_setup_failure_never_blocks`.
- `test_pre_exec_cancelled_setup_is_not_blocking`.
- `test_pre_exec_skipped_cached_setup_entries_emit_no_warning`.
- Caller-level (MockChatService): worker NOT spawned when `run_and_store_pre_execution_setup` returns ExecutionBlocked; spawned (with warning metadata) in Warn/Off.
- **F5b containment (unit, in the existing `path_safety` `#[cfg(test)]` mod, mirroring `rejects_parent_components`/`accepts_absolute_child_path`):** `ensure_path_within` accepts a normal child under the canonical root; rejects absolute-escape, `../` traversal, and a symlink-resolved escape.
- **F5b per-sink (transition_handler tests):** `execution.rs` pre-exec — `worktree_path` containing `../` (or outside `compute_task_worktree_path` root) → setup skipped/blocked, `run_pre_execution_setup` NOT reached; valid path → proceeds. **Assert this holds in Off mode too (containment unconditional).** `merge_validation/setup.rs` — analysis `entry.path = "../escape"` (or symlink target escaping `merge_cwd`) → `create_dir_all`/spawn NOT executed for that entry, accepted normal entry still runs (cover the `target.parent()` `create_dir_all` sink specifically). `merge_validation/install.rs` — `entry.path` with `../` → `cmd_cwd` rejected before `node_modules` `read_link`/`remove_file` and before spawn; normal entry still installs.
- **F5b override sink (`application::chat_service::chat_service_handlers::tests`):** `validated_completion_override` with `task.worktree_path` = relative or `../`-containing → returns `false` (no `get_head_sha` launch); valid path + green HEAD-matched fresh cache → returns `true`. Integrate with the WI-1 episode-gate test so containment failure short-circuits before the cache check.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list | rg transitions_agents` then run the resolved module filter; or `cargo test --manifest-path src-tauri/Cargo.toml --test execution_control_flows`; plus the `path_safety` unit filter and `application::chat_service::chat_service_handlers::tests` for the override sink.

### WI-6 — F7 prompt no-tests payload (prompt-only)

**ROOT CAUSE:** `agents/ralphx-execution-worker/claude/prompt.md:182` instructs the no-tests case to send `test_result: { tests_ran: false }`, omitting `tests_passed`. `tests_passed` is required at three layers (MCP `step-tools.ts:218` `required:["tests_ran","tests_passed"]`; proxy `index.ts:944-957` drops `undefined`; Rust `TestResultInput.tests_passed: bool` non-Option, `types.rs:1941-1948`). The present-but-malformed `test_result` fails Axum `Json` deser → 422 before the handler runs, skipping the IPR stdin-EOF clean exit (`steps.rs:732-751`). Does NOT affect completion correctness.

**DECIDED FIX (prompt-only; no Rust/MCP-schema change, no rebuild):**
1. `agents/ralphx-execution-worker/claude/prompt.md:182` — change the no-tests case to OMIT `test_result` entirely: call `execution_complete` with `task_id` (+ `summary`) only; "an absent `test_result` simply skips the validation cache" (consistent with `types.rs:1954`).
2. Mirror in `agents/ralphx-execution-worker/codex/prompt.md` (omit when no tests ran).
3. Do NOT touch the Rust struct, MCP schema, or the hedged Codex harness-native delegation language (sub-claim 2 is NOT a defect; `delegate_start` is ideation-scoped and the worker lacks the grant).

**NEW-RISK GUARDRAILS:** Confirm the reviewer's `skip_test_validation` path treats a missing cache as "no green evidence, run my own validation" (it should — absent cache is the default). Enforcement still lives in the schema; a model ignoring the prompt still hits 422. Note: WI-1 step 4 adds episode-freshness scoping to this same reviewer reader; keep the two changes consistent (absent cache and stale-episode cache both mean "run my own validation").

**TESTS-FIRST:**
- Agent-prompt contract test: assert `agents/ralphx-execution-worker/claude/prompt.md` does NOT contain `test_result: { tests_ran: false }` and the no-tests case omits `test_result`.
- Rust deser test: `{ "testResult": { "testsRan": false } }` fails to deserialize `ExecutionCompleteRequest`; `{ "summary": "x" }` (no `testResult`) ⇒ `test_result == None`.
- Handler test: body without `test_result` ⇒ 200 and reaches the IPR stdin-EOF removal path.
- `cargo test --manifest-path src-tauri/Cargo.toml --test steps_handlers`; prompt/contract test via its existing harness module.

### WI-7 — F8 docs correction (docs-only)

**ROOT CAUSE:** Handoff docs under `docs/handoffs/failing-tasks-2026-06-26/` were committed alongside the fixes but still frame the completion-gate work (issues 2 & 3) as TODO. Doc `04:177-179` claims `transition_task_corrective→Merged` "auto-unblocks P8 ... and Merge plan" — wrong: `apply_corrective_transition` (`task_transition_service.rs:2783-2834`) sets `internal_status` + history only and runs no `on_enter(Merged)` side effects; `dependency_manager.unblock_dependents()` lives only in `on_enter(Merged)` (`on_enter_states/outcomes.rs:322-335`).

**DECIDED FIX (minimal correction + resolution notes):**
1. `04-p6-p5-dependency-deadlock.md:177-179` — replace the auto-unblock claim with: a corrective transition to Merged does NOT run `on_enter(Merged)` and therefore does NOT call `unblock_dependents`; dependents unblock only via the normal Merged transition (or an additional explicit `BlockersResolved`/unblock).
2. Add a short "Resolution (landed in this branch)" note to `00-OVERVIEW.md`/`02`/`03` pointing at `has_tracked_steps`, `validated_completion_override`, and the `IncompleteSteps`/`AgentIncomplete` classification; demote the completion-gate "Recommended sequencing" item to Done.
3. Keep issues 1 (worktree provisioning) and 4 (dependency reconciliation/self-block loop) framed as STILL-OPEN.

**NEW-RISK GUARDRAILS:** Do NOT over-correct to "all fixed" — issues 1 and 4 remain open. Do not advise a corrective jump to Merged on a task whose work was never implemented.

**TESTS-FIRST:** Documentation-only — no code test obligation. Optional (out of scope): a single assertion in `transition_handler/tests/transitions_agents.rs` that a corrective transition to Merged does NOT unblock a dependent.

### WI-8 — F9 test-quality gaps

**ROOT CAUSE:** Setup-before-spawn/Block-prevents-spawn covered only by helper-level `transitions_agents.rs` tests (`80`/`134`/`182`) that call `run_and_store_pre_execution_setup` directly (`98`/`150`/`198`), never `on_enter(&State::Executing)`/`enter_executing_state` (`execution.rs:653-719`). Block assertion at `207-210` uses `err.to_string().contains("Pre-execution setup failed")` instead of `matches!(err, AppError::ExecutionBlocked(_))` (violates Key Principle #5; production returns `ExecutionBlocked` at `execution.rs:152`). `enter_reexecuting_state` setup-before-spawn is uncovered. AgentExit rescue test (`chat_service_handlers_tests.rs:3128`) asserts only final status, missing a negative assertion on stale failure metadata.

**DECIDED FIX (on_enter-level + typed-error + negative-metadata):**
1. Add `on_enter(&State::Executing)` and `on_enter(&State::ReExecuting)` tests injecting a spawn-counting `MockChatService` + a Block-mode failing-install `detected_analysis`: assert `matches!(err, AppError::ExecutionBlocked(_))` AND worker spawn `call_count==0`; plus an Off-mode case asserting setup ran AND spawn happened (proves setup-before-spawn through the real entry).
2. Replace `transitions_agents.rs:207-210` string assertion with `matches!(err, AppError::ExecutionBlocked(_))`.
3. Extend the AgentExit rescue test (`:3128`) and success-finalizer rescue tests (`:1437`/`:1514`) to assert the rescued `PendingReview`/`Reviewing` task carries NO `last_agent_error`/`failure_error`/`is_timeout` metadata (assert absence of those SPECIFIC keys; `validation_cache` is legitimately present).

**NEW-RISK GUARDRAILS:** on_enter tests need real worktree dirs + a deterministically failing install (`"install":"false"`) executed via `sh` through `tool_paths`; keep them in the existing tempdir fixture (avoid ambient-HOME/path-sink). Use a worker-spawn-specific predicate (like `test_entering_executing_uses_chat_service`), not raw `call_count`.

**TESTS-FIRST (names):** `test_on_enter_executing_block_mode_failing_install_blocks_and_does_not_spawn`, `test_on_enter_executing_off_mode_runs_setup_and_spawns`, `test_on_enter_reexecuting_block_prevents_spawn`, `test_on_enter_reexecuting_off_runs_setup_then_spawns`, `test_agent_exit_cache_rescue_has_no_stale_failure_metadata`.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list | rg transitions_agents` → run resolved filter; `cargo test --manifest-path src-tauri/Cargo.toml 'application::chat_service::chat_service_handlers::tests::' --lib`.

### WI-9 — F10 reconciliation cache lifecycle + startup source fidelity — uses H6

**ROOT CAUSE:** Cache has no lifecycle: nothing clears `validation_cache` on restart/auto-retry/reset. `clear_execution_flat_metadata` (`metadata.rs:736-752`) removes only `is_timeout`/`failure_error`; `reset_execution_recovery_metadata` (`760-791`) and `move_task` terminal→Ready (`mutation.rs:340-415`) leave it. Separately, startup `recover_timeout_failures` (`execution.rs:388-399`) hardcodes `(TransientTimeout, Timeout)` for all non-git failures, dropping the `AgentIncomplete→IncompleteSteps` mapping that the recurring loop preserves (`execution.rs:1041`, `1283-1287`).

> Note: with WI-1's episode-freshness gate, a stale cache already cannot rescue a different attempt via either reader. These clears are hygiene (avoid re-running already-validated tasks / clean diagnostics) on no-in-flight-run paths — they are NOT the cross-attempt protection mechanism, and they are deliberately NOT placed on `on_enter(Executing/ReExecuting)` (that would reintroduce the recovery-resume false-negative; see H6).

**DECIDED FIX:**
1. H6 `clear_validated_completion_cache` wired into: (a) `move_task` terminal→Ready (`mutation.rs:340-415`, inside the same `task_repo.update` block); (b) `reconcile_failed_execution_task` auto-retry (`execution.rs ~1224-1232`); (c) `reset_execution_recovery_metadata` (`metadata.rs:760-791`). All via `update_metadata` (DbConnection) — no `internal_status` mutation. **No on_enter clear** (H6 scope).
2. `execution.rs:388-399` — replace hardcoded `(TransientTimeout, Timeout)` with `recovery.events.last().failure_source` mapped via `failure_source_to_reason_code` (`metadata.rs:937-939`); keep `is_git_isolation_startup` budget routing unchanged.

**NEW-RISK GUARDRAILS:** These three paths have no in-flight run, so clearing is safe (unlike on_enter). Preserving the startup source must not change git-vs-global budget routing (`is_git_isolation_startup` at `258-276`). No new path sinks / async DB patterns.

**TESTS-FIRST:**
- `test_move_task_restart_clears_validation_cache` (preserve_steps/recovery reset still applies).
- `test_reconcile_auto_retry_clears_validation_cache`.
- `test_reset_execution_recovery_metadata_clears_validation_cache`.
- `test_startup_recovery_preserves_agent_incomplete_source` (last event AgentIncomplete + last_state Retrying ⇒ AutoRetryTriggered carries AgentIncomplete/IncompleteSteps, not TransientTimeout/Timeout).
- Regression: `test_green_run_same_episode_cache_still_promotes` (episode-gated `validated_completion_override` still rescues a live same-episode green HEAD cache); base SHA-only predicate guards (`validation_cache_proves_completion` SHA-mismatch / tests_ran=false / tests_passed=false) stay green.
- `cargo test --manifest-path src-tauri/Cargo.toml --test startup_jobs_runner` and the reconciliation module filter (`-- --list | rg reconciliation`).

### WI-10 — F6 emit-before-gate (LOW, tracked follow-up)

**ROOT CAUSE:** `execution_complete_http` (`steps.rs:764-792`) persists `task:execution_completed` (`outcome=completed`) + webhook at call time, before the completion gate decides PendingReview vs Failed. A run the gate later fails has already published "completed" externally. Internal state is already idempotent: the status check + H2 latest-entry guard make the internal transition single-fire (a stale/duplicate finalizer hits status≠Executing → Stale early-return).

**DECIDED FIX (compensate, do NOT hard-reorder as a merge requirement):** Either (a) relabel the call-time external event as "signal received" and emit the authoritative `completed` outcome behind the gate's PendingReview branch, OR (b) add a `Failed`-outcome external event on the gate's Failed path so external state is not left falsely "completed." Choose (b) if minimizing external-consumer contract change. **Defer past the merge-blocking subset.**

**TESTS-FIRST (when implemented):** `test_execution_complete_failed_gate_emits_failed_external_event` or the relabel equivalent; assert external_events row + webhook payload outcome matches final gate decision.

---

## 5. Cross-Finding Interactions

- **F2 ↔ F10 (cache lifecycle):** F2's episode-freshness gate (`captured_at >= latest exec entry`, via H1) is the SOLE cross-attempt protection for BOTH readers; it is non-destructive and recovery-safe. F10's H6 clears are hygiene on no-in-flight-run paths only. **There is NO on_enter cache clear** — the prior "primary mechanism" was proven to reintroduce the false-negative under recovery re-drive and is deleted.
- **F1 ↔ F1b ↔ F2 ↔ F6 (attempt-identity seam):** All converge on "is this finalizer the live attempt?". F1 (H2 resolver) provides attempt authority AND the episode boundary timestamp; F2 (H4 episode-freshness) provides temporal evidence authority using that boundary; **F1b gates the evidence rescue behind positive identity** (`validated_completion_override` is consulted only on `Current{...}`, and under `IdentityUnknown` even the episode-boundary read fails → fail closed); F6's internal idempotency relies on H2's status≠Executing → Stale early-return. The success block (`1010-1074`), the AgentExit override (`2476-2496`), and the two internal Cancelled-as-success calls (`1605`/`1685`) are the shared call sites — edit as ONE change set.
- **F3 ↔ F4 (shared gate predicate):** Both rewrite `execution_completion_action` / `should_transition_task_execution_to_pending_review` and consume H5. F4 adds `completion_tool_called` (threaded through ALL THREE `handle_stream_success` call sites); F3 adds the tri-state enum. Land together so the pure-fn test battery covers all states × signal × validation.
- **Deliberate asymmetry:** attempt-identity `IdentityUnknown` ⇒ proceed-as-current FOR THE STEP-GATED PATH ONLY (never strand in Executing) AND NEVER consult the cache rescue; step-state `Unknown` ⇒ fail closed to Failed (never advance via `has_output`). Because the cache rescue is gated behind `Current{...}` AND its episode-boundary read fails under outage, a simultaneous DB outage (run-read error → `IdentityUnknown`, step-read error → step `Unknown`, `validation_complete` forced `false`) yields `Failed`+auto-retry — never a false-complete and never a stuck task.
- **Recovery re-drive ↔ episode-freshness (the round-2 crux):** `execute_entry_actions` re-runs `on_enter` WITHOUT a transition, so it appends no status-history row. This is the load-bearing invariant that makes the episode-freshness gate recovery-safe (shutdown-resume preserves the episode boundary → same-attempt cache stays trusted) AND that makes any on_enter cache clear unsafe (it would run during recovery re-drive and destroy that same cache). The fix uses the invariant in the read predicate, not in a destructive clear.

---

## 6. Sequencing / Dependency Graph

```
            ┌─ H1 get_status_last_entered_at (trait@crates/ralphx-domain + sqlite + memory + 7 doubles)
shared ─────┼─ H2 resolve_current_execution_attempt → Current{task, episode_entered_at} / Stale / IdentityUnknown
helpers     ├─ H4 validation_cache_fresh_for_episode (captured_at >= episode boundary; NO struct migration)
(land       ├─ H5 StepCompletionState + StreamOutcome.completion_tool_called
first)      └─ H6 clear_validated_completion_cache (F10 no-in-flight-run paths ONLY — NOT on_enter)
                       │     [H3 DELETED — no write-side run stamping]
   ┌───────────────────┼───────────────────────────────┐
   ▼ (MERGE-BLOCKING SUBSET — ONE coordinated change set)│
 WI-1 F2+F1b (H1,H2,H4) ─┐                               │
 WI-2 F3 (H5)            ├─ rewrite chat_service_handlers.rs success block 1010-1074
 WI-3 F4 (H5)            ┘   + AgentExit override 2476-2496
                         │    + internal Cancelled-as-success calls 1605/1685
                         │    + reviewer reader http_server/helpers.rs (episode boundary)
                         │    + update existing rescue/override tests for the episode gate
                       │
   ▼ (low-risk hardening — same branch, can follow)
 WI-4 F1 (H1,H2) ── replaces both guards + routes all finalizer paths through H2
 WI-5 F5a/F5b (H7) ── independent (merge_validation/on_enter)
 WI-9 F10 (H6) ──── independent (reconciliation/mutation), reuses H6 (no-in-flight paths only)
 WI-6 F7 ───────── independent (prompt-only, no rebuild)
 WI-7 F8 ───────── independent (docs-only)
 WI-8 F9 ───────── depends on WI-1/WI-5 landing (asserts new behavior)
 WI-10 F6 ──────── deferred follow-up (external-event consistency)
```

**Parallelizable (independent files):** WI-5 (`merge_validation/`, `on_enter_states/execution.rs`), WI-6 (prompt markdown), WI-7 (docs), WI-9 (`reconciliation/`, `commands/task_commands/mutation.rs`).
**Serialized:** Everything touching `chat_service_handlers.rs:1010-1074` / `2476-2496` / `1605` / `1685` and the reviewer reader (`http_server/helpers.rs`) (WI-1/2/3, then WI-4) must be one change set per coordination rule.

**Atomic commit plan (compiles after each):**
1. `feat: add run-aware completion helpers (H1,H2,H4,H5,H6)` — `get_status_last_entered_at` (corrected trait path + sqlite/memory + all 7 test doubles) + `resolve_current_execution_attempt` returning the episode boundary + `validation_cache_fresh_for_episode` (no struct change) + `StreamOutcome.completion_tool_called` (both construction sites) + tri-state enum/fetch + `clear_validated_completion_cache` helper. New helper unit tests. Compiles; all 9 impls present.
2. `fix: scope validation cache to execution episode + identity-gated rescue + tri-state step gate + zero-step signal (F2,F1b,F3,F4)` — the single coordinated `chat_service_handlers.rs` (success block + AgentExit override + internal 1605/1685 calls) + `chat_service_send_background.rs` + reviewer reader (`http_server/helpers.rs`) rewrite; update existing rescue/override tests for the episode gate; invert `test_has_tracked_steps_false_on_repo_error`; add the shutdown-resume regression test. **This commit makes merge-ready.**
3. `fix: route finalizers through latest-entry attempt resolver (F1)` — H2 latest-entry hardening + tolerance from runtime_config + guard replacement.
4. `fix: surface hard setup failures + worktree containment (F5)` — H7.
5. `fix: clear validation cache on no-in-flight lifecycle paths + preserve startup failure source (F10)`.
6. `fix(prompt): omit test_result for no-tests execution_complete (F7)`.
7. `test: on_enter setup-before-spawn + typed ExecutionBlocked + negative rescue metadata (F9)`.
8. `docs: correct handoff completion-gate resolution + corrective-merge unblock note (F8)`.
9. (deferred) `fix: align external execution-completed event with gate outcome (F6)`.

---

## 7. Acceptance Criteria / Definition of Done

- All merge-blocking work items (WI-1 incl. F1b, WI-2, WI-3) plus their helpers (H1, H2, H4, H5) landed in one coordinated change set; F1/F5/F7/F8/F9/F10 landed or explicitly tracked. **No run/chain stamping struct migration and no on_enter cache clear are present** (both were proven harmful/redundant in round 2).
- Every test named in §4 written FIRST, failing before the fix, passing after. Pass/fail counts reported. **The existing rescue/override tests at `:1452`/`:3144`/`:1071`-`:1108` remain GREEN as the false-negative regression guard, updated only to set the mock's latest-entry timestamp before the fixture cache `captured_at` (NOT deleted, NOT given run-id arity).** The shutdown-resume regression test (`test_shutdown_interrupted_resume_preserves_same_attempt_cache_rescue`) is mandatory and proves the episode gate does NOT reintroduce the false-negative under recovery.
- **Zero-lint/zero-test gate (NON-NEGOTIABLE, root CLAUDE.md #8):**
  - `cargo clippy --all-targets --all-features -- -D warnings` clean (fix pre-existing too; if blocked by the documented pre-existing `filter_map_bool_then` in `chat_service/mod.rs ~L1579`, prove your code clean with a scoped `-A` and note it).
  - `cargo nextest run --manifest-path src-tauri/Cargo.toml --lib --profile ci` green.
  - `cargo test --manifest-path src-tauri/Cargo.toml --doc` green.
  - `cargo test --manifest-path src-tauri/crates/ralphx-domain/Cargo.toml` green (H1 trait method added to the ralphx-domain mock at `task_repository_tests.rs:63` — required or this target won't compile).
  - Integration binaries compile (`tests/chat_service_context.rs`, `tests/review_service.rs`, `tests/apply_service.rs` each implement the new H1 method).
  - Local CI parity: `scripts/test-rust-fast.sh pr`.
- No new clippy warnings; no `cargo check`/full `cargo test` (hangs).
- **End-to-end execution smoke (manual, user-driven app):** (1) worker all steps complete → PendingReview; (2) worker zero-step + `execution_complete` called → PendingReview; (3) worker zero-step + chatter, no signal, no green cache → Failed → auto-retry; (4) **fresh re-execution at unchanged HEAD with a PRIOR attempt's green cache → does NOT false-complete (completion path AND reviewer skip path both run validation, because the prior cache's `captured_at` precedes the new Executing entry)**; (5) **shutdown/restart mid-completion with the SAME attempt's green cache → still rescues to PendingReview (no false-negative)**; (6) Block-mode hard setup failure → ExecutionBlocked, worker NOT spawned; (7) Warn/Off setup failure → warns, spawns. Verify user-visible state changed; do not trust stale logs.
- Transition discipline verified: all status changes via `TaskTransitionService::transition_task*`; all metadata via `update_metadata`/`MetadataUpdate` (DbConnection `db.run`/`db.query_optional`); no direct `internal_status` writes; CodeQL `rust/path-injection` suppressed only after containment validation (F5b).
- No MCP server source changed (F7 is prompt markdown only) ⇒ no `npm run build` required. If any `plugins/app/ralphx-mcp-server/src/**` is touched, rebuild before commit.

---

## 8. Non-Goals / Deferred LOW Items

- **Run/chain stamping of `ValidationCacheMetadata`** — REJECTED (round 2): adds a struct migration + ~6 test migrations, does not reach the HEAD-only reviewer reader, and creates a shutdown-resume false-negative if recovery re-drive does not continue the run chain. Superseded by the episode-freshness gate.
- **Unconditional (or any) on_enter(`Executing`/`ReExecuting`) `validation_cache` clear** — REJECTED (round 2): fires during recovery re-drive (`execute_entry_actions`) on a still-Executing task holding its own green cache, reintroducing the exact false-negative the branch removes.
- **F6 external-event reordering** — deferred follow-up; internal state is already single-fire/idempotent.
- **F1 same-second/within-tolerance re-entry residual race** — option (a) latest-entry + run-chain reject closes the realistic vector; a fully unambiguous monotonic attempt counter is explicitly NOT in scope.
- **F7 sub-claim 2 (Codex harness-native delegation)** — NOT a bug; do not touch the hedged delegation language or `delegate_start` scoping.
- **F8 issues 1 (worktree provisioning) and 4 (dependency reconciliation / self-block loop)** — remain OPEN.
- **Making `transition_task_corrective→Merged` actually unblock dependents** — behavior change with its own tests, out of scope for the F8 docs fix.
- **Relaxing the Rust `TestResultInput` schema / MCP `tests_passed` requirement (F7 option c)** — rejected; prompt-only fix chosen to avoid an MCP rebuild.
- **`mod.rs:451` merge-validation `_setup_had_failures` discard** — separate code path; only address if it changes pre-exec gating.

## 9. Known Residual Risks (LOW, accepted/deferred)

- **Exact-instant cross-attempt false-positive (episode-freshness residual) — REVISED DOWN in the final round.** The prior revision sized this as a full "same-wall-clock-second" window on the belief that `created_at` is second-granular. That is WRONG: production rows are SUB-SECOND (`persist_status_change` writes `Utc::now().to_rfc3339()`, nanosecond precision; the second-granular `strftime` is only an unused column DEFAULT), and both `captured_at` (`Utc::now()` via `execution_complete`) and `episode_entered_at` (`DateTime::parse_from_rfc3339`, preserves sub-second) compare at full precision on parsed `DateTime<Utc>`. So the residual collapses to an EXACT-nanosecond tie (`captured_at == episode_entered_at`), which is effectively impossible across two distinct attempts — a prior cache is always captured before the prior episode's exit transition, which strictly precedes the new episode's entry. The pivot is therefore SAFER than the prior revision documented; the `rowid DESC` tiebreaker is defensive-only and the monotonic-sequence follow-up is **downgraded in priority** accordingly. Still errs toward the already-accepted false-positive class, NOT the false-negative the branch exists to fix. **Recommended follow-up (low priority):** an optional monotonic per-episode sequence / `captured_by` strict-`>` tiebreaker; tracked, not in the merge-blocking set. Keep the comparison on parsed `DateTime<Utc>` — do NOT weaken to a string compare.
- **F6 emit-before-gate** — external "completed" event can precede a later Failed gate decision; internal state is idempotent. Tracked as WI-10, deferred.
- **Zero-step worker killed in the completion grace window before its `tool_use` block is observed** → Failed (F4 narrow new false-negative). Mitigated by the in-stream marker normally being set when `tool_use` appears and by `validation_complete` rescue when `test_result` is supplied; accepted as LOW.
- **Reviewer episode-freshness depends on the same `get_status_last_entered_at` boundary being readable at review time.** If that read errors, the reviewer conservatively runs its own validation (safe false-negative for an optimization — redundant test run, never an incorrect skip). Accepted.
- **`mod.rs:451` merge-validation `_setup_had_failures` discard** — left as-is unless shown to change pre-exec gating; documented decision, not fixed in this change set.
- **Recovery-re-drive episode-boundary invariant.** The episode-freshness gate's recovery-safety rests on `execute_entry_actions` appending no status-history row (verified on disk: `task_transition_service.rs:2925` runs entry actions without `transition_task*`). If a future change makes recovery re-drive perform an actual `Executing→Executing` transition (appending a new history row), the same-attempt shutdown-resume cache would be wrongly rejected → false-negative; this invariant MUST be guarded by `test_shutdown_interrupted_resume_preserves_same_attempt_cache_rescue` and re-checked on any reconciliation/startup-recovery change. Recorded as a maintenance constraint.
- **Shared get_task_context hint reader affects worker/reviewer/merger (final round).** The episode boundary threaded into `compute_validation_cache`/`compute_validation_hint` (WI-1 step 4) is on the SHARED get_task_context hint path, not a reviewer-exclusive reader. Threading the boundary is benign for all three audiences (a prior-episode cache resolves to `run_tests` for each). If that boundary read errors at hint time, the hint conservatively resolves to "run validation" (a redundant test run, never an incorrect skip). Accepted.

---

## 10. Final Confirmation Round (independent)

### Independent pivot-skeptic — verdict: `pivot_sound_zero_blocking`

The episode-freshness gate holds under independent adversarial review. (a) No reintroduced false-negative: the only same-attempt re-entry of Executing/ReExecuting is `execute_entry_actions`, which appends no status-history row (`task_transition_service.rs:2925`); the self-transition guard makes a second same-status history row structurally impossible without first leaving the status, so a shutdown-resume re-drive provably leaves `episode_entered_at` unchanged and keeps a same-attempt cache trusted. (b) No cross-attempt false-positive: any genuine re-execution must leave-then-return via `transition_task` (new history row at `T_new`), and a prior attempt's cache is always captured before that prior episode's exit, which precedes `T_new` — and sub-second `to_rfc3339` precision closes the feared same-second window down to an impossible exact-tie. (c) No compile/contradiction blocker: `captured_at` is a required pre-existing field (no migration), the LATEST-entry query is writable, and both readers are reachable with the boundary in scope.

**Gaps raised by the pivot-skeptic (both LOW, non-blocking — resolved as spec-accuracy corrections):**
- **Gap A — `created_at` granularity:** spec claimed second-granular RFC3339 (making the `rowid DESC` tiebreaker "mandatory" and sizing a same-second residual). On disk, `persist_status_change` writes `Utc::now().to_rfc3339()` (sub-second); the second-granular `strftime` is only the unused column DEFAULT. **Resolved:** corrected the H1 rationale (§3) and the §9 residual to sub-second precision, downgraded the `rowid DESC` tiebreaker to defensive-only and the monotonic-sequence follow-up's priority; kept the comparison on parsed `DateTime<Utc>`.
- **Gap B — non-existent symbol `is_skip_test_validation`:** the real reviewer-skip surface is `compute_validation_cache` (async, `helpers.rs:1606`) + `compute_validation_hint` (pure, `:1562`), and it is the SHARED get_task_context hint path used by worker/reviewer/merger, not reviewer-only. **Resolved:** replaced the symbol throughout (§3 audit note, WI-1 ROOT CAUSE, WI-1 step 4), noted scope and that `state`/`task_repo` are in scope at the `:1424` caller for boundary threading, and recorded the shared-reader residual in §9.

### F5b path-containment grounding (its original verifier crashed; re-run) — `stillReal: true`, severity MEDIUM, prior spec treatment **NOT adequate**

Real, currently-unvalidated path sinks confirmed on disk across FOUR sink families (process-launch `current_dir` + symlink read/remove + `create_dir_all`), all fed by DB- or analysis-JSON-derived paths with no containment: `execution.rs` `exec_cwd` (exists()-only), shared `merge_validation/mod.rs:91` spawn cwd, `merge_validation/setup.rs` (`cmd_cwd` + parsed symlink target/parent → `create_dir_all` `:353`, `read_link` `:128`, `remove_file` `:136`/`:166`, spawn `:427`), `merge_validation/install.rs` (`cmd_cwd`/`nm_path` → `read_link` `:16`, `remove_file` `:40`, spawn `:128`), and `git_cmd.rs:107` `current_dir` via `GitService::get_head_sha` from `validated_completion_override` (`chat_service_handlers.rs:262-265`, no check at all).

**Chosen containment helper:** REUSE the existing app-owned `crate::utils::path_safety` (`validate_absolute_non_root_path` + `checked_*` family, already the `git_service/worktree.rs` pattern); add ONE shared `ensure_path_within(root, candidate, context)` (canonicalize trusted root + assert candidate-under-root) since no such helper exists yet. Do NOT invent a task-specific `ensure_worktree_path_contained`.

**Corrections applied to the spec (H7 + WI-5 step 5 + WI-5 TESTS-FIRST):**
- Replaced the invented `ensure_worktree_path_contained` with the existing `path_safety` reuse + one new `ensure_path_within`.
- Expanded the single-sink (`exec_cwd`-only) treatment to the full five-family sink inventory, including the per-entry `resolve(&entry.path)` joins and parsed symlink target/parent (the real injection components) and the omitted `git_cmd.rs`/`get_head_sha` sink (validate in `validated_completion_override`, return `false` on `Err`).
- Moved the CodeQL suppression from the `execution.rs` call site to the blank line above each ACTUAL sink (`mod.rs:91`, `setup.rs:353`/`:427`, `install.rs:40`/`:128`, `git_cmd.rs:107`), with validation applied at the `cmd_cwd` construction sites so the shared `mod.rs:91` sink is not masked while still tainted.
- Made containment UNCONDITIONAL across `merge_validation_mode` (Off has identical sink exposure); only F5a blocking stays mode-gated.
- Clarified F5b is hardening of pre-existing unvalidated sinks, NOT a branch regression (`merge_validation/*` and `git_service/*` are unchanged vs. main).
- Added F5b test obligations: `path_safety` unit (accept child / reject absolute-escape / `../` / symlink-escape), per-sink transition_handler tests (incl. Off-mode unconditionality and the `target.parent()` `create_dir_all` sink), and the `validated_completion_override` containment short-circuit integrated with the WI-1 episode-gate test.

### Convergence statement

Per the orchestrator's computed convergence (pivot has 0 blocking gaps; F5b spec-treatment-adequate was `false`): the spec entered this round with a **residual blocking gap** (F5b's inadequate sink coverage). That gap has now been resolved at the SPEC level by the surgical H7/WI-5 corrections above (full sink inventory, reused helper, corrected suppression placement, unconditional containment). The pivot SURVIVED independent attack with only two LOW spec-accuracy gaps, both corrected. No CRITICAL/HIGH/MEDIUM gap remains unaddressed in the spec text. **The spec is now converged for implementation handoff**, with the understanding that F5b containment rides WI-5 (low-risk hardening, same branch) and is NOT part of the merge-blocking subset (WI-1/WI-2/WI-3 + helpers).
