> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# frontend/src/CLAUDE.md — Frontend

Quality standards: @../../.claude/rules/code-quality-standards.md
Task detail views: @../../.claude/rules/task-detail-views.md
Interaction performance: @../../.claude/rules/frontend-interaction-performance.md

## Stack
React 19.2 | TS 6.0 | Zustand 5.0+immer | TanStack Query 5.100 | Tailwind 4.1 | Zod 4.4
dnd-kit 6.3 | Vite 8.0 | Vitest 4.1 | Testing Library 16.3 | Tauri API 2.x

## Key Directories
```
src/
├─ api/           # Tauri wrappers
├─ components/    # UI (Chat/, tasks/, TaskGraph/, Ideation/, ui/)
├─ hooks/         # TanStack Query + custom
├─ lib/           # tauri.ts (typedInvoke), queryClient.ts
├─ stores/        # Zustand+immer
├─ styles/        # globals.css (@theme inline)
├─ test/          # setup.ts (Tauri mocks)
└─ types/         # Zod schemas
```

## Patterns

### Zustand + Immer
```typescript
const useTaskStore = create<State & Actions>()(immer((set) => ({
  tasks: {},  // Record<id, Task> for O(1)
  updateTask: (id, changes) => set(s => { Object.assign(s.tasks[id], changes) })
})));
export const selectByStatus = (status) => (s) => Object.values(s.tasks).filter(...)
```

### TanStack Query Keys
```typescript
const taskKeys = { all:["tasks"], list:(pid)=>[...taskKeys.all,"list",pid], detail:(tid)=>[...taskKeys.all,"detail",tid] }
```

### Typed Tauri + Zod
```typescript
async function typedInvoke<T>(cmd, args, schema: z.ZodType<T>): Promise<T> {
  return schema.parse(await invoke(cmd, args))
}
```

### Types via Zod
```typescript
const TaskSchema = z.object({ id:z.string().min(1), ... })
type Task = z.infer<typeof TaskSchema>
```

### Component Organization
```
Component/
├─ index.tsx, Component.tsx, Component.test.tsx
├─ ChildComponent.tsx, hooks.ts
└─ *.test.tsx (co-located)
```

### Event-Driven Updates
EventProvider wraps app with hooks: `useTaskEvents()`, `useSupervisorAlerts()`, `useReviewEvents()`

### Path Aliases
`import { Task } from "@/types/task"` — configured in vite.config.ts + tsconfig.json

## Rules

### API Layer Patterns
See api-layer.md for Tauri conventions, schemas, transforms, and mocking.
- **Tauri invoke args use camelCase** — Rust structs use `#[serde(rename_all = "camelCase")]`, so `invoke()` calls must pass `contextId` NOT `context_id`

### TS Config (strict)
```json
{ "strict":true, "noUncheckedIndexedAccess":true, "noImplicitReturns":true, "exactOptionalPropertyTypes":true }
```
Conditional props: `{ required: val, ...(optional !== undefined && { optional }) }`

### Tailwind v4 Config
- NO tailwind.config.js (ignored)
- NO tailwindcss-animate (deprecated)
- `@tailwindcss/vite` in vite.config.ts
- `@theme inline` in globals.css
- `"config":""` in components.json

### CSS Variables
```css
:root { --bg-base:hsl(0 0% 6%); --accent-primary:hsl(14 100% 60%); }
@theme inline { --color-bg-base:var(--bg-base); --color-accent-primary:var(--accent-primary); }
```
Tokens: bg-base|surface|elevated | text-primary|secondary|muted | accent-primary|secondary

### Anti-AI-Slop
NO purple gradients | NO Inter font | Warm orange #ff6b35

## Code Quality

### Quality Scope
Keep work inside the requested feature/refactor/polish scope; file limits, migration rules, and quality targets live in `../../.claude/rules/code-quality-standards.md`.

### Zero Warnings Policy (NON-NEGOTIABLE)
Fix lint/typecheck/test failures caused by the current change; report unrelated pre-existing failures without expanding scope. Run `npm run lint` and `npm run typecheck` when the changed frontend surface requires them.

### File Size Limits
**See:** `../../.claude/rules/code-quality-standards.md` (single source of truth)

Quick reference: Component 500 max (refactor at 400), Hook 300 max, Presentational 200 max.

### Single Responsibility
Component does ONE of: Display UI | Manage State | Coordinate children

### Document Patterns Inline
When introducing a new architectural pattern, add a one-liner here. Pattern name + rule only.
Example: "View Registry Pattern" — see @../../.claude/rules/task-detail-views.md

- **Reuse Before Invent (NON-NEGOTIABLE)** — new chat/agents behavior extends the existing owning surface: context derivation → chat-context-registry, send/queue/stop → `useChatActions`, streaming → `useChatEvents`, hydration → `useChatRecovery`, scrolling → `ChatScrollController`, per-conversation state → the conversation-keyed stores. ❌ Parallel stores/hooks/scroll writers for owned concerns.
- **Chat Context Registry** — `src/lib/chat-context-registry.ts`. Use `buildStoreKey()`, `resolveContextType()`, `getContextConfig()` for all chat context derivations. New context type = add to registry + `CONTEXT_TYPE_VALUES`.
- **Unified Chat Hooks** — `useChatActions` (send/queue/stop), `useChatEvents` (streaming/tool calls), `useChatRecovery` (polling/sync). Both panels use these.
- **First-Paint Shells** — heavy panes/drawers/widgets render a lightweight shell immediately, then lazy-load/hydrate content after paint. See @../../.claude/rules/frontend-interaction-performance.md
- **Backend-owned Startup Readiness** — `StartupRoot` polls the typed startup snapshot and is the only frontend mount gate; time, localStorage, and root-query settlement never authorize the real App.
- **Provider MCP Settings** — Harness → MCP uses refreshed enabled/available provider readiness, provider-scoped query keys, redacted catalogs, and global/project tri-state deny controls; provider definitions/auth/trust never enter frontend state.
- **Async Confirmations** — pass backend work through `useConfirmation({ onConfirm, pendingText })` so dialogs stay open with disabled actions until settlement.
- **Persistent Operation Toasts** — long-running confirmed publish/update operations may close the dialog after intent is captured and keep one stable-id Sonner loading toast with title separate from conversation/detail/elapsed metadata until terminal success/error.
- **Single Scroll Authority** — ALL chat scroll writes go through `ChatScrollController` (`src/components/Chat/scroll/controller.ts`, `pinned`/`free`/`returning` FSM). ❌ Independent `scrollTop`/`scrollIntoView` in chat surfaces. Only explicit user intent enters `free`; programmatic growth never re-enables follow.
- **Conversation-Keyed Live State** — drafts, attachments, artifact-tab state, review context, and publish state are keyed by conversation id; switching conversations must not leak another conversation's state. Defaults/start-composer may use broader project/provider scopes.
- **Review-Mode Boundary** — Workspace Review displays the local publish gate; Review PR displays linked remote GitHub head/lifecycle/action state and suppresses stale actions on terminal durable state. See `../../.claude/rules/agent-workspace-review-modes.md`.
- **Timeline Canonical, Live Supplementary** — persisted timeline pages are the transcript authority (legacy logical history only when no page exists); keep live streamed output visible until the matching persisted message arrives, then release the live duplicate. An incomplete live tail never replaces full persisted history.
- **Stale-Event Rejection** — chat event handlers validate payloads and reject terminations/updates keyed by BOTH conversation and active run identity, not conversation alone.
- **Shared Persona Menu** — `src/components/personas/PersonaMenuList.tsx` is the single writer for persona choose-menus (picker + chip); it owns the scoped `globalAndProject` query, grouping, and inspect preview. ❌ New flat/unscoped persona lists.
- **StatusPill** — `src/components/ui/status-pill.tsx` is the single pill surface for status/stage/judge badges (tone-based, WKWebView-safe longhands). ❌ New ad-hoc `rounded-full px-2 py-0.5` status spans; automation run-card badge dedupe lives in `automations/automationRunBadges.ts`.
- **Plan Bundle Tabs** — Agents Plan uses `PlanBundleTabs` for persistent Overview/Blueprint selection and conditional Proposals; lifecycle controls remain bundle-level while edit/history/export operate on the selected document.
- **Remote Connection Journal** — `stores/remoteConnectionJournalStore.ts` is the per-environment connection diagnostics ring buffer; `lib/remote/environment-runtime.ts` is its single writer, the banner Details dialog its reader. Remote HTTP reads must lift the host's `REMOTE_COMMAND_UNAVAILABLE` envelope into `RemoteTransportError` (capability boundary, tolerated by the hydration barrier) — ❌ flattening it into generic HTTP errors.
- **Syncing Presentation** — `lib/remote/supervisor-presentation.ts` owns the `syncing` projection (live socket mid-hydration → chip-only accent pulse in `EnvironmentSwitcher`, no banner) with K=2 barrier-failure / T=12s one-way escalation back to `reconnecting`; still read-only. ❌ New surfaces reading FSM state directly to infer "connection dropped".

### Composition Over Props
```tsx
// ❌ <TaskModal task={task} showChat showHistory showContext />
// ✅ <TaskModal task={task}><TaskModal.Chat /><TaskModal.History /></TaskModal>
```

### Import Order
1. React & framework
2. Third-party (alphabetical)
3. Internal (@/)
4. Stores
5. Types (`import type`)
6. Components (general → specific)
7. Local (relative)

## Commands
```bash
npm test           # watch mode
npm run test:run   # single run
npm run typecheck  # TS check
npm run lint       # ESLint
npx playwright test tests/visual/views/chat/chat-widget-matrix.spec.ts                     # verify chat widget visuals
npx playwright test tests/visual/views/chat/chat-widget-matrix.spec.ts --update-snapshots  # refresh chat widget baselines
```
Visual-test dev servers may be started/stopped by agents for the scoped run; prefer Playwright and follow the explicit-request-only Computer Use boundary in `../../.claude/rules/visual-testing.md`.
Playwright visual rule: run from `frontend/` only; do not launch from repo root with `--config frontend/playwright.config.ts`, or `page.goto('/')` can fail before the configured `baseURL`/`webServer` is applied.
Playwright report rule: `frontend/playwright.config.ts` keeps `use.screenshot = "on"` so every run has an end-of-test screenshot in the HTML report; use explicit `testInfo.attach(...)` in multi-state specs when one final screenshot is not enough.

## Task Management (MANDATORY)
Use TaskCreate/TaskUpdate/TaskList for complex work. See `../../.claude/rules/task-management.md`

## Adding Features
1. Types: Zod schema in types/
2. API: wrapper in lib/tauri.ts
3. Store: Zustand+immer
4. Hook: TanStack Query
5. Component: with co-located test
6. **Tests FIRST (TDD mandatory)**
