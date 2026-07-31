---
paths:
  - "src-tauri/**/*.rs"
  - "src-tauri/CLAUDE.md"
  - ".github/workflows/ci.yml"
  - ".github/workflows/coverage.yml"
  - "scripts/test-rust-fast.sh"
  - "scripts/tests/test-ci-rust-full-integration-targets.sh"
  - "scripts/tests/test-coverage-rust-shards.sh"
  - ".claude/rules/*.md"
---

# Rust Test Execution

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

## Non-Negotiables

| Rule | Detail |
|---|---|
| Local agents run focused Rust validation only | Integration suites → ✅ `cargo nextest run --manifest-path src-tauri/Cargo.toml --test <suite> -E 'test(<module_or_test>)'`; lib pinpoints → ✅ `cargo test --manifest-path src-tauri/Cargo.toml --features test-utils <filter> --lib` |
| `--lib` REQUIRES `--features test-utils` (NON-NEGOTIABLE) | The whole `src-tauri` lib test target must compile before any filter runs, and it depends on Tauri mock-app helpers (`crate::testing::create_mock_app*`, `tauri::test`) that live behind `test-utils`. ❌ bare `cargo test … --lib` fails to compile (E0433/E0425) regardless of filter. The `crates/ralphx-domain` crate tested on its own does NOT need the flag (`cfg(test)` covers it). |
| Never broaden as fallback | If exact test discovery is uncertain, use the nearest relevant module/suite/crate check or report no applicable local test; ❌ broad lib/workspace/full integration as a confidence fallback |
| CI owns broad proof | RalphX workspace CI/autofix owns broad lib/integration suites, dual clippy, doctests, coverage, and remediation; local broad runs require explicit user request or reproduction of a named CI failure |
| Merged suites are nextest-only | `src-tauri/tests/suite_*/main.rs` has a guard that fails under plain libtest; nextest isolates each test process and avoids env/PATH races |
| Broad runner stays CI/manual | `cargo-nextest` remains the broad CI runner; do not invoke broad commands during ordinary agent handoff |
| Dual clippy is CI-owned | The no-default-features and all-targets/all-features clippy matrix runs in CI; reproduce a failing lane locally only when needed |
| Fast wrapper is diagnostic | `scripts/test-rust-fast.sh` bundles CI-shaped lanes for explicit manual/CI-failure reproduction; ordinary agents do not run `pr`, `main`, shards, or `full-integration` |
| Scope the layering ratchet | Run `python3 scripts/check-layering.py` locally only for layer/import/module-boundary changes; CI runs it for every Rust-relevant PR |
| Keep helper runs checkout-local | `scripts/test-rust-fast.sh` resolves paths relative to its own checkout/worktree and refuses to run if the current cwd belongs to a different RalphX checkout |
| Clean after Rust testing | If any Rust test command starts (`cargo test`, `cargo nextest run`, Rust coverage, or a wrapper that executes Rust tests), run `cd src-tauri && cargo clean` as a separate best-effort command in the active workspace once after the final or aborted test attempt and before handoff, whether the test succeeds, fails, times out, is cancelled, or is interrupted; no Rust test means no cleanup. Report cleanup failure and never manually delete target directories as a fallback |
| PATH must honor rustup toolchain | If Cargo reports an older compiler despite `rust-toolchain.toml`, run with `RUSTC=$(rustup which --toolchain 1.91.0 rustc) rustup run 1.91.0 cargo test ...`; Homebrew `cargo` can otherwise drive Homebrew `rustc` |
| `cargo test` name filters are single-filter only | `cargo test <TESTNAME>` / `cargo test --lib <FILTER>` accepts one substring filter; do not append multiple test names and expect Cargo/libtest to combine them |
| No broad formatter runs | ❌ `cargo fmt` / broad `rustfmt` unless user explicitly asks; they can touch hundreds of files and hide the real diff |
| Keep diffs reviewable | Use `apply_patch` for code edits, then verify `git diff` / `git diff --staged` only shows intended hunks |
| Benchmark build-cost changes | Use `scripts/bench-rust-build.sh --label <before\|after>` for profile/linker/crate-type changes; paste compact summaries, not raw logs |
| Heavy SQLite tests use shared temp DB fixtures | Use `ralphx_lib::testing::SqliteTestDb` / `SqliteStateFixture` instead of rerunning migrations into fresh `:memory:` DBs |
| Don’t over-convert narrow utility tests | Pure formatting/connection tests that never run migrations can stay on lightweight `:memory:` setup or direct connection helpers |

## Standard Stack

| Layer | Standard |
|---|---|
| Test runner | `cargo test` for lib pinpoints and ignored lib-side capability checks; `cargo nextest run` for targeted integration suites and broad CI runs |
| Low-dependency workspace crate | `src-tauri/crates/ralphx-domain` holds pure `agents`, `qa`, `execution`, `ideation`, `review`, most `entities`, and the pure repository trait subset; `question`/`permission` repos stay in the root crate until their application-type dependencies move |
| Target discovery | Use `rg` in the owning module, sibling `*_tests.rs`, and mapped integration suite before invoking Cargo; do not compile the root lib merely to list tests |
| Async SQLite repo tests | `SqliteTestDb` + repo `from_shared(db.shared_conn())` |
| AppState integration tests | `SqliteStateFixture::new(...)` |
| HTTP handler integration tests | Import handlers/types through `ralphx_lib::http_server::{handlers, types}` from `src-tauri/tests/suite_http_handlers/*.rs`; use `AppState::new_sqlite_test()` or `AppState::new_sqlite_test_with_registry(...)` only when the handler calls SQLite sync helpers via `db.run(...)` |
| Sync SQLite repo tests | `SqliteTestDb` + `db.new_connection()` |
| Setup/seeding | Shared suite helpers/builders on top of `SqliteTestDb`; one migration pass per temp DB only |
| Concurrency | File-backed temp DBs for shared access; `:memory:` only for intentionally isolated narrow tests |
| Compile-scope reduction | Move oversized state-machine/worktree/orchestration suites out of `src-tauri/src/**` lib tests into an existing `src-tauri/tests/suite_*` integration binary when they only need explicit public/internal-facing APIs |
| Command-suite test seams | When moving a `src-tauri/src/commands/**/tests.rs` sidecar into `src-tauri/tests/suite_*`, re-export any required helper entry points from the command module root with `#[doc(hidden)] pub`; don’t couple integration tests to private submodules |
| Prefer public diagnostics in integration tests | When a moved suite only needs visibility into state, prefer existing public methods like `dump_state()` over widening `#[cfg(test)]` helpers just to keep the old assertions |
| Shared regression helpers | If an integration suite validates shared state-machine logic, expose the minimal helper once with `#[doc(hidden)] pub` rather than duplicating the production logic in the test |
| SQLite sync-helper seams | If a moved SQLite repo suite intentionally tests sync helpers, expose only the specific helper functions it calls with `#[doc(hidden)] pub`; don’t make the whole repo surface public |
| Async command helper seams | For moved command suites that exercise real git/filesystem helpers, expose the existing async helper with `#[doc(hidden)] pub` instead of introducing test-only wrappers |
| Broad-run runner config | Rust workspace config lives in `src-tauri/.config/nextest.toml`; keep group changes there, not in ad hoc shell flags |
| Formatter policy | No broad `cargo fmt`; if formatting is required, keep it scoped and separate |

## Scale Direction

| Topic | Direction |
|---|---|
| Shared state | Keep tests isolated and parallel-safe; avoid shared DB state except for explicitly serialized cases |
| Fixture style | Rust has no built-in fixture system here; use helper modules, suite-local `setup_*()` functions, and small builders |
| Compile vs run | Optimize both separately: narrow targets to reduce compile scope, then keep per-test runtime setup cheap |
| Large-suite runner | `cargo-nextest` is the adopted runner for all merged integration suites; targeted lib edit-loop runs still stay on `cargo test` |
| Test layers | Keep fast repo/unit suites separate from slower integration/state-machine/git suites |
| Large lib suites | When a lib-side test file becomes a massive orchestration suite, prefer moving it to `src-tauri/tests/` and exposing only the minimum internal-facing API with `#[doc(hidden)] pub` rather than keeping it in the giant `--lib` binary |
| Internal support | Invest early in a thin shared test-support layer under `src-tauri/src/testing/` when setup repeats |
| CI coverage split | PR CI owns layering, IPC contracts, root-lib shards, dual clippy, workspace doctests, and full integration; local agents provide only focused evidence before publication |

## CI Topology Maintenance

| Change | Keep in sync |
|---|---|
| Add an integration test module | Prefer an existing `src-tauri/tests/suite_*/main.rs` target and update the suite mapping below; the existing target is already included in the integration archive |
| Add an unavoidable top-level integration target | Add it to `FULL_INTEGRATION_TESTS` in `scripts/test-rust-fast.sh`; add a `nextest.toml` group override only when resource behavior requires one; ❌ duplicate the target list in workflow YAML |
| Change archive execution | Keep one archive producer and partition-only consumers on the same profile/features/workspace remap; consumers must not rebuild Cargo targets |
| Change lib coverage topology | `rust-lib-coverage-archive` is the sole archive producer; four partition-only consumers mirror the CI test archive pattern; update `scripts/tests/test-coverage-rust-shards.sh` and Codecov inputs with shard/artifact changes |
| Add IPC/command coverage | Update the single target/filter union in `.github/workflows/coverage.yml`; keep one `cargo llvm-cov nextest` invocation so filter groups do not relink the instrumented root crate repeatedly |
| Change shard counts or artifact names | Update the matrix, unique artifact/JUnit names, publish-time artifact validation, and every Codecov input together |
| Validate topology changes | Run `scripts/tests/test-ci-rust-full-integration-targets.sh` and `scripts/tests/test-coverage-rust-shards.sh` plus YAML/actionlint checks; do not run broad Rust/llvm-cov suites merely to validate workflow wiring |

## Focused Agent Commands

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features test-utils db_connection --lib
cargo test --manifest-path src-tauri/crates/ralphx-domain/Cargo.toml
cargo nextest run --manifest-path src-tauri/Cargo.toml --features test-utils --test suite_ipc_commands ipc_contract
cargo nextest run --manifest-path src-tauri/Cargo.toml --features test-utils --test suite_commands -E 'test(release_notes_commands)'
cargo nextest run --manifest-path src-tauri/Cargo.toml --test suite_http_handlers -E 'test(artifacts_handlers)'
cargo nextest run --manifest-path src-tauri/Cargo.toml --test suite_sqlite_flows -E 'test(repository_swapping)'
cargo nextest run --manifest-path src-tauri/Cargo.toml --features test-utils --test suite_transition_git -E 'test(transition_handler_freshness)'
python3 scripts/check-layering.py # only when layer/import/module boundaries changed
```

## CI And Explicit Reproduction Commands

```bash
scripts/test-rust-fast.sh pr
scripts/test-rust-fast.sh main
scripts/test-rust-fast.sh full-integration
cargo nextest run --manifest-path src-tauri/Cargo.toml --lib --features test-utils
cargo nextest run --manifest-path src-tauri/Cargo.toml --lib --profile ci --features test-utils
cargo nextest run --manifest-path src-tauri/Cargo.toml --profile ci --features test-utils
cargo test --manifest-path src-tauri/Cargo.toml --workspace --doc
python3 scripts/check-rust-test-features.py
python3 scripts/check-test-suite-modules.py
```

## Integration Suite Mapping

| Suite | Former targets |
|---|---|
| `suite_ipc_commands` | `task_commands`, `api_key_commands`, `project_commands`, `unified_chat_commands`, `task_step_commands`, `harness_provider_commands` |
| `suite_commands` | `activity_commands`, `agent_profile_commands`, `methodology_commands`, `metrics_commands`, `qa_commands`, `release_notes_commands`, `research_commands`, `workflow_commands`, `artifact_commands`, `review_commands`, `review_service`, `plan_branch_commands`, `conversation_stats_commands`, `execution_commands_running_count`, `question_commands`, `git_commands` |
| `suite_http_handlers` | `api_keys_handlers`, `artifacts_handlers`, `conversations_handlers`, `delegation_handlers`, `internal_handlers`, `projects_handlers`, `reliability_tests`, `session_linking_handlers`, `teams_handlers`, `chat_service_streaming`, `ideation_event_emission` |
| `suite_sqlite_repos` | `sqlite_chat_message_repo`, `sqlite_ideation_session_repo`, `external_issue_links`, `clickup_integration_settings`, `granola_integration_settings`, `linear_integration_settings` |
| `suite_sqlite_flows` | `state_machine_flows`, `qa_system_flows`, `review_flows`, `execution_control_flows`, `per_project_execution_scoping`, `workflow_integration`, `artifact_integration`, `methodology_integration`, `gsd_integration`, `research_integration`, `repository_swapping`, `linear_webhook_reconciliation` |
| `suite_metrics` | `metrics_integration`, `metrics_schema_validation`, `metrics_delivery_trends`, `metrics_pr_insights` |
| `suite_chat_service` | `chat_service_errors`, `chat_service_context`, `chat_service_merge`, `chat_service_pause_flows`, `chat_session_recovery_integration`, `pending_session_drain`, `session_fixes_integration`, `session_linking_integration`, `http_helpers` |
| `suite_ideation` | `ideation_service`, `ideation_capacity_counting`, `ideation_webhook_enrichment_test`, `ideation_model_override`, `ideation_commands`, `ideation_runtime_handlers`, `external_ideation_runtime_handlers`, `ideation_plan_delivery_test`, `ideation_handlers`, `apply_service` |
| `suite_transition_git` | `transition_handler_freshness`, `transition_handler_freshness_integration`, `transition_handler_concurrent_freshness`, `webhook_pipeline_integration`, `reviewing_initial_recovery`, `startup_jobs_runner`, `merge_system_hardening`, `deferred_main_merge_integration`, `steps_handlers`, `reviews_handlers`, `git_handlers`, `external_handlers` |
| `suite_pr_github` | `pr_mode_integration`, `pr_mode_fallback`, `pr_mode_acceptance_paths`, `pr_poller_tests`, `pr_reconciler_tests`, `project_pr_template` |
| `suite_interactive_process` | `gate1_ipr_fast_path_tests`, `message_delivery_contract`, `ipr_cleanup_guard_tests`, `interactive_mode_integration`, `team_nudge_running_count_tests`, `task_cleanup_service`, `reconciliation_runner`, `agentic_client_flows`, `supervisor_integration`, `codex_stream_processor`, `codex_cli_capabilities`, `execution_types_serde`, `task_scheduler_service` |
| `suite_agent_workspace` | `agent_workspace_publish_recovery`, `agent_workspace_repair_auto_publish`, `agent_workspace_review` |
| `plan_selector_performance` | stays standalone under `perf-serial` |

## Nextest Setup

| Need | Command |
|---|---|
| Repo Rust toolchain | `rust-toolchain.toml` pins Rust `1.91.0`; keep CI and local development aligned to that file |
| Activate pinned toolchain locally | `rustup toolchain install 1.91.0 && rustup override set 1.91.0` from repo root |
| Homebrew Rust ahead of rustup in PATH | `RUSTC=$(rustup which --toolchain 1.91.0 rustc) rustup run 1.91.0 cargo test --manifest-path src-tauri/Cargo.toml --features test-utils <filter> --lib` |
| Install on macOS | `brew install cargo-nextest` |
| Install from Cargo | `cargo install cargo-nextest --locked` |
| Broad root-lib reproduction (explicit only) | `cargo nextest run --manifest-path src-tauri/Cargo.toml --lib --features test-utils` |
| CI-style root-lib reproduction (explicit only) | `cargo nextest run --manifest-path src-tauri/Cargo.toml --lib --profile ci --features test-utils` |
| CI-style full-suite reproduction (explicit only) | `cargo nextest run --manifest-path src-tauri/Cargo.toml --profile ci --features test-utils` |
| CI clippy feature matrix | `cargo clippy --manifest-path src-tauri/Cargo.toml --lib --bins --no-default-features -- -D warnings && cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings` |
| Pinpoint module/test validation | Lib: `cargo test --manifest-path src-tauri/Cargo.toml --features test-utils <filter> --lib`; integration: `cargo nextest run --manifest-path src-tauri/Cargo.toml --test <suite> -E 'test(<module_or_test>)'` |
| Lib-side capability check | `cargo test --manifest-path src-tauri/Cargo.toml --features test-utils '<filter>' --lib -- --ignored` |
| CI doctests | `cargo test --manifest-path src-tauri/Cargo.toml --workspace --doc` |
| CI broad coverage | `cargo nextest run --manifest-path src-tauri/Cargo.toml --profile ci --features test-utils && cargo test --manifest-path src-tauri/Cargo.toml --workspace --doc` |

## Nextest Groups

| Group | Purpose |
|---|---|
| `git-heavy` | Caps the heaviest git/worktree integration binaries at 2 threads |
| `sqlite-integration` | Caps file-backed SQLite integration binaries at 4 threads |
| `perf-serial` | Forces `plan_selector_performance` to 1 thread |
| `capability-serial` | Reserved for dedicated capability binaries if/when ignored socket/process tests move out of `--lib`; keep lib-side ignored capability checks on explicit `cargo test -- --ignored` runs |
| Config source | Edit `src-tauri/.config/nextest.toml` rather than pasting long `-E` filters into docs or CI |

## Filter Rules

| Need | Use |
|---|---|
| One unit-test/module substring | `cargo test --manifest-path src-tauri/Cargo.toml --features test-utils <filter> --lib` |
| Multiple integration targets in one run | `cargo nextest run --manifest-path src-tauri/Cargo.toml --test suite_sqlite_flows --test suite_metrics` |
| Multiple unrelated unit-test filters | Run separate `cargo test ... --lib` commands sequentially |
| Fast module-path guess | Derive `folder::tree::module::tests::` from the source tree first; for `#[path = "foo_tests.rs"] mod tests;` under `foo.rs`, prefer `...::foo::tests::` |
| Sidecar `*_tests.rs` under a production module | Prefer the parent module path first: `application/review_issue_service_tests.rs` → `application::review_issue_service::tests::`, not `application::review_issue_service_tests::` |
| Legacy standalone `*_tests.rs` modules still exist | Some suites keep the file stem path (`sqlite_team_message_repo_tests`); if the parent-module guess is not obvious, use `-- --list | rg ...` immediately instead of guessing twice |
| Filter misses unexpectedly | Use `rg -n "<repo_or_module>"` in the owning module, sibling tests, and mapped integration suite → rerun with the discovered module/test prefix |
| Parallel verification | ❌ do not start multiple Cargo test jobs against the same target dir; they block on `.cargo-lock` and add noise instead of speed |

Example:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features test-utils sqlite_chat_conversation_repo_tests --lib
cargo test --manifest-path src-tauri/Cargo.toml --features test-utils sqlite_memory_entry_repo_tests --lib
```

Module-path example:

```bash
rg -n "sqlite_question_repo" src-tauri/src/infrastructure/sqlite src-tauri/tests
cargo test --manifest-path src-tauri/Cargo.toml --features test-utils 'infrastructure::sqlite::sqlite_question_repo::tests::' --lib
```

## Shared SQLite Test Setup

| Scenario | Pattern |
|---|---|
| AppState + async SQLite repos | `SqliteStateFixture::new("suite-name", |db, state| { state.repo = Arc::new(SqliteRepo::from_shared(db.shared_conn())); })` |
| Sync `TaskStateMachineRepository` tests | `let db = SqliteTestDb::new("suite"); let conn = db.new_connection(); let repo = TaskStateMachineRepository::new(conn);` |
| Mixed async + sync repos in one suite | One `SqliteTestDb` → `db.shared_conn()` for async repos + `db.new_connection()` for sync repos |
| Fixture lifetime | Keep the fixture bound as `_db` in each test so the temp directory and DB file stay alive for the whole test |
| Raw setup SQL | Insert rows through the opened file-backed connection after fixture creation; do not rerun migrations in each helper |
| Shared seed API | Prefer `db.seed_project(...)`, `db.seed_task(...)`, `db.seed_ideation_session(...)`, `db.seed_ideation_conversation()`, `db.seed_task_conversation(...)`, `db.insert_conversation(...)`, `db.insert_review_note(...)` before adding new suite-local SQL |

## Best Practices

| Rule | Detail |
|---|---|
| Default to isolated file-backed fixtures | Rust tests should stay parallel-safe; use temp file DBs instead of shared globals |
| One helper per suite shape | Extract `setup_*()` returning fixture + repo + seeded IDs when 2+ tests share setup |
| Builders over repeated SQL | Promote repeated inserts into `seed_project(...)`, `seed_task(...)`, `seed_review_note(...)` helpers instead of cloning raw SQL blocks |
| Helpers take `&SqliteTestDb` when possible | Keep seeding logic reusable across async repos, sync repos, and mixed suites |
| Use `db.with_connection(...)` for direct SQL checks | If a suite needs repo calls plus raw SQL assertions, keep the repo on `db.new_connection()` and use `db.with_connection(...)` for direct setup or verification |
| Use `db.shared_conn()` for shared-connection variants | For `from_shared(...)` coverage, pass `db.shared_conn()` instead of building a second ad hoc `Arc<Mutex<Connection>>` |
| Shared helper fixtures keep `_db` alive | If a suite returns `(shared_conn, repo)`-style helpers, include the `SqliteTestDb` in that helper result so temp DB cleanup never races the shared async connection |
| Extend `SqliteTestDb` when patterns repeat | If the same row graph appears across suites, add a shared helper in `src-tauri/src/testing/sqlite_test_db.rs` instead of copying another local setup block |
| Service suites keep fixture ownership explicit | Prefer a small `TestContext { _db, service, ids... }` so the temp DB lifetime is obvious and setup is reused across tests |
| Keep suite-local seed helpers when the shape is narrow | If only one suite needs a specific FK graph, add a local `setup_repo()` / `seed_*()` helper on top of `SqliteTestDb` instead of reaching through `repo.db.inner()` in each test |
| Keep migrations out of per-test setup | Create one temp DB, migrate once, then seed rows; do not call `run_migrations()` inside every helper |
| Prefer explicit fixture ownership | Bind fixture as `_db` in the test body so cleanup timing stays obvious |
| Split slow suites from narrow logic tests | Keep pure/unit logic off SQLite when possible; reserve DB fixtures for repository and integration coverage |
| Sandbox-safe temp paths | If a test only needs “under HOME”, prefer `tempdir_in(std::env::current_dir()?)` over writing into `$HOME` root directly |
| Discover exact libtest paths first | If a filter misses, use `-- --list` before guessing more Cargo invocations |
| Run selective jobs sequentially | Never overlap targeted Cargo runs against the same target dir; they reproduce `Blocking waiting for file lock on build directory` and erase any speed gain. If you need lower local wall-clock time, use `scripts/test-rust-fast.sh lib-parallel` / `pr-parallel`, which isolate `CARGO_TARGET_DIR` per lane |
| When a builder repeats across files, centralize it | Move shared fixture/builders into `src-tauri/src/testing/` once multiple suites need the same seeded graph |

## Agent Guidance

| Situation | Action |
|---|---|
| Converting an old SQLite test | Replace `open_memory_connection() + run_migrations()` with `SqliteTestDb` first, then extract shared seed helpers |
| Reproducing a named Rust CI failure | Run only the failing lane/target first; use `scripts/test-rust-fast.sh pr` / `main` only when the user explicitly requests full parity or the failure cannot be isolated |
| Suspected environmental CI failure (timeout/infra/download flake, not a code signal) | First response: `gh run rerun <run-id> --failed` to re-run only red jobs without a push; a code fix still requires a push and full re-run; ❌ pushing no-op commits to re-trigger CI |
| Seeing remaining `open_memory_connection()` calls after migration work | Check whether the suite is connection/formatting-only before converting it; optimize real migration-replay hotspots first |
| Splitting oversized lib suites | Move them to `src-tauri/tests/<suite>.rs`, compile them as a separate integration binary, and keep the exported surface minimal and explicitly internal-facing |
| Splitting HTTP handler suites | Make the handler/types module reachable from integration tests, import through `ralphx_lib::http_server::{handlers, types}`, and keep SQLite-only handler helpers on `AppState::new_sqlite_test()` / `new_sqlite_test_with_registry()` instead of duplicating ad hoc setup |
| Splitting ideation/external handler runtime suites | Keep runtime-heavy handler flows in dedicated integration binaries such as `ideation_runtime_handlers` and `external_ideation_runtime_handlers`, and add the new targets to the selective command list in this file |
| Runtime-config determinism | Integration tests must not assume ambient `config/ralphx.yaml`, cached runtime config, entity defaults, or default worktree roots like `~/ralphx-worktrees`; set or neutralize the precondition explicitly in suite helpers/builders |
| Sandbox-safe by default | Default `cargo test` / `cargo nextest run` suites should avoid requiring loopback sockets, process killing, or ambient HOME writes; extract those behind seams and keep true OS-capability checks as explicit `#[ignore]` capability tests or dedicated capability targets |
| Capability + nextest alignment | Do not invent a `nextest` group for ignored lib tests; `nextest` broad runs skip them by default. Add a `capability-serial` override only after moving those checks into a dedicated integration binary |
| Dedicated capability binaries | When capability checks graduate out of `--lib`, give the binary a specific name (for example `backend_readiness_capability`), add exactly one `capability-serial` override in `src-tauri/.config/nextest.toml`, and list the command in this file |
| Artifacts handler post-create mutations | In `artifacts_handlers`, tests that create a plan and then mutate it should quiesce auto-verify first (reset parent + archive/unregister verification children) unless they are asserting the freeze/bypass path |
| Exposing helper surfaces for moved integration suites | Prefer `#[doc(hidden)] pub` on the smallest needed helper fn/const instead of keeping `#[cfg(test)]` visibility tied to lib-side sidecar tests |
| Prefer test accessors over exposed fields | If an integration suite needs scheduler/cache/watchdog internals, add narrow `*_for_test()` accessors instead of making raw fields public |
| Adding a new repo suite | Start from a suite-local `setup_*()` helper; only introduce a shared helper when repetition appears in multiple files |
| Verifying a migration | Test the migration itself explicitly; do not force every repo test to replay the full migration chain |
| Considering `cargo-nextest` tuning | Adjust `src-tauri/.config/nextest.toml` groups/profiles instead of ad hoc command-line concurrency flags |

## Capability Tests

| Situation | Rule |
|---|---|
| Test only needs to verify decision logic around sockets/processes/HOME paths | Extract a seam and keep the default suite on a fake probe/controller/temp workspace path |
| Test must bind loopback, kill real processes, or depend on OS-level permissions | Mark it `#[ignore = "requires <capability>"]` or move it to a dedicated capability target |
| Broad default run | Keep `cargo test` / `cargo nextest run` green without requiring those ignored capability tests |
| Capability verification | Run the explicit ignored test or capability target separately in a permissive environment |
| `nextest` use | Only put capability tests in `nextest` groups after they live in dedicated binaries; ignored lib tests stay on explicit `cargo test -- --ignored` commands |

Capability examples:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features test-utils 'tests::lib_shutdown_tests::test_wait_for_backend_ready_real_socket_returns_200' --lib -- --ignored
cargo test --manifest-path src-tauri/Cargo.toml --features test-utils 'domain::services::running_agent_registry::tests::test_kill_process_immediate_kills_process_group_children' --lib -- --ignored
```

## Adding Tests Framework

| Question | Decision |
|---|---|
| Is this pure logic with no DB/git/process/AppState setup? | Keep it in `src-tauri/src/**` as a normal `--lib` test |
| Does it need real SQLite schema/repositories? | Start with `SqliteTestDb` / `SqliteStateFixture` |
| Does it mostly exercise handlers, orchestration, state machines, worktrees, or large service flows? | Put it in an existing `src-tauri/tests/suite_*/` module; do not create a new top-level test binary |
| Did you move a suite out of `--lib`? | Import through `ralphx_lib::*`, not `super::*` / `crate::*` |
| Does the moved suite need internals? | Expose the smallest seam: re-export, `#[doc(hidden)] pub`, or `*_for_test()` |
| Does it only need one small test helper from a private module? | Localize that helper in the integration target instead of exporting a broad test-only helper tree |
| Are you repeating a setup graph twice? | Extract a suite helper now; promote to `src-tauri/src/testing/` once a second file needs it |
| Do multiple integration targets need the same non-production helper? | Promote it into `src-tauri/tests/support/` rather than duplicating it or exporting it from production code |
| Are you validating several targeted suites? | Run them sequentially; do not launch parallel Cargo jobs against the same target dir |
| Do you need local wall-clock speed instead of target-dir reuse? | Use `scripts/test-rust-fast.sh lib-parallel` / `pr-parallel` instead of ad hoc backgrounded Cargo commands |

## Move Decision Framework

| Question | If yes | If no |
|---|---|---|
| Is the suite large enough to materially bloat `--lib` compile scope? | Prefer moving it into an existing `src-tauri/tests/suite_*/` module | Keep it in `--lib` |
| Does the suite mostly exercise public behavior or explicit internal helpers? | Move it | Keep it local if it only probes private implementation details |
| Does the suite rely on SQLite migrations or real git/process setup? | Move it and give it a dedicated integration target | Keep pure logic tests in `--lib` |
| Can the suite work with `ralphx_lib::*` imports plus a few narrow helper exports? | Move it | Do not widen large surfaces just to move it |
| Would moving require exposing raw mutable fields or broad internal modules? | Add narrow `#[doc(hidden)] pub` helpers or `*_for_test()` accessors first | If that still needs broad exposure, leave the suite in place |

| Preferred seam | Use when |
|---|---|
| Re-export existing public helper from module root | The helper is already stable and test-appropriate |
| `#[doc(hidden)] pub` free function/const | Integration test needs one narrow private helper |
| `*_for_test()` accessor | Integration test needs to observe internal state without exposing fields |
| Keep suite in `--lib` | The only alternative is broad visibility churn or leaking implementation-only APIs |

## Ongoing Tuning

| Improvement | Why |
|---|---|
| Tune `cargo-nextest` groups/profiles as suites grow | Better concurrency control, retries, partitioning, and resource grouping for thousands of tests |
| Add shared seed helpers for common row graphs | Removes repeated SQL and makes suite setup cheaper to maintain |
| Group resource-sensitive tests explicitly | Prevent DB/file/git-heavy tests from competing with fast unit coverage |
| Extract pure backend modules into workspace crates | Reduces root-crate scope over time; start with low-dependency clusters before touching Tauri/SQLite-facing code |

## Formatter Warning

| Situation | Action |
|---|---|
| Need to change Rust code | Edit the smallest surface possible |
| Think "`cargo fmt` will be harmless" | Don’t do it here |
| Formatting is truly required | Ask first, keep it scoped, and commit it separately from logic changes |
