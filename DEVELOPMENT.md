# Development Strategy

This is the implementation playbook for RalphX. It exists to keep frontend, backend, agent flows, and git/worktree behavior consistent under human and LLM-driven development.

Read with:
- `CLAUDE.md`
- `frontend/src/CLAUDE.md`
- `src-tauri/CLAUDE.md`
- `docs/development/build-from-source.md`
- `docs/development/coverage.md`
- `.claude/rules/code-quality-standards.md`
- `.claude/rules/api-layer.md`
- `.claude/rules/task-state-machine.md`
- `.claude/rules/task-git-branching.md`
- `docs/architecture/agent-chat-system.md`

## Development Guides

| Guide | Use |
|---|---|
| [Build from source](docs/development/build-from-source.md) | Local checkout setup, native dev start, ports, and runtime prerequisites |
| [Code coverage](docs/development/coverage.md) | Coverage workflow behavior, Codecov flags, suite badges, and covered folders |

## Product Flows We Optimize For

RalphX is not a generic CRUD app. Most changes land inside one of these flows:

1. Ideation session -> plan artifact -> verification loop -> proposals -> active plan
2. Kanban/Graph action -> task transition -> execution/review/merge state machine
3. Chat input -> Claude agent run -> MCP tools -> streamed events -> UI hydration
4. Worktree/local git operations -> validation -> cleanup -> recovery/reconciliation

New code should extend one of these flows. If a change does not clearly belong to one, stop and identify the real owner before writing code.

## North Star

The stable architecture is:

```text
React component
  -> hook
  -> typed API wrapper
  -> Tauri command or HTTP handler
  -> application service
  -> domain/state machine
  -> repository/infrastructure
  -> emitted event / query invalidation
  -> hook/store update
  -> UI re-render
```

Single owner per invariant:

| Invariant | Owner |
|---|---|
| Task lifecycle/status changes | `domain/state_machine/transition_handler` |
| Worktree safety / merge routing | `task-git-branching` flow + merge modules |
| Chat context/type resolution | `src/lib/chat-context-registry.ts` |
| Wire format conversion | `src/api/*.schemas.ts` + `*.transforms.ts` |
| Background agent orchestration | `application/chat_service/*` |
| Persistence and external I/O | repositories + infrastructure implementations |
| Verification state | ideation verification service/handlers |

If two modules can change the same invariant, the design is drifting.

## Non-Negotiables

Core constraints from CLAUDE.md Key Principles apply here. Additionally:
- No new generic catch-all modules (`misc.ts`, `common.rs`, `services.ts`, `manager.ts`).
- No global helper dumping ground. Shared code must have a clear domain owner.

## LLM Preflight Protocol

Before writing code:

1. Name the product flow being changed.
2. Name the current module family that owns that flow.
3. List the invariants that must remain true after the change.
4. Find the proving test boundary before editing code.
5. Check whether the target file is already a hotspot or extraction candidate.

Do not start implementation until these are clear:
- where the change belongs
- which module is the single owner
- which tests will fail without the change

If ownership is unclear or split across multiple modules, fix ownership first or alongside the feature.

## Naming and Module Rules

Name by domain noun plus responsibility.

Good:
- `merge_validation.rs`
- `merge_coordination.rs`
- `chat_service_streaming.rs`
- `chat-context-registry.ts`
- `useVerificationEvents.ts`
- `task_context_service.rs`

Bad:
- `helpers.ts` at project root
- `utils.rs` for feature logic
- `service.ts` with mixed UI/data logic
- `manager.rs` that owns unrelated workflows

Allowed generic names:
- `helpers` only inside a bounded module family and only for local glue
- `types`, `schemas`, `transforms`, `tests`, `registry`, `context` when the file truly matches that role

When to use a specific pattern:

| Smell | Pattern |
|---|---|
| Same routing logic appears in 3+ places | Registry |
| Function takes repeated task/project/branch bundles | Context struct |
| Large module mixes multiple subdomains | Folder module + leaf files |
| UI component owns fetch + mutation + event wiring + dense rendering | Container hook + child components |
| Same data is derived differently in UI and backend | Shared typed transform or shared domain helper |

## Canonical Frontend Pattern

Frontend responsibilities are intentionally split:

| Layer | Owns | Does Not Own |
|---|---|---|
| `components/` | Rendering, composition, local UI interaction | Raw Tauri calls, cross-view business rules |
| `hooks/` | Query/mutation orchestration, event subscription, view-model shaping | Rendering-heavy JSX, backend schemas |
| `api/` | Typed `invoke`, Zod validation, snake_case -> camelCase transforms | UI state, cache orchestration |
| `stores/` | Cross-view client state, optimistic local state, ephemeral UI state | Server truth duplication when React Query is enough |
| `lib/` | Stable primitives, registries, event bus, shared formatters | Feature-specific logic with one caller |

Rules:
- Components should mostly receive already-shaped data and callbacks.
- Hooks are the default home for orchestration.
- Stores are for client coordination, not a second backend.
- React Query owns server-cache lifecycle.
- Follow the API contract: Rust snake_case -> Zod schema snake_case -> transform -> TS camelCase.
- Frontend `invoke()` args stay camelCase unless a command explicitly expects nested snake_case payload data.
- Backend events should update queries/stores through dedicated event hooks, not ad-hoc listeners in random components.
- Use registry files when the UI needs one source of truth for context, widgets, or detail views.

Canonical query flow:

```text
Component -> useXxx hook -> api.xxx -> typedInvokeWithTransform()
-> backend command -> repo/service -> response schema -> transform
-> TanStack Query cache -> component
```

Canonical streaming flow:

```text
Chat UI -> sendAgentMessage
-> ChatService
-> Claude CLI / MCP
-> agent:* events
-> EventBus
-> event hooks
-> query invalidation + store updates
-> chat UI
```

## TypeScript Guardrails

Default frontend rules for new code:

- Components render; hooks orchestrate.
- No raw `invoke()` or backend schema knowledge in components.
- No snake_case outside the API/schema boundary.
- Every backend payload is validated at the edge with Zod before UI use.
- Query keys live in hook/domain factories, not ad-hoc string arrays in components.
- Event names come from shared constants, not inline literals.
- Use a registry when mapping context type, widget type, detail view, status action, or tool type.
- Do not mirror server state in Zustand unless the UI truly needs client-owned coordination or optimistic behavior.
- Avoid `any`; parse `unknown` at the boundary and convert into typed data.
- Optimistic UI must have a reconciliation path via query invalidation or backend events.

## Canonical Backend Pattern

Backend responsibilities are also split on purpose:

| Layer | Owns | Does Not Own |
|---|---|---|
| `commands/` | Thin Tauri IPC wrappers and input/output mapping | Workflow orchestration |
| `http_server/handlers/` | Thin HTTP adapters | Separate business rules from services |
| `application/` | Use-case orchestration, coordination across repos/services/agents | Core domain invariants that must remain framework-agnostic |
| `domain/` | State machine, entities, repository traits, pure services, safety rules | Tauri, SQLite, CLI wiring |
| `infrastructure/` | SQLite, agent clients, GitHub CLI, process integration | Business decisions |

Rules from src-tauri/CLAUDE.md apply. Key additions:
- If both HTTP and Tauri need the same behavior, extract shared application/domain code. Do not fork logic.
- When a change adds async side effects around transitions, keep the decision in domain/state machine and the wiring in application services.

Canonical task transition flow:

```text
UI action
-> command
-> TaskTransitionService
-> TaskStateMachine + TransitionHandler
-> repo/git/chat side effects
-> emitted task/review/agent events
-> frontend event hooks
```

## Rust Guardrails

Backend rules extend src-tauri/CLAUDE.md and root CLAUDE.md Key Principles. Rules below add RalphX-specific guardrails not covered there:

- `commands/` and HTTP handlers stay thin. Parse, delegate, return.
- Domain code must not depend on Tauri, SQLite, CLI, or process primitives.
- No blocking I/O on Tokio worker threads. `DbConnection` handles SQLite; git commands, `std::process::Command`, and filesystem operations in async functions must use `spawn_blocking` or run on a dedicated OS thread. Blocking a Tokio thread stalls agent stream readers, triggers false `team_line_read_secs` timeouts, and deadlocks IPC handlers.
- Avoid `unwrap()` and `expect()` in runtime request, agent, transition, and reconciliation paths. Convert failures into typed errors with context. RalphX is a desktop app — a panic kills the entire process, orphans running agents, and may corrupt in-flight state. `unwrap()`/`expect()` are allowed only in: tests, startup invariants (with informative message), and provably unreachable paths (with a comment explaining why).
- Exhaustive enum matching for critical workflows. In state machine transitions, context type routing, and merge/review pipeline logic, list all variants explicitly. No wildcard `_ =>` arms that silently swallow new states — the 24-state task machine and 6-variant `ChatContextType` rely on compiler exhaustiveness warnings to catch unhandled additions.
- RAII cleanup guards for shared resources. Worktrees, process slots, and merge-in-flight tracking use `Drop`-based guards (`WorktreePermit`, `AutoCompleteGuard`, `InFlightGuard`). New code that acquires shared resources must follow the same pattern — partial failure must still leave the system recoverable.
- Structured tracing for async workflows. Use named fields (`task_id = %id, session_id = %sid`) instead of freeform `format!()` strings. Concurrent agent teams make log correlation by structured field the only reliable debugging path.
- Dual AppState awareness. Tauri commands and the HTTP/MCP server each get their own `AppState` instance. Any `Arc<T>` coordinating between them must be cloned in `lib.rs` setup. See `src-tauri/CLAUDE.md:34` for the full sharing table.

Rust design defaults:

| Problem | Preferred Pattern |
|---|---|
| Infra detail leaking into domain | Trait boundary or application service |
| Async workflow with retries/recovery | Explicit typed state + idempotent handlers |
| External string contract | Named constant + parser/mapper |
| Multi-step DB mutation | Single transaction / single `db.run` closure |
| Acquired shared resource (worktree, process slot, lock) | RAII `Drop` guard struct |
| Blocking I/O in async function | `spawn_blocking` or dedicated OS thread |
| New enum variant added to critical type | Exhaustive match — no wildcard `_` arm |

## Failure Modes to Assume Up Front

When coding in RalphX, assume these bugs exist unless proven otherwise:

| Area | Assume This Can Break | Required Guardrail |
|---|---|---|
| Git/worktree cleanup | TOCTOU, double cleanup, missing paths, stale branches | idempotent cleanup + integration tests |
| Agent orchestration | duplicate runs, stale registry entries, lost events | ownership checks + event tests + reconciliation awareness |
| State transitions | bypassed state machine, wrong auto-transition chain | transition tests at the state-machine boundary |
| Chat/event flows | stale UI, duplicate chunks, missed invalidation | event constants + hook tests + cache reconciliation |
| Recovery/retry logic | partial cleanup, resumed wrong state, repeated side effects | metadata-based routing + replay-safe handlers |
| Shared abstractions | second owner introduced quietly | registry/context extraction or remove duplicate path |

## Placement Matrix

When adding code, place it by ownership, not convenience.

| If the change is mainly about... | Put it in... | Avoid putting it in... |
|---|---|---|
| Rendering and layout | `src/components/*` | hooks, stores, API wrappers |
| Query/mutation orchestration | `src/hooks/*` | components, stores |
| Tauri contract, schema, transforms | `src/api/*` | components, hooks |
| Cross-view client state | `src/stores/*` | API layer |
| Shared frontend context/routing/config | `src/lib/*registry*`, `src/lib/*context*` | random components |
| Thin IPC adapter | `src-tauri/src/commands/*` | domain, large inline logic |
| HTTP adapter | `src-tauri/src/http_server/handlers/*` | duplicated service logic |
| Multi-repo orchestration or process coordination | `src-tauri/src/application/*` | commands, infrastructure |
| Core invariant or state machine rule | `src-tauri/src/domain/*` | Tauri commands |
| SQLite/GitHub/Claude CLI/process implementation | `src-tauri/src/infrastructure/*` | domain |
| Git/worktree helpers tied to transition flow | `transition_handler/*` or `application/git_service/*` | generic utils files |

## Extraction Rules

Extract before the module becomes ambiguous.

| Trigger | Required Action |
|---|---|
| File > 400 lines and still growing | Split now, do not wait for 500 |
| 2+ unrelated responsibilities in one file | Split by domain noun |
| Handler > 10 lines or service method > 50 lines | Extract helper/context-specific function |
| 3+ branches on context/status/tool type | Introduce registry or dedicated router |
| 5+ impl blocks or 30+ functions | Break into submodules |
| One test file > 50 tests | Move to `tests/` submodule family |

Extraction conventions already used successfully in this repo:
- Re-export hubs after splits (`mod.rs` or `index.ts`)
- Leaf modules named by responsibility (`merge_validation`, `chat_service_streaming`)
- Context structs to replace long parameter lists (`TaskCore`, `BranchPair`, `ProjectCtx`)
- Delete moved code in the same change; do not leave duplicates behind

## Guard Rails for LLM-Generated Code

Before changing code, answer these internally:

1. Which of the four product flows am I modifying?
2. Which file or module family already owns that flow?
3. Am I extending an owner, or accidentally creating a second owner?
4. Does this require a registry, context struct, or extraction before behavior work?
5. Which test boundary proves the change?

Default LLM behavior for this repo:
- Extend an existing module family before inventing a new top-level pattern.
- Prefer one more specific file over one more generic file.
- Prefer vertical slices over shared abstractions created "just in case".
- Extract repeated logic only after the second real use, not on speculation.
- If touching worktrees, merges, agent process lifecycle, or reconciliation, assume regressions are easy and add explicit tests.

## Feature Recipes

### Frontend feature

1. Add or update schema/transform/types in `src/api` or `src/types`.
2. Add or update a hook in `src/hooks` for data flow.
3. Add store state only if the feature needs client-owned cross-view coordination.
4. Wire backend events through an event hook if the feature is async/streaming.
5. Keep components focused on rendering and composition.
6. Add tests for transforms, hooks, and UI behavior at the changed seam.

### Backend feature

1. Decide whether the rule belongs in domain, application, or infrastructure.
2. Add/change domain types or repo traits first if the invariant changes.
3. Add/change application service orchestration.
4. Expose via thin command or HTTP handler.
5. Add infra implementation changes last.
6. Add unit and integration tests at the owner boundary.

### Cross-cutting async feature

1. Define the source of truth for status and ownership.
2. Define the emitted events and their typed payloads.
3. Define cache invalidation and store updates on the frontend.
4. Add reconciliation or recovery only once, in the existing recovery layer.

## Current Pressure Points

These files already carry too much responsibility. Do not casually add more logic to them:

- `src/App.tsx`
- `src/components/TaskGraph/TaskGraphView.tsx`
- `src/components/Ideation/PlanningView.tsx`
- `src/components/tasks/detail-views/BasicTaskDetail.tsx`
- `src-tauri/src/commands/execution_commands.rs`
- `src-tauri/src/commands/task_commands/mutation.rs`
- `src-tauri/src/application/task_transition_service.rs`
- `src-tauri/src/application/chat_service/mod.rs`
- `src-tauri/src/domain/state_machine/transition_handler/on_enter_states.rs`
- `src-tauri/src/domain/state_machine/transition_handler/side_effects.rs`

When working in one of these hotspots, extraction is the default, not the exception.

## Testing and Verification

Testing is part of the design, not post-processing.

| Change Type | Minimum Proof |
|---|---|
| TS API/schema transform | schema/transform tests |
| Hook/query/event behavior | hook tests |
| Complex UI behavior | component tests |
| Task state / review / merge path | Rust unit + integration tests |
| Git/worktree/recovery logic | real git integration tests |
| Event or streaming changes | backend event tests + frontend event hook tests |

Required checks — see CLAUDE.md Key Principle #8 and src-tauri/CLAUDE.md Commands section for exact commands.

Do not treat `cargo check` as sufficient validation for backend changes.

## Commit Strategy

- Keep diffs scoped to one architectural decision.
- If a feature requires extraction, do the extraction cleanly before or alongside behavior work, not as hidden incidental churn.
- New files and deletions from a split belong in the same change.
- Review the final diff against `HEAD` and confirm only intended hunks remain.

## Review Checklist

Before marking work complete, confirm:

- The change extends an existing owner instead of creating a second owner.
- No new god file or catch-all helper file was introduced.
- Tauri/API casing is correct: input args camelCase, backend payload schemas snake_case, frontend types camelCase.
- Task lifecycle changes still route through the state machine.
- Event names and payload shapes remain typed and consistent across backend and frontend.
- Worktree/merge/process cleanup changes include explicit regression coverage.
- The diff contains extraction cleanup, not copied-and-left-behind duplicates.

## Definition of Done

Work is not done until all of these are true:

- The changed behavior has one clear owner in the codebase.
- The proof exists at the right test boundary.
- Lint, typecheck, clippy, and the relevant test suites pass.
- Recovery, retry, and duplicate-execution paths were considered for async/process/git changes.
- No new generic helper bucket or god file was created.
- Any new reusable pattern is documented back into repo guidance if it should become standard.

## Appendix: Runtime and Debugging

### Setup and MCP

Prerequisites:

```bash
rustup install stable
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Register the RalphX MCP server with Claude:

```bash
claude mcp add ralphx node plugins/app/ralphx-mcp-server/build/index.js
```

Install MCP server dependencies:

```bash
cd plugins/app/ralphx-mcp-server
npm i
```

The MCP HTTP server starts automatically on the local backend port when the Tauri app initializes: production defaults to `http://127.0.0.1:3847`; source dev defaults to `http://127.0.0.1:3857` so it can run beside the installed app.

### Startup Modes

| Mode | Command | Frontend Port | Backend | Use Case |
|---|---|---|---|---|
| Native | `cd frontend && npm run tauri dev` | 1420 | Real Rust/Tauri on backend `3857` | Full-stack development beside installed app |
| Native (fresh clean) | `./dev-fresh` | 1420 | Real Rust/Tauri on backend `3857` after `cargo clean`, with frontend install self-repair and Tauri version drift checks | Large-artifact fresh starts |
| Web | `cd frontend && npm run dev:web` | 5173 | Mocked via `frontend/src/api-mock/` | UI-only work |

Core commands:

```bash
cd frontend && npm run tauri dev
./dev-fresh
cd frontend && npm run dev:web
```

### Frontend Logging

Use `src/lib/logger.ts` instead of raw `console.*` for app-level debug output:

```typescript
import { logger } from "@/lib/logger";

logger.debug("operation details", data);
logger.log("info message");
logger.warn("non-fatal issue");
logger.error("failure", err);
```

Behavior:

| Method | Dev Mode | Production | Notes |
|---|---|---|---|
| `logger.debug()` | shown | silent | prefixed for filtering |
| `logger.log()` | shown | silent | dev-only info |
| `logger.warn()` | shown | shown | use for recoverable issues |
| `logger.error()` | shown | shown | use for failures |

DevTools filtering:
- `Verbose` shows debug output
- filter by `[debug]` to isolate `logger.debug()`
- use `Cmd+Option+I` on macOS to open DevTools

### Backend Logging

Backend logging uses `tracing` with `RUST_LOG`.

Default:
- `ralphx=info,warn`

Useful logging:

```bash
RUST_LOG=ralphx=debug npm --prefix frontend run tauri dev
RUST_LOG=ralphx::application::chat_service=debug npm --prefix frontend run tauri dev
RUST_LOG=ralphx::domain::state_machine=debug npm --prefix frontend run tauri dev
RUST_LOG=ralphx::application::git_service=debug npm --prefix frontend run tauri dev
RUST_LOG=ralphx::http_server=debug npm --prefix frontend run tauri dev
RUST_LOG="ralphx::application::chat_service=debug,ralphx::domain::state_machine=debug,ralphx::http_server=warn" npm --prefix frontend run tauri dev
```

Quick reference:

| Pattern | Effect |
|---|---|
| `ralphx=debug` | all RalphX modules at debug |
| `ralphx=trace` | maximum RalphX verbosity |
| `warn` | warnings and errors only |
| `ralphx::application::chat_service=debug` | chat service only |
| `ralphx=info,ralphx::application::git_service=debug` | normal logs plus git detail |
| `ralphx=debug,ralphx::http_server=warn` | broad debug with quieter HTTP |

Log locations:
- Dev logs: `.artifacts/logs/ralphx_YYYY-MM-DD_HH-MM-SS.log`
- Prod logs: `~/Library/Application Support/com.ralphx.app/logs/`
- Stream debug logs: `/tmp/ralphx-stream-debug-{conversation_id}.log`

### Common Debugging Workflows

Chat/streaming issues:

```bash
RUST_LOG=ralphx::application::chat_service=debug npm --prefix frontend run tauri dev
```

State machine transitions:

```bash
RUST_LOG=ralphx::domain::state_machine=debug npm --prefix frontend run tauri dev
```

Git/worktree/merge issues:

```bash
RUST_LOG=ralphx::application::git_service=debug npm --prefix frontend run tauri dev
```

Startup or migration issues:

```bash
RUST_LOG=ralphx=debug npm --prefix frontend run tauri dev
```

MCP HTTP handler issues:

```bash
RUST_LOG=ralphx::http_server=debug npm --prefix frontend run tauri dev
```

What to check during startup failures:
- database migration errors
- HTTP server bind retries/failures on the active backend port (`:3857` for source dev, `:3847` for production)
- reconciliation and recovery logs
- missing local env overrides

### Key Backend Module Map

| Module | Covers |
|---|---|
| `ralphx::application::chat_service` | agent streaming, conversations, queueing, recovery |
| `ralphx::application::git_service` | branch, merge, rebase, worktree operations |
| `ralphx::application::task_scheduler_service` | task scheduling and concurrency |
| `ralphx::application::task_transition_service` | transition orchestration |
| `ralphx::domain::state_machine` | task state machine and transition side effects |
| `ralphx::http_server` | MCP proxy handlers and HTTP APIs |
| `ralphx::commands` | Tauri IPC layer |
| `ralphx::infrastructure` | SQLite, Claude client, GitHub/process integration |
