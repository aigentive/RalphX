> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# src-tauri/CLAUDE.md — Backend

Quality standards: `../.claude/rules/code-quality-standards.md` | Rust API safety: `../.claude/rules/rust-stable-apis.md` | Thinking capture: `../.claude/rules/agent-thinking-capture.md`

## Stack
Rust 2021 | Tauri 2.0 | rusqlite 0.32 | statig 0.3 (async state machine)
tokio 1.x | serde 1.x | chrono 0.4 | thiserror 1.x | async-trait 0.1 | tracing 0.1

## Key Directories
```
src-tauri/src/
├─ domain/
│  ├─ entities/        # Re-exports ralphx-domain entities (Task, Project, InternalStatus)
│  ├─ repositories/    # Traits (interfaces)
│  └─ state_machine/   # machine/, transition_handler/
├─ application/
│  ├─ app_state.rs     # DI container
│  └─ *_service.rs     # Business logic
├─ commands/           # Thin Tauri IPC wrappers
├─ http_server/        # Axum :3847 handlers/routes for MCP adapters
└─ infrastructure/
   ├─ sqlite/          # Repo implementations
   └─ memory/          # Test repos
src-tauri/crates/
├─ ralphx-domain/      # Pure entities, AgenticClient trait, review/scope_drift logic
└─ ralphx-events/      # Object-safe event sink
```

## Architecture: Clean/Hexagonal
```
Commands (Tauri IPC) → Application Services → Domain Layer ← NO INFRA DEPS → Infrastructure
```

### Dual AppState (CRITICAL)
TWO `AppState` object graphs exist (Tauri commands + HTTP/MCP server), wired in `app_setup.rs` / `server_boot.rs` / `runtime_wiring.rs`; the HTTP state is built from the Tauri state's **shared physical SQLite connection**. Any `Arc<T>` coordinating between them MUST be explicitly cloned in `runtime_wiring.rs`. ❌ Relying on `new_production()` defaults.

| Shared State | What Breaks If Not Shared |
|---|---|
| `question_state` | MCP questions never reach Tauri UI |
| `permission_state` | Permission prompts never shown |
| `message_queue` | Messages lost between IPC/HTTP |
| `interactive_process_registry` | Teammate→lead nudge fails |
| event sink/bus, durable queue repo, streaming cache, GitHub service, PR poller, webhook publisher, merge locks, capability gate | Event/notification/runtime coordination silently diverges between the two graphs |

## Patterns

### Repository Pattern
Trait in `domain/repositories/` → impls: `sqlite_*_repo.rs` | `memory_*_repo.rs`. All async with `#[async_trait]`.

### Newtype IDs
`pub struct TaskId(pub String)` — compile-time safety, can't pass `TaskId` where `ProjectId` expected.

### DbConnection (NON-NEGOTIABLE)
All SQLite repos MUST use `db.run(|conn| { ... })` / `db.query_optional(|conn| { ... })`. ❌ `conn.lock().await`. See `db_connection.rs`.

### DI via AppState
`AppState` holds `Arc<dyn XRepository>` for all repos. `new_production()` → SQLite | `new_test()` → Memory.

### Error Handling
`AppError` enum with domain-specific variants + `AppResult<T>`. ❌ Generic string errors. ❌ `error == "some string"` — use `matches!(err, MyError::Variant)`. External strings → named `pub(crate) const` (e.g., `AGENT_ERROR_PREFIX`).

## Rules

### State Machine (CRITICAL)
Refs: task-state-machine.md (28 states) | task-git-branching.md (git/merge) | task-execution-agents.md (agents)
❌ `task.internal_status = X` for live workflow paths | ✅ validated `TaskTransitionService::transition_task*()` or `handler.handle_transition(&state, &TaskEvent::Schedule).await` | ✅ nonstandard repair only via `transition_task_corrective()` / `apply_corrective_transition()` | ✅ direct status writes stay confined to canonical transition-handler / merge-engine internals that also own history/events
Auto-transitions: QaPassed→PendingReview | PendingReview→Reviewing | RevisionNeeded→ReExecuting | Approved→PendingMerge
Review approval gate: AI review may continue `review_passed → approved` when `require_human_review=false`, but do not shortcut `reviewing → approved`
API layer: see api-layer.md

### Command Handlers (THIN)
5-10 lines max — extract, delegate to service, return. ❌ Business logic in commands.

### Permission Bridge Flow
Agent → `permission_request` MCP → POST `/api/permission/request` → backend emits `permission:request` → UI dialog → user Allow/Deny → `resolve_permission_request` → MCP long-poll returns decision

### Test File Separation (NON-NEGOTIABLE)
❌ `#[cfg(test)] mod` or `#[path = "..."]` in production files. Tests → dedicated `*_tests.rs` importing from `crate::`.

### Conventions
Types: PascalCase | Functions/files: snake_case | Enums: `#[serde(rename_all="snake_case")]` | Tauri inputs: `#[serde(rename_all = "camelCase")]` | JSON: snake_case | Dates: RFC3339

### Architectural Patterns
New pattern → add one-liner here. Pattern name + rule only.

| Pattern | Rule |
|---|---|
| User-message delivery contract | Queue-drain/session gates may refuse to resume a session, never to discard a user message; blocked continuations fall back to fresh-session replay (`chat_service_queue.rs`), and staleness applies only to hidden recovery messages. |
| Backend-owned Startup Gate | `StartupCoordinator` is the sole readiness writer: window first → AppState registration → listener + safety barrier → interactive shell → owned finite recovery settlement; timers/localStorage/recurring loops never authorize readiness |
| Reuse before invent (NON-NEGOTIABLE) | New behavior extends the seam that owns the domain — transitions → `TaskTransitionService`, publish/review gates → `agent_workspace_review*`, events → `AppState.events`, spawns → `provider_onboarding_gate` + `harness_runtime_registry`, git primitives → `git_service/`, queueing → `chat_service_queue` + durable repo, recovery → the domain's dedicated recovery module. ❌ New parallel services/engines/managers for owned concerns |
| Validated task transitions | Normal workflow status changes use validated `TaskTransitionService::transition_task*`; corrective/recovery-only jumps use `transition_task_corrective()` / `apply_corrective_transition()`; raw `internal_status` writes are limited to canonical engine/bootstrap paths |
| Shared scope drift logic | Review/merge scope matching and out-of-scope blocker fingerprints should live in `ralphx-domain::review::scope_drift`; root crate code should only handle repo/git wiring |
| Follow-up blocker dedupe | Autonomous blocker follow-ups dedupe by first-class `blocker_fingerprint`; never rely on `spawn_reason` wording alone. See `.claude/rules/followup-blocker-dedupe.md` |
| EventSink emission | New backend code emits via `AppState.events` (`crates/ralphx-events` object-safe sink; Tauri adapter at `src/shell/event_sink.rs`), never `AppHandle` directly; emission is fire-and-forget/non-fatal |
| Transport-owned run identity | Model-facing MCP schemas must not accept run/orchestration IDs; inject `agentRunId` from MCP runtime context and validate against the active monitor (workspace-review pattern) |
| Two-stage provider spawn gate | Every spawn path (send, queued resend, recovery, background transition, startup) requires an enabled default provider AND an enabled selected provider — `provider_onboarding_gate.rs`; missing provider settings fail closed |
| Agent runtime-state envelope | Volatile per-turn workspace/task/delegation/ledger/branch/plan/team state is composed once by `application/agent_runtime_context` and injected by every spawn path including true resume; the composer reads only, keeps network/subprocess refresh off the message path, and reuses `chat_service::escape_attr` instead of adding another XML escaper |
| Context vs. reference vs. authorization | Volatile per-turn state → best-effort runtime envelope; user-selected artifacts → frozen message-attached composer references; plan admission/fingerprint gates → fail-closed command layer; never move authorization or message-scoped intent into the degradable envelope |
| Persisted review gate | Workspace Review is persisted `not_required\|required\|reviewing\|passed\|blocking\|failed` state; never infer pass from recency; validity = review scope + diff fingerprint + applicable head (content-equivalent commits preserve it, content changes invalidate) |
| Review-mode boundary | Workspace Review owns the local publish gate; Review PR owns linked remote GitHub PR head/lifecycle/action state. Never share transitions by analogy; see `.claude/rules/agent-workspace-review-modes.md` |
| Typed failure taxonomy | Classify git/merge failures via `MergeFailureSource` → `RetryStrategy`; auth/disk-full/deterministic-infra/unknown never blind-retry; auth text is matched BEFORE broad transient patterns |
| Durable completion proof | Completion authority = accepted `execution_complete` tool RESULT + current attempt + current validation evidence (HEAD + execution episode, non-baseline, tests ran+passed); never call-start, process exit, or commit SHA alone |
| Rustfmt module roots | Never run `rustfmt` on `mod.rs` or other module-root files for a surgical change; rustfmt can recurse into child modules and create unrelated diffs |
| ExecutionState Propagation | `Arc<ExecutionState>` → `TaskTransitionService::new()` + `AgenticClientSpawner::with_execution_state()` |
| Delegation park/wake | Park records are durable, generation-scoped, and deadline-bounded; wake is a hidden `resume_in_place` message dispatched only after `commit_terminal` accepts and a `claim_wake` CAS succeeds; resumed re-parking inherits only the exact park/job tuple named by backend action provenance |
| Agent MCP Tool Allowlist | MCP/tool changes are multi-layer: keep canonical `agents/<agent>/agent.yaml`, prompt contracts, runtime authorization, and registered handlers aligned; see `.claude/rules/agent-mcp-tools.md` |
| Provider-native MCP policy | Third-party MCP definitions/auth/trust stay provider-owned; exact Claude user-scope `ralphx` is reserved cleanup state settled by coherent-home rediscovery, while other scopes/providers and `ralphx_internal` fail closed. See `docs/architecture/provider-native-mcp-policy.md` |
| Backend-routed Project maintenance assignments | Only exact canonical workspace-repair and PR-fixer agents bypass the Project data envelope; render their backend-owned requests as XML-escaped executable assignments |
| Git Modes & Merge | Two modes (Local/Worktree), two-level branches (plan→task) — see task-git-branching.md |
| PreMergeCleanup | Kill agents + kill_worktree_processes BEFORE git worktree ops (TOCTOU race prevention) |
| MergeDeadline | `attempt_programmatic_merge` wraps cleanup + strategy in bounded deadline (`attempt_merge_deadline_secs`) |
| No Inline Timeout Consts | All durations → `runtime_config` + `config/ralphx.yaml`, never Rust `const` |
| Rust test runner split | Local agents use targeted `cargo test` filters and targeted `cargo nextest --test ... -E ...`; broad Rust runs are CI/manual-diagnostic only; fixture rules and commands live in `.claude/rules/rust-test-execution.md` |
| Tauri test-utils gate | Tauri mock-app helpers require `--features test-utils`; keep root lib/IPC CI lanes feature-on until later phases remove lib-side `tauri::test` users |
| Worktree-safe Rust helper | `scripts/test-rust-fast.sh` bundles selected CI lanes for explicit manual diagnosis; ordinary agent handoff never runs its broad `pr`/`main` modes |
| Layering ratchet | `python3 scripts/check-layering.py` blocks new tracked backend layering violations; intentional baseline changes require reviewing `scripts/baselines/layering.json` |
| Workspace domain split | Low-dependency backend modules and pure entities move into `src-tauri/crates/ralphx-domain`; review logic, shared memory/team types, and pure repository traits belong there, while Tauri/SQLite-facing or root-coupled code stays in the root crate until a clean boundary exists |
| Forward-only migration repairs | Never reuse or renumber shipped migration versions; schema repair for already-upgraded DBs must be a new forward-only migration |
| Oversized lib suite split | Move massive orchestration/state-machine/worktree suites out of `src/**` lib tests into existing `src-tauri/tests/suite_*/` modules, and expose only the minimum internal-facing API needed for them |
| HTTP handler suite split | Move large handler sidecar suites to `src-tauri/tests/suite_http_handlers/`; import via `ralphx_lib::http_server::{handlers,types}` and use `AppState::new_sqlite_test()` only for SQLite-backed handler cases |
| HTTP handler module split | Move oversized production handler files to directory-backed modules (`foo/mod.rs` + endpoint-family files) and keep the module root as a thin prelude/re-export layer |
| Team authority and exit | Team HTTP/tool authority derives from the run→binding→session chain in `handlers/managed_team/authority.rs`; never gate on `conversation.bound_agent_name`, and route `RxNativeTeam` capability exits through `exit_team` |
| Mechanical extraction only (NON-NEGOTIABLE) | Large backend module splits must move existing bodies with real extraction commands/scripts (`mv`, `sed`, `awk`, scripted extractors); do not hand-copy/retype large existing functions into new files |
| Apply-patch is fix-up only (NON-NEGOTIABLE) | During a large split, `apply_patch` is only for the post-move fix-up layer: imports, visibility, re-exports, module wiring, and targeted test adjustments |
| Mechanical split rollback | If a backend module split starts drifting into patch-copied bodies or cascading visibility churn, restore the module to `HEAD`, move any parked WIP out of the repo tree, and redo the extraction mechanically instead of continuing the partial split |
| Serial cargo during extractions | While validating a large Rust split, run one targeted Cargo job at a time; concurrent runs only create build-lock noise and hide the real compile/test errors |
| Reference upkeep | When refactors move/split backend modules, update concrete file/path references in `.claude/rules/*`, specialist prompts, and docs in the same change; remove triggers that no longer match live code |
| Ideation/external runtime suite split | Keep ideation and external handler runtime flows in dedicated integration binaries (`ideation_runtime_handlers`, `external_ideation_runtime_handlers`) and keep `.claude/rules/rust-test-execution.md` in sync when splitting more suites |
| Integration helper visibility | When a moved integration suite needs private handler/helpers, expose the minimum surface as `#[doc(hidden)] pub` instead of keeping `#[cfg(test)]` sidecar-only access |
| Test determinism | Integration tests must not rely on ambient `config/ralphx.yaml`, cached runtime config, entity defaults, or default worktree roots like `~/ralphx-worktrees`; set or neutralize each behavioral precondition explicitly in the fixture/helper |
| Sandbox-safe default tests | Default Rust suites must avoid ambient HOME/network/process assumptions; extract OS operations behind seams, keep logic coverage on fakes, and mark true socket/process checks as explicit ignored capability tests |
| Capability test runner split | Ignored lib-side capability checks run via explicit `cargo test -- --ignored`; only add a `nextest` group after moving them into a dedicated integration binary |
| Capability binary convention | Dedicated OS-capability integration suites should use a specific binary name, get one `capability-serial` override in `src-tauri/.config/nextest.toml`, and be listed in `.claude/rules/rust-test-execution.md` |
| Large async state entry | If an `on_enter`/recovery path grows large enough to overflow debug/test stack, extract it to a helper and `Box::pin` that future instead of growing the parent async fn |
| Dependency acknowledgment gate | Multi-proposal `apply_proposals_core` / external apply requires `dependencies_acknowledged=true`; tests must either expect `422` or simulate `analyze_session_dependencies` / explicit dependency edits before apply |
| Completion shutdown grace | After `execution_complete` / `complete_review` / `complete_merge`, stream timeout logic must honor `completion_grace_secs`, match the fully-qualified MCP tool names (`mcp__ralphx__*`), and treat post-completion non-zero exits as successful shutdowns |
| Execution setup before agents | Always run worktree setup before execution when a task has a worktree; `merge_validation_mode` controls whether setup failure blocks, not whether setup runs |
| Stateful workflow false-success review | Completion/cache/retry/recovery/state-machine changes must run the `.claude/rules/stateful-workflow-review.md` lens before handoff |
| Attempt-scoped completion proof | Completion, validation-cache, retry, resume, and finalizer decisions require current-run/attempt evidence; commit SHA alone is not current-run proof |
| Fail-closed progress reads | Repo/query errors must not collapse into "no tracked work" when that permits forward progress; use typed errors or tri-state results |
| Effects after final authority | Completion events/webhooks, terminal metadata, and auto-commit must happen after final backend transition authority accepts the run |
| Claude usage-limit banners | Treat Claude subscription exhaustion text (for example `You've hit your limit` / `You're out of extra usage`) as provider errors even when it arrives as assistant content on the success path; globally pause agent-active work instead of advancing state |
| Ideation history overflow | Oversized ideation session-history messages should be stored as context artifacts and injected as preview + `artifact_id` references; don't inline giant message bodies into bootstrap prompts |
| Execution defaults seeding | `execution_defaults` in `config/ralphx.yaml` may seed only pristine/default execution-settings rows at bootstrap; once DB rows diverge, live DB/UI values are authoritative |
| Execution halt mode contract | Execution status/events must expose persisted halt mode (`running`/`paused`/`stopped`); don't collapse `stopped` into `isPaused` and accidentally re-enable resume UI |
| Artifacts test quiesce | `artifacts_handlers` plan-mutation tests that create a plan first must quiesce auto-verify (reset parent + archive/unregister verification children) unless the test is explicitly asserting freeze/bypass behavior |
| Plan bundle authority | `plan_artifact_id` remains the Overview compatibility anchor; v2 plan actions derive authority from the exact Overview + `plan_blueprint_artifact_id` pair and fail closed when either member is missing |
| SQLite write transactions | `DbConnection::run_transaction()` uses `BEGIN IMMEDIATE`; keep read-then-write sync-helper flows inside it to avoid WAL upgrade failures surfaced as `database is locked` |
| Remote facade registration | `:3849` commands are allowlisted in `remote_server/registry.rs` against existing command fns with a `capability_ledger.rs` class; a `Denied`/over-privileged entry fails compilation |
| Remote event classification | Delivery class comes from the checked-in classification table (`ralphx-remote-protocol`); Durable → sequencer + `remote_event_log`, Transient → broadcast, no seq, never persisted |
| Remote two-instance fixture | Tests needing a real host boot `remote_server::harness` (`test-utils`): production constructors, real loopback listener, `RemoteHostHarness` + `ScriptedClient`. ❌ Hand-rolled routers or auth bypasses |

## Code Quality
Keep work inside the requested feature/refactor/polish scope. File limits + migration rules: `.claude/rules/code-quality-standards.md`.
**500 lines max** (refactor@400). Focused local validation policy — see root CLAUDE.md #8. Public API → doc `/// # Errors` section.

## Database
`ralphx.db` (dev) | Migrations: `infrastructure/sqlite/migrations/` | System: `.claude/rules/code-quality-standards.md`
New migration: `python3 scripts/new_sqlite_migration.py <description>` → `vYYYYMMDDHHMMSS_description.rs` + matching `*_tests.rs`, then register in `MIGRATIONS`, bump `SCHEMA_VERSION`, and run `python3 scripts/validate_sqlite_migrations.py` | Use `IF NOT EXISTS` | `helpers::add_column_if_not_exists()`

## Commands
Local agents: focused commands only; ❌ `cargo check` (hangs) | ❌ broad/full suites | ❌ `--nocapture`
```bash
cargo test --manifest-path src-tauri/Cargo.toml --features test-utils <filter> --lib           # pinpoint lib tests
cargo nextest run --manifest-path src-tauri/Cargo.toml --test <suite> -E 'test(<module_or_test>)'  # targeted integration suites
python3 scripts/check-layering.py                                        # only for layer/import/module-boundary changes
rustfmt --edition 2021 --check <touched-leaf.rs>                         # touched leaves only
```
CI/manual reproduction commands + suite mapping + SQLite test fixture rules → `.claude/rules/rust-test-execution.md`

## Real Integration Tests
Pattern: `tempfile::TempDir` + git CLI → `Memory*Repository` → `TaskServices::new_mock()` | `MockChatService` → `TransitionHandler` → assert state + git.
Shared helpers: `transition_handler/tests/helpers.rs` — `setup_real_git_repo()`, `PendingMergeSetup`, `RealGitRepo`.

| File | Tests | Real | Mocked |
|------|-------|------|--------|
| `tests/suite_transition_git/merge_system_hardening.rs` | 22 | git, MemoryTaskRepo | — |
| `tests/suite_transition_git/deferred_main_merge_integration.rs` | 8 | MemoryTaskRepo | git/merge side effects |
| `src/domain/state_machine/transition_handler/tests/real_git_integration.rs` | 11 | git, merge dispatch | MockChatService |
| `src/domain/state_machine/transition_handler/tests/orchestration_chain_tests.rs` | 3 | git, full state machine | MockChatService |
| `src/domain/state_machine/transition_handler/tests/plan_update_from_main.rs` | 9 | git, pure fn | — |
| `src/domain/state_machine/transition_handler/tests/source_update_from_target.rs` | 8 | git, pure fn | — |
| `src/domain/state_machine/transition_handler/tests/rc12_rc13_stale_worktree.rs` | 5 | git worktrees | — |
| `src/domain/state_machine/transition_handler/tests/merge_cleanup.rs` | 12 | transitions | TaskServices::new_mock() |

## Allowed Clippy Lints
Crate-level `#![allow(clippy::...)]` list lives at the top of `src/lib.rs` (currently 18 lints) — that file is the source of truth; keep new allows there, not per-module.
