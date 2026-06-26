# Worktree Execution Environment Incompleteness

## Summary

When a task executes in a git worktree, that worktree is a clean checkout of the task branch and is **missing two classes of things the main checkout has**: (1) gitignored dependency dirs (`frontend/node_modules`, `target`) that are supposed to be symlinked in by pre-execution setup, and (2) gitignored/untracked runtime files such as `.artifacts/specs/**` and tracker docs that only ever existed in the main checkout and are never carried into a branch worktree. The worker agent's environment-probing bash commands therefore fail (`find`/`rg`/`wc`/`ls`/`eslint`), the Claude CLI process ultimately exits non-zero, and RalphX classifies that exit as `AgentExit` → `AgentCrash`, which is marked retryable and re-queued by the reconciler — producing the ~40x auto-retry loop.

## Affected task(s)

- **`40f6acc4-edb2-4f83-aae4-f285c61db9b6`** ("P6 — Has PR indicators"), worktree `/Users/reefagent/ralphx-worktrees/ralphx/task-40f6acc4-edb2-4f83-aae4-f285c61db9b6`.
- **Shared mechanism, not task-specific.** The worktree-provisioning path (`create_worktree` + `run_pre_execution_setup`) and the agent-exit → retry classification are global to every worktree-mode task execution. Any task whose worktree is missing `node_modules` or references untracked `.artifacts/**` paths will reproduce this.

## Evidence

Concrete errors recorded in the task metadata (`failure_source=agent_crash`, `reason_code=agent_exit`):

1. `find: .artifacts/specs: No such file or directory`
2. `rg: frontend/src/routes: No such file or directory (os error 2)`
3. `eslint` run failed: `Error [ERR_MODULE_NOT_FOUND]: Cannot find package '@eslint/js' imported from .../task-40f6acc4-.../frontend/eslint.config.js`
4. `wc: .artifacts/specs/p6-pr-list-affordances/tracker.md: open: No such file or directory`
5. `ls: frontend/node_modules: No such file or directory`

Errors 3 and 5 are the same root fact: `frontend/node_modules` is absent in the worktree, so `eslint` cannot resolve `@eslint/js`. Errors 1 and 4 are `.artifacts/specs/**` paths that exist only in the main checkout. Error 2 (`frontend/src/routes`) is a path that does not exist on this branch's tree.

## Root cause analysis

### 1. Worktree creation provisions nothing beyond the git checkout

`GitService::create_worktree` (`src-tauri/src/application/git_service/worktree.rs:18-116`) runs only `git worktree add -b <branch> <path> <base>`. It creates the parent dir and handles locked/already-exists races, but it does **not** symlink `node_modules`, create `.artifacts/`, or copy any untracked/gitignored content. A worktree is therefore a pristine checkout of `<base>` with none of the main checkout's gitignored state.

### 2. `node_modules` symlinks come from a separate, conditionally-run setup phase

The symlink commands are **authored by the project-analyzer agent**, not by git code:
- `agents/ralphx-project-analyzer/shared/prompt.md:16` emits `ln -s {project_root}/<path>/node_modules {worktree_path}/<path>/node_modules` per detected entry; lines 32-34 and 74/85 give the root vs sub-package forms.
- These commands are stored as `worktree_setup` arrays inside the project's `detected_analysis` / `custom_analysis` JSON.

They are executed by `run_pre_execution_setup` (`src-tauri/src/domain/state_machine/transition_handler/merge_validation/install.rs:317-441`), which builds a template resolver for `{project_root}`/`{worktree_path}`/`{task_branch}` (install.rs:357-370) and calls `run_setup_phase` (`merge_validation/setup.rs:195-573`) to run each `ln -s` (hardened to `ln -sfn`, setup.rs:394-401).

`run_pre_execution_setup` is invoked from the Executing/ReExecuting on-enter path via `run_and_store_pre_execution_setup` (`src-tauri/src/domain/state_machine/transition_handler/on_enter_states/execution.rs:71-118`). **Three guards can cause the `node_modules` symlink to never be created:**

- **`merge_validation_mode == Off`** → the entire pre-exec setup block is skipped (`execution.rs:90`). If this project has validation Off, no symlink is ever created and `frontend/node_modules` will always be absent in the worktree.
- **No `worktree_setup` entry for `frontend/`** → if the stored analysis has no entry whose `path` is `frontend` (or the analysis is empty/unparseable, install.rs:339-349), no symlink command exists for `frontend/node_modules`. Errors 3 and 5 are consistent with the analysis lacking (or mis-resolving) the `frontend` sub-path entry.
- **`worktree_path` missing / cwd absent** → setup is skipped with a warning (`execution.rs:91-107`).

Even when setup runs, the install phase deliberately *skips* `npm install` if a `node_modules` symlink/dir is already present (`install.rs:80-102`, `node_modules_available_for_install_skip` install.rs:14-51) — so a missing or broken `frontend/node_modules` symlink is never backfilled by an install at execution time.

### 3. `.artifacts/specs/**` and tracker files are never provisioned into a worktree

The project-analyzer explicitly only symlinks **dependency** directories — "Symlink DEPENDENCY directories only: `node_modules/`, `.venv/`, vendor dirs" (`agents/ralphx-project-analyzer/shared/prompt.md:25`), and skips `node_modules/`, `target/`, `.git/`, `dist/`, `build/` when walking (prompt.md:39). `.artifacts/specs/p6-pr-list-affordances/tracker.md` is neither a tracked file (gitignored runtime output) nor a dependency dir, so it is never symlinked or copied. A worktree branched from `<base>` simply does not contain it. The agent (or its plan) was instructed to read a path that only exists in the main checkout — a textbook runtime-root-vs-target-project confusion (`.claude/rules/runtime-root-vs-target-project.md`): the agent assumed the worktree contained the same untracked runtime state as the originating checkout.

### 4. A non-zero agent process exit is classified as a retryable crash

When the Claude CLI process exits unsuccessfully during the stream, the streamer raises `StreamError::AgentExit` (`src-tauri/src/application/chat_service/chat_service_streaming.rs:2915-2918`, "Agent process exited unsuccessfully during stream" at :2895; sibling sites at :3080, :3099, :3221, :3643). That maps to:
- `StreamError::AgentExit → ExecutionFailureSource::AgentCrash` (`src-tauri/src/application/chat_service/chat_service_errors.rs:448`)
- `ExecutionFailureSource::AgentCrash → ExecutionRecoveryReasonCode::AgentExit` (`src-tauri/src/application/reconciliation/metadata.rs:931`)

This is exactly the recorded `failure_source=agent_crash`, `reason_code=agent_exit`.

Note: individual failing bash *tool* calls do not by themselves terminate the Claude process; what is classified here is the **process-level** non-zero exit. The escalation point is the non-zero CLI exit, and the missing-environment errors are what drive the agent toward that exit (repeated failed validation/probing with no recoverable state).

## Why it loops

`AgentExit` is both retryable and routes the task to `Failed`:
- `StreamError::is_retryable()` returns `true` for `AgentExit` (`chat_service_errors.rs:366-375`).
- `StreamError::suggested_task_status()` returns `Failed` for `AgentExit` (`chat_service_errors.rs:392-405`).

On the live failure path, the handler writes an `ExecutionRecoveryMetadata` event with **state `Retrying`** (`src-tauri/src/application/chat_service/chat_service_handlers.rs:2193-2235`, `append_event_with_state(..., ExecutionRecoveryState::Retrying)` at :2226-2229). The reconciler then re-queues the task: `recover_timeout_failures` treats any `Failed` task whose recovery metadata is `Retrying && !stop_retrying` as eligible (`src-tauri/src/application/reconciliation/handlers/execution.rs:212-219`) and transitions it `Failed → Ready` (execution.rs:420-423), which re-enters `Executing` and re-runs the worker.

Because the retry reuses/recreates the **same incomplete worktree** (same missing `frontend/node_modules`, same absent `.artifacts/specs/**`), the worker hits the identical failures and exits non-zero again → another `Retrying` event → another re-queue. The loop is bounded only by `default_reconciliation_execution_failed_max_retries()` (`execution.rs:185`, enforced at :329 `attempt_count >= task_max_retries` → `MaxRetriesExceeded` at `metadata.rs:648`). A ~40x observed count indicates either a high configured cap, or the staleness window / per-source counters resetting across attempts (the staleness skip at execution.rs:221-251 and per-source count selection at :262-275 are worth confirming against the actual recorded metadata).

## Recommended fix

1. **Guarantee the `node_modules` symlink before the worker runs, independent of `merge_validation_mode`.** The symlink (and `.venv`/vendor links) are required for the worktree to be functional even when validation is `Off`. Either decouple the symlink portion of `run_pre_execution_setup` from the `merge_validation_mode != Off` gate at `on_enter_states/execution.rs:90`, or move dependency-symlink provisioning into the worktree-creation path (`git_service/worktree.rs:create_worktree`) so every worktree gets it. Follow the documented `ln -s {project_root}/frontend/node_modules {worktree_path}/frontend/node_modules` form (per-entry, never collapsing sub-paths onto the root target — `agents/ralphx-project-analyzer/shared/prompt.md:32-34`).

2. **Self-heal a missing/broken sub-path symlink.** `node_modules_available_for_install_skip` (`install.rs:14-51`) already removes a broken root symlink before install, but the *setup* phase only creates symlinks for entries present in the stored analysis. Add a verification step that, for each `package.json`-bearing entry actually present in the worktree tree, ensures a valid `node_modules` symlink exists (recreate via `ln -sfn` if missing/broken) before declaring setup complete. This directly addresses errors 3 and 5.

3. **Stop pointing the agent at main-checkout-only paths.** `.artifacts/specs/**` / `tracker.md` must not be read from inside a worktree. Either (a) resolve such RalphX-owned runtime artifacts against the app-owned runtime root rather than the worktree cwd (consistent with `.claude/rules/runtime-root-vs-target-project.md`), or (b) strip those references from the worker prompt / task plan, or (c) provision the specific spec/tracker files into the worktree if they are genuinely task inputs. As written, the plan asks the agent to `find`/`wc` paths that cannot exist in a fresh branch worktree.

4. **Do not classify an environment-incomplete worktree as an infinitely-retryable agent crash.** A worktree that is structurally missing `node_modules` will fail identically on every retry; auto-retrying it ~40 times wastes execution slots. Consider gating `AgentExit` retryability on a precondition check (worktree integrity / required symlinks present) and converting "environment incomplete" into a non-retryable blocked/failed terminal state with a clear reason, rather than `Retrying`. Re-examine the retry eligibility at `chat_service_handlers.rs:2226` and `reconciliation/handlers/execution.rs:212-219`.

5. **Exit-code masking pitfall (`cmd 2>&1 | tail`).** Per the existing memory note ("Worktree frontend node_modules symlink + tail exit masking"), validation/probe commands written as `cmd 2>&1 | tail` take the exit status of `tail` (0), masking the real failure of `cmd`. Whatever prompt/analysis emits validation commands should avoid piping the failing command into `tail`/`head` (or use `set -o pipefail` / `${PIPESTATUS[0]}`), so a genuine `npm run lint` failure is neither hidden nor mis-attributed.

## Open questions / things to verify

- **What is this project's `merge_validation_mode`?** If `Off`, that alone explains the missing `frontend/node_modules` (setup skipped at `execution.rs:90`). If not `Off`, the stored analysis likely lacks a correct `frontend` entry.
- **Inspect the stored analysis JSON** (`detected_analysis` / `custom_analysis` on the project row) for a `worktree_setup` entry targeting `frontend/node_modules`. If absent or mis-pathed, the project-analyzer output is the upstream defect.
- **Confirm whether the Claude process actually exited non-zero**, or whether the failure was a no-output/parse-stall reinterpreted as `AgentExit`. The streamer has multiple `AgentExit` sites (`chat_service_streaming.rs:2915/3080/3099/3221/3643); the recorded stderr should disambiguate.
- **Why ~40 retries and not the configured cap?** Verify `default_reconciliation_execution_failed_max_retries()`'s effective value in `config/ralphx.yaml`, and whether the staleness skip (`execution.rs:221-251`) or per-source count selection (`:262-275`) is letting the attempt counter reset, allowing more retries than intended.
- **Where did the `.artifacts/specs/p6-pr-list-affordances/tracker.md` reference originate** — the worker prompt, the task plan artifact, or agent improvisation? That determines whether fix #3 belongs in the prompt, the plan generator, or path-resolution code.
