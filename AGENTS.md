> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# AGENTS.md

## Project
RalphX — native Mac GUI for autonomous AI development with Rust/Tauri backend and React frontend.

Primary project docs:
- `CLAUDE.md`
- `frontend/CLAUDE.md`
- `src-tauri/CLAUDE.md`
- `.claude/rules/*.md`
- `.claude/rules/openai-gpt-5-prompting.md` for target-model prompt-guide routing across supported GPT-5 families
- `docs/ai-docs/openai/README.md` for GPT-5.4, GPT-5.5, and GPT-5.6 family guides plus official-source links
- `.claude/rules/ideation-verification-architecture.md` for the ideation verification feature map: parent-vs-child ownership, runtime flow, UI surfaces, debugging, and tests
- `.claude/rules/delegation-topology.md` for canonical delegation allowlists, auto-injected delegation guidance, and MCP visibility/enforcement rules
- `.claude/rules/runtime-root-vs-target-project.md` for the contract between RalphX-owned runtime/plugin/log roots and the user’s active target project checkout
- `.claude/rules/production-cli-resolution.md` for Finder/Homebrew-safe CLI binary resolution in installed app runtime paths
- `.claude/rules/codeql-path-safety.md` for CodeQL-safe filesystem sink validation when paths are influenced by env vars, settings, HTTP/MCP payloads, DB state, agent metadata, or repo contents
- `.claude/rules/multi-harness.md` for provider-neutral runtime/config/event rules and documentation sync requirements
- `.claude/rules/agent-thinking-capture.md` and `docs/architecture/agent-thinking-capture.md` for cross-harness reasoning capture, capability probing, event/persistence ownership, failure edges, and focused proofs
- `.claude/rules/agent-mcp-tools.md` for multi-layer agent MCP/tool alignment across canonical agent metadata, harness runtime config, prompt contracts, and MCP registration
- `.claude/rules/agent-workspace-review-modes.md` for the non-interchangeable local Workspace Review gate and remote GitHub Review PR workflow
- `.claude/rules/merge-recovery-consistency.md` for the coupled merge-failure behavior across merge outcome handling, manual retry, reconciliation, startup recovery, and MergeIncomplete UI
- `.claude/rules/stateful-workflow-review.md` for false-success review of completion/cache/retry/recovery/state-machine changes
- `.claude/rules/task-state-machine.md` for the 28 internal task statuses, the transition table, and the validated transition API contract
- `.claude/rules/big-pr-review-checklist.md` for the recurring failure classes big PRs ship here and the 12 falsifiable pre-merge checks that catch them
- `.claude/rules/rust-test-execution.md` for selective Rust test commands, the standard Rust test stack, shared SQLite fixtures/builders, and the no-broad-`fmt` rule
- `.claude/rules/wkwebview-css-vars.md` for Tauri (WKWebView) CSS custom-property inheritance rules — theme tokens for bg/text/border MUST be literals, not chained `var()` references
- `.claude/rules/release-script-validation.md` for safe validation of release proposal/wrapper scripts without triggering real publish steps
- `.claude/rules/icon-only-buttons.md` for accessible tooltip requirements on icon-only controls
- `.claude/rules/frontend-interaction-performance.md` for non-negotiable lazy loading, first-paint-safe UI transitions, deferred hydration/teardown, and decoupled panel/drawer interactions
- `.claude/rules/visual-testing.md` for Playwright-first visual QA, scoped dev-server use, and the explicit-request-only Computer Use boundary
- `.claude/rules/pr-descriptions.md` for reviewer-focused PR bodies: context, impact, decisions, risks; validation logs stay secondary

## Codex Rules

- Read project instructions first: check `CLAUDE.md`, subtree docs, and relevant `.claude/rules/*` before substantial work.
- Pattern alignment first (NON-NEGOTIABLE): before implementing any bug fix or feature, identify the owning backend service / frontend component and its established pattern (subtree CLAUDE.md pattern tables, `.claude/rules/*`, `docs/architecture/`); extend that seam instead of introducing new architecture. A new pattern requires explicit justification in the PR description plus a pattern one-liner in the relevant CLAUDE.md.
- For OpenAI/Codex prompt work, check `.claude/rules/openai-gpt-5-prompting.md`, then load only the local guide matching the configured target model before substantial prompt edits.
- When touching ideation verification, read `.claude/rules/ideation-verification-architecture.md` first.
- When touching plugin/root resolution, canonical agent loading, generated plugin bundles, or runtime log placement, read `.claude/rules/runtime-root-vs-target-project.md` first.
- When touching production subprocess launches or CLI discovery, read `.claude/rules/production-cli-resolution.md` first.
- When touching provider thinking/reasoning flags, native event parsing, `agent:thinking`, thinking persistence, or thinking UI lifecycle, read `.claude/rules/agent-thinking-capture.md` first.
- When touching filesystem sinks or any path influenced by external/runtime state, read `.claude/rules/codeql-path-safety.md` first.
- CodeQL path findings block PRs: tests are scanned too; use process-owned runtime roots, fixed entry lists, pure test builders, and suppress `rust/path-injection` only after containment validation.
- When touching merge failure recovery, merge retry/resolve actions, merge reconciliation, or startup merge remediation, read `.claude/rules/merge-recovery-consistency.md` first.
- When touching completion gates, validation caches, retries, recovery, state-machine transitions, or execution prompts, read `.claude/rules/stateful-workflow-review.md` first and run a false-success review before handoff.
- When changing task workflow status, use validated `TaskTransitionService` paths per `.claude/rules/task-state-machine.md`; never write `internal_status` directly outside canonical engine paths.
- Model-facing MCP tool schemas must never accept run/orchestration IDs; identity is injected from transport/runtime context and validated backend-side.
- Before finishing any large feature/refactor PR, run `.claude/rules/big-pr-review-checklist.md` against your own diff — especially the scope-leak, stale-metadata, recovery-parity, and single-writer checks.
- When touching release automation, read `.claude/rules/release-script-validation.md` first.
- Preserve user work: never revert unrelated edits; isolate your diffs in a dirty tree.
- PR branch freshness (NON-NEGOTIABLE): before opening, updating, or handing off a PR, fetch the base branch, rebase onto the latest `origin/<base>`, and push the rebased branch so GitHub does not show it as behind.
- Existing PR fixes (NON-NEGOTIABLE): when patching an open PR, land the fix on that PR branch, then rebase it onto current `origin/<base>` before asking for checks or review.
- PR/commit naming (NON-NEGOTIABLE): do not prefix PR titles or commit subjects with `[codex]`.
- PR descriptions: follow `.claude/rules/pr-descriptions.md`; lead with context, user impact, decisions, and risks instead of local validation transcripts.
- Legacy harness compatibility (NON-NEGOTIABLE): provider-neutral changes stay additive/derivable from legacy Claude-only persisted data until an explicit migration removes that requirement.
- Minimal diffs: avoid formatter churn and opportunistic refactors.
- Context-preservation trackers (NON-NEGOTIABLE): for multi-step investigations or fixes, create and keep updating a local tracker under `.artifacts/specs/<topic>/tracker.md` so findings and decisions survive context compaction.
- Tracker Git probes (NON-NEGOTIABLE): `.artifacts/specs/**/tracker.md` is ignored local state; create parent dirs/files as needed, and never pass tracker paths as `--ignored=<path>`. Use `git status --short -- <path>` for status, `git check-ignore -v -- <path> || true` for ignore diagnostics, or `git status --short --ignored=matching -- <path>` when ignored status output is required.
- Agent tool alignment: keep canonical agent capabilities, harness runtime config, live prompt contracts, and MCP registration/authorization aligned. Source: `.claude/rules/agent-mcp-tools.md`.
- Prompts are not migration diaries (NON-NEGOTIABLE): prompts are clean contracts for the live tool surface and role; migration notes, forbidden legacy paths, and compatibility ballast belong in backend validation, tests, or docs, not prompt prose.
- Surface-local descriptions only (NON-NEGOTIABLE): tool schemas, recovery hints, and prompt prose must not mention tools that are not on the caller agent’s live tool surface.
- One normal flow (NON-NEGOTIABLE): if backend owns orchestration state, the model must not be asked to replay delegate ids, timestamps, rescue flags, wait knobs, or other persisted bookkeeping.
- Model chooses lenses, backend runs them (NON-NEGOTIABLE): optional specialist selection belongs to the model; backend owns dispatch, waits, settlement, parent resolution, and terminal state.
- Kill transitional vocabulary: when typed findings replace artifact-prefix contracts, remove the old artifact/prefix language from live verifier surfaces instead of narrating both.
- Verification regressions are TDD-first (NON-NEGOTIABLE): reproduce backend, prompt-contract, and UI failures with focused tests before patching production code.
- Stateful workflow regressions are false-success-first (NON-NEGOTIABLE): tests and reviews must attack stale attempts, stale cache, fail-open reads, event ordering, prompt/schema drift, and path sinks before handoff.
- Handler module split: oversized Rust HTTP handlers belong in directory-backed modules, not giant single files.
- Mechanical extraction only (NON-NEGOTIABLE): large moves/splits must use real mechanical extraction (`mv`, `sed`, `awk`, scripts), not hand-copying.
- `apply_patch` is fix-up only (NON-NEGOTIABLE): after a mechanical move, use it only for imports, visibility, wiring, and tests.
- Mechanical split recovery: if an extraction drifts into patch-copying, restore to `HEAD`, clean parked WIP, and redo it mechanically.
- Rustfmt scope safety: never run `rustfmt` on `mod.rs` roots unless recursive formatting is intentional.
- Rustfmt edition safety: prefer `rustfmt --edition 2021 <leaf-file.rs>` for surgical work; avoid plain `cargo fmt` unless broad formatting is intended.
- Cargo during refactors: run one targeted Cargo job at a time.
- Local validation boundary (NON-NEGOTIABLE): agents run only focused tests/checks for touched behavior; never fall back to broad lib/workspace suites, full integration, workspace doctests, dual clippy, llvm-cov, or `scripts/test-rust-fast.sh {pr|main}` unless the user explicitly requests it or a named CI failure must be reproduced. If no exact test exists, use the nearest module/suite/crate check or report no applicable local test. RalphX workspace CI/autofix owns broad validation and remediation.
- Rust test runner split: use `cargo test` for focused lib filters and `cargo nextest run --test <suite> -E ...` for focused integration tests; broad runs are CI/manual-diagnostic only. Source: `.claude/rules/rust-test-execution.md`.
- Post-Rust-test cleanup (NON-NEGOTIABLE): if any Rust test command starts (`cargo test`, `cargo nextest run`, Rust coverage, or a wrapper that executes Rust tests), run `cd src-tauri && cargo clean` separately in the active workspace once after the final or aborted test attempt and before handoff, whether it succeeds, fails, times out, is cancelled, or is interrupted; no Rust test means no cleanup. Report cleanup failure and never manually delete target directories as a fallback.
- Worktree-safe Rust helper: `scripts/test-rust-fast.sh` is for explicit CI reproduction/manual diagnostics from the current checkout; it refuses cross-checkout drift.
- Rust toolchain source of truth: `rust-toolchain.toml` is authoritative.
- Rust PATH mismatch: if Cargo still uses Homebrew `rustc`, run through `rustup run` with `RUSTC=$(rustup which --toolchain 1.91.0 rustc)`; details in `.claude/rules/rust-test-execution.md`.
- Rust std API stability (NON-NEGOTIABLE): do not ship unstable std APIs. Source: `.claude/rules/rust-stable-apis.md`.
- Format what you write (NON-NEGOTIABLE): every Rust file you create or modify must pass `rustfmt --edition 2021 --check <file>` before handoff — format each touched leaf file, never `mod.rs` roots, never broad `cargo fmt`. "Minimal diffs" means no formatter churn on untouched code, not unformatted new code.
- Test file separation (NON-NEGOTIABLE): no `#[cfg(test)] mod tests` inside production `src-tauri` files — tests go in a sibling `<module>_tests.rs`. If you find a pre-existing inline test block, move it out; never extend it.
- Behavioral tests only (NON-NEGOTIABLE): every test drives a production entry path and carries a falsifiable assertion. No getter/no-op/line-execution tests. Guards get both directions: CAS wrong-`from` leaves state untouched; suppressed side effects get absence assertions.
- Rust test stack: root-lib tests need `--features test-utils`; `cargo test` takes ONE name filter; suites run under `cargo nextest`; SQLite repos test on `SqliteTestDb`/`SqliteStateFixture`; scheduler/service tests use memory repos through production tick/entry paths. Source: `.claude/rules/rust-test-execution.md`.
- Frontend tests: Vitest via `cd frontend && npm run test:run -- <files>`; assert user-visible behavior (Testing Library), not implementation internals; strict TS with no `any`; API layer follows the zod snake_case schema → camelCase transform pipeline (`.claude/rules/api-layer.md`).
- Package build hygiene (NON-NEGOTIABLE): use each package's configured scripts (`frontend/`: `npm run ...`; `plugins/app/ralphx-mcp-server`: `npm run build` after any `src/` change — committing without rebuilding dist is a broken commit).
- Current-scope handoff: fix warnings/failures caused by the change and report unrelated pre-existing failures without expanding scope; run `python3 scripts/check-layering.py` locally only for layer/import/module-boundary changes.

## Test Coverage Work

- Patch gate: Codecov requires ≥90% patch coverage (`codecov.yml`); read its path EXCLUSIONS first — lines in excluded composition-root/streaming files never count, so don't spend tests there.
- Measure diff coverage against `origin/main` with `cargo llvm-cov` (lcov output intersected with the diff) and Vitest `--coverage`; two Rust passes max (baseline → write tests from the saved uncovered-line report → confirm).
- llvm-cov disk budget (NON-NEGOTIABLE): `CARGO_INCREMENTAL=0`; delete `*.profraw` after each pass; keep ≥5G free disk or stop and clean `src-tauri/target/llvm-cov-target`; delete `llvm-cov-target` when done.
- Coverage is a byproduct of real tests: close gaps by testing uncovered BEHAVIOR (error arms, guard rejections, round-trips on the concrete repo impl that lacks them), never by executing lines without assertions.
- Worktree safety (NON-NEGOTIABLE): worktree-mode flows must never silently fall back to the main checkout.
- Verify before commit: review `git diff` against `HEAD` for every touched file.
- Frontend visual QA (NON-NEGOTIABLE): prefer automated Playwright visual tests, run them from `frontend/`, and start/stop only the scoped dev servers they require.
- Native Tauri QA through Computer Use is prohibited unless the user explicitly requests it in the current request; never infer permission from UI/theme scope or other repository guidance.
- UI design/theme changes (NON-NEGOTIABLE): use explicit WebKit-safe bg/border longhands for themed surfaces; prefer Playwright visual coverage, and perform Native Tauri/WKWebView QA through Computer Use only when explicitly requested. Source: `.claude/rules/wkwebview-css-vars.md`.
- Icon-only buttons: use an accessible name plus the app tooltip component; native `title` alone is not enough. Source: `.claude/rules/icon-only-buttons.md`.
- Frontend interaction performance (NON-NEGOTIABLE): user-triggered panels/drawers/widgets must paint a lightweight shell before lazy imports, fetches, persistence, process startup, or heavy mount/unmount work; warm up likely heavy paths on safe intent/idle; fix safe current-scope opportunities with TDD. Source: `.claude/rules/frontend-interaction-performance.md`.
- Refactor tracker hygiene: when a turn exposes real architectural debt, update `## High-Value Refactor Targets` in the same slice.
- `.artifacts` tracker hygiene (NON-NEGOTIABLE): for any multi-step investigation/fix likely to outlive the current context window, create/update `.artifacts/specs/<slug>/tracker.md` as soon as substantive findings appear and keep it current before continuing.
- Turn-level refactor discipline (NON-NEGOTIABLE): if production callsites repeat the same wiring/branching, centralize it or track it before continuing.
- Factory-first runtime wiring: when scheduler/chat/transition assembly repeats in 3+ production callsites, extend a shared builder/factory instead of adding another copy.
- Future harness readiness: prefer provider-neutral registries/factories keyed by `AgentHarnessKind` over one-off `claude+codex` branching when safe.

## Backend

When working in `src-tauri/`, also follow:
- `src-tauri/CLAUDE.md`
- `.claude/rules/multi-harness.md`
- `.claude/rules/rust-stable-apis.md`
- `.claude/rules/rust-test-execution.md`
- `.claude/rules/task-git-branching.md`
- `.claude/rules/code-quality-standards.md`
- `.claude/rules/production-cli-resolution.md`
- `.claude/rules/codeql-path-safety.md`
- `.claude/rules/stateful-workflow-review.md`
- `.claude/rules/agent-mcp-tools.md`

## Active Project Tracking

Volatile optimization, reliability, migration, refactor, and allocation status lives in `docs/governance/active-trackers.md`; keep dated “landed/next” notes out of this always-loaded file.
