# Active Plan API Architecture

## Overview

The Active Plan feature provides a project-scoped, persisted "active plan"
state that drives task filtering across the Graph and Kanban tabs embedded in
the Agents Tasks artifact. This document describes the backend API, database
schema, ranking algorithm (planned), and frontend integration points.

## Architecture Layers

```
┌─────────────────────────────────────────────────────────────┐
│ Frontend (React/TypeScript)                                  │
│ ┌─────────────────┐  ┌──────────────────┐                  │
│ │ planStore       │  │ UI Components    │                  │
│ │ (Zustand)       │  │ - PlanSelector   │                  │
│ │                 │  │ - QuickSwitcher  │                  │
│ └────────┬────────┘  └──────────────────┘                  │
│          │                                                   │
│    ┌─────▼──────┐                                           │
│    │ planApi    │                                           │
│    │ (invoke)   │                                           │
│    └─────┬──────┘                                           │
└──────────┼────────────────────────────────────────────────┘
           │ Tauri IPC
┌──────────▼────────────────────────────────────────────────┐
│ Backend (Rust/Tauri)                                       │
│ ┌──────────────────────────────────────────────────────┐  │
│ │ Tauri Commands (plan_commands.rs)                    │  │
│ │ - get_active_plan                                    │  │
│ │ - set_active_plan                                    │  │
│ │ - clear_active_plan                                  │  │
│ └────────┬───────────────────────────────────────────────┘  │
│          │                                                   │
│ ┌────────▼───────────────────────────────────────────────┐  │
│ │ ActivePlanRepository (trait)                          │  │
│ │ Implementations:                                      │  │
│ │ - SqliteActivePlanRepo                               │  │
│ │ - MemoryActivePlanRepo (tests)                       │  │
│ └────────┬───────────────────────────────────────────────┘  │
│          │                                                   │
│ ┌────────▼───────────────────────────────────────────────┐  │
│ │ Database (SQLite)                                     │  │
│ │ - project_active_plan                                │  │
│ │ - plan_selection_stats                               │  │
│ └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## Database Schema

### Table: `project_active_plan`

Stores the currently active plan (ideation session) for each project.

```sql
CREATE TABLE project_active_plan (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    ideation_session_id TEXT NOT NULL REFERENCES ideation_sessions(id) ON DELETE CASCADE,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
);

CREATE INDEX idx_project_active_plan_session ON project_active_plan(ideation_session_id);
```

**Constraints & Behavior:**
- **Primary key:** `project_id` (one active plan per project max)
- **Foreign key cascade:** If session is deleted, active plan row is deleted (plan cleared)
- **Updated timestamp:** Tracks when the active plan was last changed

**Notes:**
- If a session is reopened (status changed back to "active"), application logic must manually clear the active plan
- The table can be empty (no row) if no plan is selected for a project

### Table: `plan_selection_stats`

Tracks interaction history for ranking plans by frequency and recency of selection.

```sql
CREATE TABLE plan_selection_stats (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    ideation_session_id TEXT NOT NULL REFERENCES ideation_sessions(id) ON DELETE CASCADE,
    selected_count INTEGER NOT NULL DEFAULT 0,
    last_selected_at TEXT NULL,
    last_selected_source TEXT NULL,
    PRIMARY KEY (project_id, ideation_session_id)
);

CREATE INDEX idx_plan_selection_stats_session ON plan_selection_stats(ideation_session_id);
CREATE INDEX idx_plan_selection_stats_last_selected ON plan_selection_stats(last_selected_at);
```

**Columns:**
- `selected_count` — Total number of times this plan was manually selected
- `last_selected_at` — ISO8601 timestamp of most recent selection
- `last_selected_source` — Source of selection: `kanban_inline`, `graph_inline`, `quick_switcher`, or `ideation`

**Update Pattern (UPSERT):**
```sql
INSERT INTO plan_selection_stats
    (project_id, ideation_session_id, selected_count, last_selected_at, last_selected_source)
VALUES (?, ?, 1, ?, ?)
ON CONFLICT(project_id, ideation_session_id) DO UPDATE SET
    selected_count = selected_count + 1,
    last_selected_at = excluded.last_selected_at,
    last_selected_source = excluded.last_selected_source;
```

This ensures every selection increments the counter and updates the timestamp/source atomically.

## Backend API

### Tauri Commands

All commands are defined in `src-tauri/src/commands/plan_commands.rs`.

#### 1. `get_active_plan`

Retrieves the active plan session ID for a project.

**Signature:**
```rust
#[tauri::command]
pub async fn get_active_plan(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String>
```

**Parameters:**
- `project_id` (String) — The project ID

**Returns:**
- `Ok(Some(session_id))` — If an active plan is set
- `Ok(None)` — If no active plan is set
- `Err(error_message)` — On database error

**Frontend Usage:**
```typescript
import { invoke } from "@tauri-apps/api/core";

const sessionId = await invoke<string | null>("get_active_plan", {
  projectId: "proj-123"
});
```

#### 2. `set_active_plan`

Sets the active plan for a project. Validates that the session exists, belongs to the project, and is accepted.

**Signature:**
```rust
#[tauri::command]
pub async fn set_active_plan(
    project_id: String,
    ideation_session_id: String,
    source: String,
    state: State<'_, AppState>,
) -> Result<(), String>
```

**Parameters:**
- `project_id` (String) — The project ID
- `ideation_session_id` (String) — The session ID to set as active
- `source` (String) — Selection source for tracking: `"kanban_inline"`, `"graph_inline"`, `"quick_switcher"`, or `"ideation"`

**Returns:**
- `Ok(())` — On success
- `Err(error_message)` — On validation failure or database error

**Validations:**
1. Session exists in the database
2. Session belongs to the specified project
3. Session status is "accepted"

**Side Effects:**
1. UPSERT into `project_active_plan` table
2. UPSERT into `plan_selection_stats` table (increments `selected_count`, updates `last_selected_at` and `last_selected_source`)

**Frontend Usage:**
```typescript
await invoke("set_active_plan", {
  projectId: "proj-123",
  ideationSessionId: "session-456",
  source: "kanban_inline"
});
```

#### 3. `clear_active_plan`

Clears the active plan for a project.

**Signature:**
```rust
#[tauri::command]
pub async fn clear_active_plan(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<(), String>
```

**Parameters:**
- `project_id` (String) — The project ID

**Returns:**
- `Ok(())` — On success (even if no active plan was set)
- `Err(error_message)` — On database error

**Side Effects:**
- Deletes row from `project_active_plan` table where `project_id` matches

**Frontend Usage:**
```typescript
await invoke("clear_active_plan", { projectId: "proj-123" });
```

### Repository Layer

The backend uses a repository pattern for database abstraction.

**Trait:** `ActivePlanRepository`

**Key Methods:**
- `get(&self, project_id: &ProjectId) -> Result<Option<IdeationSessionId>>`
- `set(&self, project_id: &ProjectId, session_id: &IdeationSessionId) -> Result<()>`
- `clear(&self, project_id: &ProjectId) -> Result<()>`
- `record_selection(&self, project_id: &ProjectId, session_id: &IdeationSessionId, source: &str) -> Result<()>`

**Implementations:**
- `SqliteActivePlanRepo` — Production implementation using SQLite
- `MemoryActivePlanRepo` — In-memory implementation for unit tests

### Extended Tauri Commands (Session Filtering)

Several existing commands now accept an optional `ideation_session_id` parameter to filter results by active plan:

#### Task Commands
- `list_tasks(ideation_session_id: Option<String>)` — Filter tasks by session
- `search_tasks(ideation_session_id: Option<String>)` — Filter search results by session
- `get_archived_count(ideation_session_id: Option<String>)` — Count archived tasks in session

#### Graph/Timeline Commands
- `get_task_dependency_graph(session_id: Option<String>)` — Filter graph nodes by session
- `get_task_timeline_events(session_id: Option<String>)` — Filter timeline events by session

**Filtering Logic:**
- If `session_id` is `None` — Show all tasks (legacy behavior)
- If `session_id` is `Some(id)` — Show only tasks where `ideation_session_id = id`

## Ranking Algorithm (Planned)

**Status:** The backend ranking algorithm (`list_plan_selector_candidates`) is **not yet implemented**. The current frontend uses client-side filtering of `list_ideation_sessions` results.

When implemented, the ranking algorithm will combine three weighted factors:

### 1. Interaction Score (45% weight)

Measures how often and how recently a plan was manually selected.

**Formula:**
```
frequency_score = ln(selected_count + 1) / ln(10)  // Capped at 1.0
recency_decay = exp(-days_since_selection / 21)    // 21-day half-life
interaction_score = frequency_score * recency_decay
```

**Data Source:** `plan_selection_stats.selected_count`, `plan_selection_stats.last_selected_at`

### 2. Activity Score (35% weight)

Measures current work activity in the plan.

**Formula:**
```
active_now_bonus = has_active_tasks ? 1.0 : 0.0    // Tasks in executing/review/merge states
incomplete_ratio = incomplete_tasks / total_tasks

activity_score = 0.6 * active_now_bonus + 0.4 * incomplete_ratio
```

**Data Source:** Task count queries grouped by `ideation_session_id` and filtered by `internal_status`

### 3. Recency Score (20% weight)

Measures how recently the plan was accepted (created).

**Formula:**
```
recency_score = exp(-days_since_acceptance / 30)   // 30-day half-life
```

**Data Source:** `ideation_sessions.converted_at` (timestamp when session was accepted)

### Final Score

```
final_score = 0.45 * interaction_score + 0.35 * activity_score + 0.20 * recency_score
```

**Tie-Breakers (in order):**
1. Higher `final_score`
2. Newer `converted_at` timestamp
3. Alphabetical by `title` (case-insensitive)
4. Alphabetical by `session_id` (deterministic)

### Planned Response Type

```rust
pub struct PlanCandidateResponse {
    pub session_id: String,
    pub title: Option<String>,
    pub accepted_at: String,  // ISO8601 timestamp from converted_at
    pub task_stats: TaskStats,
    pub interaction_stats: InteractionStats,
    pub score: f64,
    pub score_breakdown: Option<ScoreBreakdown>,  // Debug only
}

pub struct TaskStats {
    pub total: u32,
    pub incomplete: u32,
    pub active_now: u32,  // executing + review + merge
}

pub struct InteractionStats {
    pub selected_count: u32,
    pub last_selected_at: Option<String>,
}

pub struct ScoreBreakdown {
    pub interaction_score: f64,
    pub activity_score: f64,
    pub recency_score: f64,
    pub final_score: f64,
}
```

## Frontend API

### Module: `src/api/plan.ts`

**Exported Types:**
```typescript
export type SelectionSource = "kanban_inline" | "graph_inline" | "quick_switcher" | "ideation";
```

**API Object:**
```typescript
export const planApi = {
  getActivePlan: (projectId: string): Promise<string | null>,
  setActivePlan: (projectId: string, sessionId: string, source: SelectionSource): Promise<void>,
  clearActivePlan: (projectId: string): Promise<void>,
}
```

### Store: `src/stores/planStore.ts`

**State:**
```typescript
interface PlanState {
  activePlanByProject: Record<string, string | null>;  // projectId → sessionId
  planCandidates: PlanCandidate[];  // Cached candidates from last load
  isLoading: boolean;
  error: string | null;
}
```

**Actions:**
```typescript
interface PlanActions {
  loadActivePlan: (projectId: string) => Promise<void>;
  setActivePlan: (projectId: string, sessionId: string, source: SelectionSource) => Promise<void>;
  clearActivePlan: (projectId: string) => Promise<void>;
  loadCandidates: (projectId: string, query?: string) => Promise<void>;
}
```

**Usage Example:**
```typescript
import { usePlanStore } from "@/stores/planStore";

function MyComponent() {
  const projectId = useProjectStore((s) => s.activeProjectId);
  const activePlanId = usePlanStore((s) => s.activePlanByProject[projectId] ?? null);
  const setActivePlan = usePlanStore((s) => s.setActivePlan);

  const handleSelect = async (sessionId: string) => {
    await setActivePlan(projectId, sessionId, "kanban_inline");
  };

  return (
    <div>
      Active Plan: {activePlanId ?? "None"}
      <button onClick={() => handleSelect("session-123")}>Select Plan</button>
    </div>
  );
}
```

**Selectors:**
```typescript
export const selectActivePlanId = (projectId: string) => (state: PlanState) =>
  state.activePlanByProject[projectId] ?? null;

export const selectCurrentActivePlan = (state: PlanState & { activeProjectId: string | null }) =>
  state.activeProjectId ? state.activePlanByProject[state.activeProjectId] ?? null : null;
```

## View Integration

### Agents Tasks — Kanban tab (`frontend/src/components/tasks/TaskBoard/TaskBoard.tsx`)

**Changes:**
1. Added `<PlanSelectorInline>` to toolbar
2. Pass `activePlanId` to all task queries (`useInfiniteTasksQuery`, `searchTasks`, `getArchivedCount`)
3. Show empty state when `activePlanId` is null

**Query Example:**
```typescript
const { data: taskColumns } = useInfiniteTasksQuery({
  projectId,
  statuses: ["draft", "ready", "executing"],
  ideationSessionId: activePlanId,  // ← Filter by active plan
  includeArchived: showArchived,
});
```

### Agents Tasks — Graph tab (`frontend/src/components/TaskGraph/TaskGraphView.tsx`)

**Changes:**
1. Removed multi-plan filter UI (previously `filters.planIds[]`)
2. Added `<PlanSelectorInline>` to controls area
3. Pass `activePlanId` to graph query
4. Show empty state when `activePlanId` is null

**Query Example:**
```typescript
const { data: graphData } = useQuery({
  queryKey: ["taskGraph", projectId, activePlanId, includeArchived],
  queryFn: () => taskApi.getDependencyGraph(projectId, includeArchived, activePlanId),
});
```

### Agents Plan/Ideation artifact (`frontend/src/components/agents/AgentsArtifactPane.tsx`)

**Changes:**
1. On session acceptance (`apply_proposals_to_kanban` success), call `planStore.setActivePlan(projectId, sessionId, "ideation")`
2. On session reopen, call `planStore.clearActivePlan(projectId)` if the reopened session was active

**Example:**
```typescript
const setActivePlan = usePlanStore((s) => s.setActivePlan);

const handleApplyProposals = async () => {
  await applyProposals.mutateAsync({ sessionId, proposalIds, options });

  // Auto-set as active plan
  await setActivePlan(projectId, sessionId, "ideation");

  revealAgentArtifactTab(conversationId, "tasks");
};
```

## Error Handling

### Validation Errors

`set_active_plan` command validates:
1. **Session exists** — Returns error if session ID is invalid
2. **Session belongs to project** — Returns error if project ID mismatch
3. **Session is accepted** — Returns error if status is not "accepted"

**Error Format:**
```rust
Err("Session not found".to_string())
Err("Session does not belong to project".to_string())
Err("Only accepted sessions can be set as active plan".to_string())
```

**Frontend Handling:**
```typescript
try {
  await planApi.setActivePlan(projectId, sessionId, source);
} catch (error) {
  console.error("Failed to set active plan:", error);
  // Show toast notification to user
}
```

### Cascade Behavior

**Scenario:** User deletes an ideation session that is currently active.

**Behavior:**
1. Database foreign key cascade deletes row from `project_active_plan`
2. Database foreign key cascade deletes row(s) from `plan_selection_stats`
3. Frontend state becomes stale (still shows deleted session as active)

**Solution:**
- Frontend should call `loadActivePlan` after session deletions
- Or use event-driven state sync (future enhancement)

### Reopen Session Behavior

**Scenario:** User reopens a session (status changes from "accepted" back to "active").

**Behavior:**
1. Session is no longer eligible as active plan
2. Application logic must manually call `clear_active_plan` for the project
3. The Agents Tasks artifact shows an empty state

**Implementation Location:** `src-tauri/src/commands/ideation_commands.rs::reopen_ideation_session`

## Testing Strategy

### Backend Unit Tests

**Test Cases:**
1. `get_active_plan` returns None when no plan is set
2. `set_active_plan` succeeds for valid accepted session
3. `set_active_plan` fails for non-accepted session
4. `set_active_plan` increments selection count in stats table
5. `clear_active_plan` removes row from table
6. Foreign key cascade deletes active plan when session is deleted
7. Selection stats are correctly recorded with source tracking

**Test Location:** `src-tauri/src/infrastructure/sqlite/sqlite_active_plan_repo.rs` (integration tests)

### Frontend Unit Tests

**Test Cases:**
1. `planStore.loadActivePlan` updates state correctly
2. `planStore.setActivePlan` calls backend and updates state
3. `planStore.clearActivePlan` calls backend and clears state
4. `PlanSelectorInline` shows "No plan selected" when activePlanId is null
5. `PlanSelectorInline` shows plan title and task count when activePlanId is set
6. Keyboard navigation (↑/↓/Enter/Escape) works in selector
7. Quick switcher opens on Cmd+Shift+P

**Test Locations:**
- `src/stores/planStore.test.ts`
- `src/components/plan/PlanSelectorInline.test.tsx`
- `src/components/plan/PlanQuickSwitcherPalette.test.tsx`

### Integration Tests

**Test Cases:**
1. Select plan in Kanban → Graph updates immediately
2. Select plan in Graph → Kanban updates immediately
3. Accept session in Ideation → Active plan is set → Navigate to Kanban → Tasks are filtered
4. Reopen session → Active plan is cleared → Views show empty state
5. Delete session → Active plan is cleared (cascade)

## Performance Considerations

### Database Queries

**`get_active_plan` Query:**
```sql
SELECT ideation_session_id FROM project_active_plan WHERE project_id = ?
```
- **Complexity:** O(1) — Primary key lookup
- **Index:** PRIMARY KEY on `project_id`

**`set_active_plan` Validation Query:**
```sql
SELECT id, project_id, status FROM ideation_sessions WHERE id = ?
```
- **Complexity:** O(1) — Primary key lookup

**`set_active_plan` Selection Stats UPSERT:**
```sql
INSERT INTO plan_selection_stats (...) VALUES (...)
ON CONFLICT(...) DO UPDATE SET ...
```
- **Complexity:** O(1) — Composite primary key lookup + update

**Graph/Kanban Filter Query (example):**
```sql
SELECT * FROM tasks WHERE project_id = ? AND ideation_session_id = ? AND ...
```
- **Complexity:** O(n) — Table scan, but reduced by session filter
- **Index:** Existing `idx_tasks_project_status` helps, but consider composite index `(project_id, ideation_session_id, internal_status)` for optimal performance

### Frontend State Management

**Store Design:**
- `activePlanByProject` uses `Record<string, string | null>` for O(1) lookup
- State updates are localized to single project (no global re-render)
- `planCandidates` cached to avoid redundant backend calls

**Optimization:**
- Use React memoization (`useMemo`, `React.memo`) for candidate list rendering
- Debounce search input (300ms) to reduce backend calls

## Migration Guide

### Adding Active Plan to Existing Project

**Step 1: Run Database Migration**
```sql
-- Create active plan table
CREATE TABLE project_active_plan (...);
CREATE INDEX idx_project_active_plan_session ON project_active_plan(ideation_session_id);

-- Create selection stats table
CREATE TABLE plan_selection_stats (...);
CREATE INDEX idx_plan_selection_stats_session ON plan_selection_stats(ideation_session_id);
CREATE INDEX idx_plan_selection_stats_last_selected ON plan_selection_stats(last_selected_at);
```

**Step 2: Update Tauri Command Registration**
Add commands to `src-tauri/src/lib.rs`:
```rust
.invoke_handler(tauri::generate_handler![
    commands::plan_commands::get_active_plan,
    commands::plan_commands::set_active_plan,
    commands::plan_commands::clear_active_plan,
    // ... other commands
])
```

**Step 3: Initialize Frontend Store**
No initialization needed — `activePlanByProject` starts empty, views show empty state.

**Step 4: Load Active Plan on App Start**
```typescript
const projectId = useProjectStore((s) => s.activeProjectId);
const loadActivePlan = usePlanStore((s) => s.loadActivePlan);

useEffect(() => {
  if (projectId) {
    loadActivePlan(projectId);
  }
}, [projectId, loadActivePlan]);
```

## Future Enhancements

### 1. Implement Backend Ranking (`list_plan_selector_candidates`)

**Status:** Not implemented. Current frontend uses client-side filtering.

**TODO:**
- Implement ranking algorithm in Rust
- Add Tauri command `list_plan_selector_candidates`
- Update `planApi.listCandidates` to call new command
- Update `planStore.loadCandidates` to use backend ranking

### 2. Multi-Plan Comparison View

Allow side-by-side comparison of 2-3 plans in Graph view.

**Challenges:**
- Graph layout complexity (distinguishing nodes by plan)
- Cross-plan dependency visualization

### 3. Plan Analytics Dashboard

Show selection frequency, work patterns, and completion rates per plan.

**Data Source:** `plan_selection_stats` table + task completion timestamps

### 4. Plan Archival

Mark old/completed plans as "archived" to reduce selector noise.

**Schema Addition:**
```sql
ALTER TABLE ideation_sessions ADD COLUMN is_archived BOOLEAN DEFAULT FALSE;
```

### 5. Recently Viewed Plans

Add "Recent" section in quick switcher based on `last_selected_at`.

**UI Change:** Group candidates by "Recent" (last 7 days) vs "All Plans"

## Troubleshooting

### Symptom: Active plan doesn't persist after app restart

**Cause:** `project_active_plan` row not saved to disk (SQLite WAL not flushed).

**Solution:**
- Ensure database is properly closed on app exit
- Check SQLite `PRAGMA journal_mode` setting

### Symptom: "Only accepted sessions can be set as active plan" error

**Cause:** Session status is not "accepted".

**Solution:**
- Check `ideation_sessions.status` column value
- Ensure `converted_at` timestamp is set (indicates acceptance)

### Symptom: Selection stats not updating

**Cause:** `record_selection` call is failing silently.

**Solution:**
- Check `plan_selection_stats` foreign key constraints
- Verify session exists and belongs to project

### Symptom: Agents task tabs show all tasks despite active plan set

**Cause:** Backend query not passing `ideation_session_id` parameter.

**Solution:**
- Verify frontend is passing `activePlanId` to query functions
- Check backend query includes `WHERE ideation_session_id = ?` clause

## Related Documentation

- [Active Plan User Guide](../features/active-plan.md) — End-user documentation
- [Task State Machine](../../.claude/rules/task-state-machine.md) — Task lifecycle states
- [Git Workflow](../../.claude/rules/task-git-branching.md) — Task branch management
- [Ideation System](../architecture/ideation-system.md) — Session and proposal management (if exists)
