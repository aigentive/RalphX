> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# CLAUDE.md

## Priority Zero — Owner Strategy Alignment (NON-NEGOTIABLE)

Before ANY user-facing content, documentation, UI copy, or messaging work, agents must probe these optional files with non-failing file checks and load any that exist:
- `~/.ralphx/founder/founder-profile.md` — Owner vision and non-negotiables
- `~/.ralphx/strategy/project-goal-card.md` — Messaging architecture, positioning, ICPs, competitive landscape
- `~/.ralphx/strategy/project-metrics.md` — Verifiable project data points

If one is missing, skip it and continue; do not fail work or run bare `sed`/`cat` commands that turn absence into a task error. Present files are the **owner's directives** and override default agent judgment on messaging. Do not keep them as always-on `@` imports in project memory.

---

## Project: RalphX
Native Mac GUI for autonomous AI dev: Kanban, multi-agent orchestration, ideation chat.
Code quality: `.claude/rules/code-quality-standards.md` | State machine: `.claude/rules/task-state-machine.md` | Stateful workflow review: `.claude/rules/stateful-workflow-review.md` | Git/merge: `.claude/rules/task-git-branching.md` | Merge recovery consistency: `.claude/rules/merge-recovery-consistency.md` | Agents: `.claude/rules/task-execution-agents.md` | Delegation topology: `.claude/rules/delegation-topology.md` | Runtime roots: `.claude/rules/runtime-root-vs-target-project.md` | Production CLI resolution: `.claude/rules/production-cli-resolution.md` | CodeQL path safety: `.claude/rules/codeql-path-safety.md` | Ideation verification architecture: `.claude/rules/ideation-verification-architecture.md` | Follow-up blocker dedupe: `.claude/rules/followup-blocker-dedupe.md` | Agent type map: `.claude/rules/agent-type-map.md` | Task detail views: `.claude/rules/task-detail-views.md` | Frontend interaction performance: `.claude/rules/frontend-interaction-performance.md` | Icon-only buttons: `.claude/rules/icon-only-buttons.md` | Rust API safety: `.claude/rules/rust-stable-apis.md` | Rust test execution: `.claude/rules/rust-test-execution.md` | WKWebView CSS vars: `.claude/rules/wkwebview-css-vars.md` | Release script validation: `.claude/rules/release-script-validation.md` | PR descriptions: `.claude/rules/pr-descriptions.md`
CodeQL path safety applies to production and tests; use process-owned runtime roots, fixed entry lists, pure test builders, and suppress `rust/path-injection` only after containment validation.
Production CLI resolution applies to installed app launches; all runtime subprocess binaries must go through the shared resolver surface.

## Structure
```
ralphx/
├─ frontend/              # Frontend project root (Vite/React) → frontend/CLAUDE.md → frontend/src/CLAUDE.md
│  ├─ src/                # Frontend app code
│  └─ tests/              # Frontend/Vitest/Playwright tests
├─ src-tauri/             # Backend (Rust/Tauri) → src-tauri/CLAUDE.md
│  ├─ src/http_server/    # Axum backend for MCP adapters (prod :3847 | dev :3857 | RALPHX_BACKEND_PORT)
│  └─ ralphx.db           # SQLite (dev)
├─ agents/                # Canonical agent metadata + harness-specific prompts
├─ config/harnesses/      # Harness-global settings and lane defaults
├─ plugins/app/           # Claude plugin plus MCP adapters
│  ├─ ralphx-mcp-server/  # Internal agent MCP (stdio → :3847)
│  └─ ralphx-external-mcp/# External API MCP (HTTP :3848 → :3847)
```

## Context And Delegation

| Rule | Detail |
|---|---|
| Load narrowly | Read project instructions plus files relevant to the current scope; use bounded delegation when the live harness exposes it and parallel work is genuinely independent. |
| Follow the live profile | Agent conversation Plan profile is read-only; implementation profiles may edit. Do not infer Claude Team/Task behavior on Codex or native delegation paths. |
| Canonical non-team topology | `agents/<agent>/agent.yaml` `delegation.allowed_targets` owns RalphX-native delegation; see `.claude/rules/delegation-topology.md`. |
| Claude team mode is optional | Apply TeamCreate/Task coordination rules only when that Claude team surface is actually active; architecture: `docs/architecture/agent-teams-system-card.md`. |
| Preserve context | Keep durable global invariants here; volatile status belongs in trackers and specialized behavior belongs in path-scoped rules or agent prompts. |

## MCP Architecture
Two MCP servers — different audiences. Full disambiguation: `.claude/rules/mcp-servers.md`
```
Internal: RalphX harness → internal MCP adapter → HTTP :3847 → Tauri Backend
External: Third-party bot → Bearer token → ralphx-external-mcp (:3848) → HTTP :3847 → Tauri Backend
```
Claude plugin: `plugins/app/` | Canonical agent capabilities: `agents/<agent>/agent.yaml` | Tool rules: `.claude/rules/agent-mcp-tools.md`
**MCP server build (NON-NEGOTIABLE):** After modifying ANY source in `plugins/app/ralphx-mcp-server/src/` or `plugins/app/ralphx-external-mcp/src/`, rebuild the respective server. ❌ Committing without rebuilding.
**MCP capability ownership (NON-NEGOTIABLE):** canonical per-agent grants live in `agents/<agent>/agent.yaml` `capabilities.mcp_tools`; `config/ralphx.yaml` holds only explicitly documented compatibility rows.
**Agent frontmatter tool fields (NON-NEGOTIABLE):** Only `tools` and `disallowedTools` are valid in agent `.md` frontmatter. ❌ `allowedTools` — silently ignored by Claude Code. Add MCP tools (e.g., `"mcp__ralphx__*"`) to the `tools` list. Note: `--allowedTools` IS valid as a CLI flag at spawn time — only invalid as frontmatter.

## Key Principles

| # | Rule |
|---|------|
| 1 | TDD mandatory — tests FIRST |
| 1.5 | **Orchestration chain tests** — see `src-tauri/CLAUDE.md` Integration Tests section |
| 2 | Anti-AI-slop — see Design System section |
| 3 | Clean architecture — domain has no infra deps |
| 4 | Type safety — strict TS, newtype IDs in Rust |
| 5 | ❌ Fragile string comparisons — use enum variants (`matches!(err, MyError::Variant)`), error codes, or named constants for external strings |
| 6 | Full timestamps in activity log |
| 7 | Live workflow status changes → validated `TaskTransitionService::transition_task*` or canonical state-machine/merge-engine writes only. ❌ Direct repo/DB `internal_status` mutation. Nonstandard repair jumps → explicit `transition_task_corrective()` / `apply_corrective_transition()` only |
| 8 | **Zero lint/test warnings (NON-NEGOTIABLE):** Fix ALL lint warnings and test failures before completing work — including pre-existing ones. ❌ "It's pre-existing" is not an excuse. Stale warnings delay future work and compound. `src-tauri/` → dual cargo clippy gates \| fast local Rust CI parity (worktree-safe) → `scripts/test-rust-fast.sh pr` / `main` (includes layering; `main` adds workspace doctests + full integration) \| broad root-lib runs during PR 0.x decoupling → `cargo nextest run --manifest-path src-tauri/Cargo.toml --lib --profile ci --features test-utils` \| doctests → `cargo test --manifest-path src-tauri/Cargo.toml --workspace --doc` \| layering → `python3 scripts/check-layering.py` \| pinpoint Rust runs → see `.claude/rules/rust-test-execution.md`. ❌ `cargo check` \| ❌ full `cargo test` (hang) |
| 9 | ❌ Start/stop dev server — user manages manually |
| 10 | Implementation playbook: `DEVELOPMENT.md` — read alongside CLAUDE.md files for placement, naming, recipes, and debugging. |
| 11 | New pattern → add one-liner to relevant CLAUDE.md. Pattern name + rule only. |
| 12 | Complex work → use the task/ledger surface exposed by the active harness; Claude Task management details → `.claude/rules/task-management.md` |
| 13 | Parallel commits → coordinate via normal git hygiene and verify `git status` / `git diff` before committing; no lock-file protocol |
| 14 | Tauri invoke: camelCase fields. ✅ `contextId` ❌ `context_id` |
| 15 | New `.claude/rules/*.md` \| `**/CLAUDE.md` → include this maintainer note at top |
| 16 | **DbConnection (NON-NEGOTIABLE):** All SQLite repo methods MUST use `db.run(\|conn\| { ... })` via `DbConnection` for non-blocking access. ❌ Direct `conn.lock().await` / `conn.query_row()` in async methods. See `db_connection.rs`. |
| 17 | **Tokio spawn safety (NON-NEGOTIABLE):** `tokio::spawn` / `tokio::task::spawn` / `spawn_blocking` → async context ONLY. Sync constructors & Tauri setup → `std::thread::spawn` or `tauri::async_runtime::spawn`. Details: `.claude/rules/tokio-runtime-safety.md` |
| 18 | **Rust std API stability (NON-NEGOTIABLE):** Avoid unstable std APIs in production code (e.g., `is_multiple_of`). Use stable equivalents (e.g., `%`). Details: `.claude/rules/rust-stable-apis.md` |
| 19 | **UI design native parity (NON-NEGOTIABLE):** Theme/layout changes → verify native Tauri/WKWebView, not Chromium only; themed surfaces use explicit bg/border longhands. Details: `.claude/rules/wkwebview-css-vars.md` |
| 20 | **Constraint bundle planning** — Ideation plans should derive repo-specific `Constraints`, `Avoid`, and `Proof Obligations` from explored architecture before verification. |
| 21 | **Mechanical extractions only (NON-NEGOTIABLE):** For large refactors/splits, move existing code with real extraction commands/scripts first (`mv`, `sed`, `awk`, scripted extraction). `apply_patch` is only for the small post-move fix-up layer, never for hand-recreating large existing bodies. Details: `.claude/rules/code-quality-standards.md` |
| 22 | **WKWebView CSS vars (NON-NEGOTIABLE):** Theme tokens for bg/text/border MUST use literal color values (`#rrggbb`, `hsl()`, `hsla()`) — ❌ chained `var(--primitive)`. WKWebView drops chained var() on inheritance. Every new `[data-theme="X"]` block needs a defensive `html[data-theme="X"]` canvas paint rule. Verify in `npm run tauri dev`, not just `dev:web`. Details: `.claude/rules/wkwebview-css-vars.md` |
| 23 | **Icon-only buttons:** Must have an accessible name and the app tooltip component; native `title` alone is not enough. Details: `.claude/rules/icon-only-buttons.md` |
| 24 | **Frontend interaction performance (NON-NEGOTIABLE):** User-triggered panes/drawers/widgets must paint a lightweight shell before lazy imports, fetches, persistence, process startup, or heavy mount/unmount work; warm up likely heavy paths on safe intent/idle; fix safe current-scope opportunities with TDD. Details: `.claude/rules/frontend-interaction-performance.md` |
| 25 | **Stateful workflow review (NON-NEGOTIABLE):** For completion/cache/retry/recovery/state-machine changes, prove current-attempt authority, fail-closed reads, event ordering, prompt/schema alignment, path containment, and production-path tests. Details: `.claude/rules/stateful-workflow-review.md` |

## Adversarial Plan Convergence (NON-NEGOTIABLE)

Plan-verification run/round/gap lineage and terminal classification are backend-owned; the active model may choose permitted lenses/delegates but must not replay orchestration bookkeeping. Follow `.claude/rules/ideation-verification-architecture.md` and the live profile prompt; implementation still requires the product's user-confirmation gate.

## Design System
`specs/design/styleguide.md` (tokens, components, layout rules — initial spec, grows with app) | `specs/DESIGN.md` | Accent: `#ff6b35` (warm orange) ❌ purple/blue | Font: SF Pro ❌ Inter | **INVOKE `/tailwind-v4-shadcn` before UI work**

Input outline removal:
```tsx
className="outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
style={{ boxShadow: "none", outline: "none" }}
```

## Key Features
- **Active Plan** — Project-scoped plan filtering for Graph/Kanban. Docs: `docs/features/active-plan.md` | `docs/architecture/active-plan-api.md`
- **Session Recovery** — Expired Claude session recovery with history preservation. Docs: `docs/features/session-recovery.md`
- **Plan Verification** — Automated adversarial review loop for ideation plans. Docs: `docs/features/plan-verification.md` | Architecture: `.claude/rules/ideation-verification-architecture.md`
- **Agent Personas** — Conversation-bound prompt-only behavior profiles for Project Agent conversations. Docs: `docs/features/agent-personas.md`

## Git Conventions
❌ git init/push/remotes | Prefixes: `docs:` | `feat:` | `fix:` | `chore:`

## Misc
- DB: `sqlite3 src-tauri/ralphx.db "SELECT * FROM table_name;"`
- App logs: per-launch file — dev: `.artifacts/logs/ralphx_YYYY-MM-DD_HH-MM-SS.log` | prod: `~/Library/Application Support/com.ralphx.app/logs/` | latest: `ls -t .artifacts/logs/*.log | head -1` | config: `file_logging` in `config/ralphx.yaml` / `RALPHX_FILE_LOGGING` env (default: true)
- Debug logs: `scripts/find-debug-logs.sh -a "<agent-name>" -d "YYYY-MM-DD" -v` — find Claude debug logs by agent name/date/keywords
- Slash commands: `/activate-prd <path>` — switch PRD | `/create-prd` — PRD wizard
- Claude integration docs: `docs/ai-docs/claude-code/README.md` — lightweight local index plus official-doc stubs; fetch official Claude Code docs when current vendor behavior matters
- OpenAI GPT-5 prompting notes: `docs/ai-docs/openai/README.md` — route by configured target model; do not apply one model's guide wholesale to another family
