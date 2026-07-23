# Active Plan Feature

## Overview

The **Active Plan** feature allows you to focus your work by selecting a single
plan to view across RalphX. Plan selection is project-scoped and is surfaced
inside the Agents Tasks artifact: its Kanban and Graph tabs filter to tasks
from the active plan, helping you maintain focus and avoid context switching.

## Key Concepts

### What is an Active Plan?

An active plan is an ideation session that has been "accepted" and converted to tasks. Each project can have one active plan at a time. The active plan selection:

- **Persists across app restarts** — Your selection is saved in the database
- **Is project-scoped** — Each project has its own active plan
- **Filters the Agents task views** — The embedded Graph and Kanban tabs show only tasks from the active plan
- **Can be changed anytime** — Switch plans quickly using the inline selector or keyboard shortcut

### Plan Eligibility

Only **accepted** ideation sessions can be selected as active plans:

- ✅ Sessions with status = "accepted" and tasks created
- ❌ Active sessions (still in ideation mode)
- ❌ Archived sessions

## How to Select a Plan

### Method 1: Inline Plan Selector

The inline plan selector appears in the Agents Tasks artifact's Kanban and
Graph controls.

**Features:**
- Shows current plan title and task count badge
- Opens a searchable dropdown of all accepted plans
- Displays task statistics (incomplete/total tasks)
- Shows "Active" badge for plans with tasks in progress
- Includes "Clear selection" option to remove filtering

**Steps:**
1. Click the plan selector button in the toolbar
2. Type to search for a plan (optional)
3. Click a plan to select it
4. Use keyboard navigation (↑/↓ arrows + Enter) if preferred

**Compact Mode:**
In tight layouts, the selector shows an icon-only button to save space.

### Method 2: Quick Switcher (Keyboard)

Press **`Cmd+Shift+P`** (Mac) or **`Ctrl+Shift+P`** (Windows/Linux) to open the plan quick switcher.

**Features:**
- Non-modal floating panel (no backdrop)
- Type-to-search interface
- Keyboard navigation (↑/↓ arrows + Enter)
- Press `Escape` to close
- Currently active plan is highlighted with a checkmark

**Why Non-Modal?**
The quick switcher is designed as a lightweight command palette that doesn't block the UI. You can see your workspace context while switching plans, reducing cognitive load.

## How Plans Are Ranked

When you open the plan selector or quick switcher, plans are ranked using a weighted scoring algorithm that combines three factors:

### 1. Interaction Score (45% weight)

Tracks how often and how recently you've manually selected each plan:

- **Frequency:** More selections = higher score (logarithmic scaling)
- **Recency:** Recent selections boost the score (21-day decay)
- **Formula:** `ln(selection_count + 1) / ln(10) * exp(-days_since / 21)`

### 2. Activity Score (35% weight)

Measures current work activity in the plan:

- **Active tasks:** Plans with tasks in "executing," "review," or "merge" states get a bonus
- **Incomplete ratio:** Plans with more incomplete tasks rank higher
- **Formula:** `0.6 * (has_active_tasks ? 1 : 0) + 0.4 * (incomplete / total)`

### 3. Recency Score (20% weight)

Considers when the plan was accepted (created):

- Newer plans rank higher than older ones
- **Formula:** `exp(-days_since_acceptance / 30)`

### Final Score

```
final_score = 0.45 * interaction_score + 0.35 * activity_score + 0.20 * recency_score
```

**Tie-breakers:**
1. Higher score wins
2. If tied, newer accepted_at timestamp wins
3. If still tied, alphabetical by title

**Result:** Plans you actively use and work on bubble to the top automatically.

## Cross-View Synchronization

The active plan selection is **globally synchronized** across all views:

1. Select a plan in the **Kanban inline selector** → Graph updates immediately
2. Select a plan in the **Graph inline selector** → Kanban updates immediately
3. Select a plan via **quick switcher** → Both views update immediately
4. Accept a plan in the Agents **Plan** artifact → Becomes active plan; embedded task views filter on navigation

All selection events are tracked with a "source" label (`kanban_inline`, `graph_inline`, `quick_switcher`, `ideation`) for analytics and to power the interaction ranking.

## Empty States

### No Plan Selected

If no active plan is set, both Graph and Kanban show an empty state:

**Graph:**
```
⚠️  No plan selected
Select a plan to view work on the Graph.
[Plan Selector Button]
```

**Kanban:**
```
📄  No plan selected
Select a plan to view work on the Kanban board.
[Plan Selector Button]
or press Cmd+Shift+P
```

### No Accepted Plans Available

If you try to open the selector but have no accepted plans yet:

```
📄  No accepted plans found
Create and accept a plan in the Agents **Plan** artifact first.
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd+Shift+P` (Mac)<br>`Ctrl+Shift+P` (Windows/Linux) | Open plan quick switcher |
| `↑` / `↓` | Navigate plans in selector/switcher |
| `Enter` | Select highlighted plan |
| `Escape` | Close selector/switcher |

**Note:** The `Cmd+Shift+P` shortcut is ignored when typing in text inputs or textareas to avoid conflicts.

## Interaction with Other Features

### Archived Tasks

The "Show Archived" toggle in Kanban and Graph respects the active plan filter:

- ✅ Archived tasks **from the active plan** are shown when toggle is on
- ❌ Archived tasks **from other plans** remain hidden

### Task Search

The search bar in Kanban filters only tasks from the active plan:

- If no plan is selected, search returns no results
- Search across all plans is not supported (by design for focus)

### Reopening a Session

If you reopen an ideation session that was previously accepted (and is currently active):

1. The session status changes back to "active"
2. The active plan is **automatically cleared** for the project
3. Graph and Kanban show empty states
4. You must select a different plan to continue working

## Best Practices

### 1. Accept Plans When Ready
Only accept ideation sessions when you're ready to work on them. Accepted plans clutter the selector if they contain no actionable tasks.

### 2. Use Quick Switcher for Fast Context Switching
Train muscle memory for `Cmd+Shift+P` to switch plans without leaving the keyboard. Type a few characters of the plan name to filter instantly.

### 3. Leverage Ranking Intelligence
The ranking algorithm learns from your behavior. Plans you work on frequently will automatically appear at the top, reducing search time.

### 4. Clear Selection When Planning
If you're in ideation mode or reviewing proposals, clear the active plan to prevent confusion. The empty state reminds you no work is currently filtered.

### 5. One Plan Per Project
Remember that active plan is scoped by project. If you work on multiple projects, each project maintains its own active plan selection independently.

## Troubleshooting

### Problem: "No plan selected" persists after selecting

**Possible Causes:**
1. The selected session was deleted or reopened (status changed)
2. Database connection issue

**Solution:**
1. Check that the session still exists in the owning Agents conversation
2. Try reselecting the plan
3. Restart the app if persistence fails

### Problem: Plan selector shows outdated task counts

**Possible Causes:**
1. Task state changed but selector wasn't refreshed
2. Multiple instances of the app running

**Solution:**
1. Close and reopen the selector dropdown to refresh data
2. Click the refresh icon if available
3. Ensure only one instance of RalphX is running

### Problem: Quick switcher doesn't open with `Cmd+Shift+P`

**Possible Causes:**
1. Another app or system shortcut is capturing the key combination
2. You're typing in a text input (shortcut is intentionally disabled)

**Solution:**
1. Check System Preferences → Keyboard → Shortcuts for conflicts
2. Click outside any input field and try again
3. Use the inline selector as an alternative

### Problem: Selected plan doesn't show any tasks

**Possible Causes:**
1. All tasks in the plan are archived
2. The plan was accepted but no tasks were created
3. Tasks were deleted manually

**Solution:**
1. Toggle "Show Archived" to reveal archived tasks
2. Check the owning Agents conversation's Tasks artifact to verify tasks were created
3. Select a different plan with active tasks

### Problem: Ranking seems incorrect

**Possible Causes:**
1. Selection tracking data is missing (fresh install or DB migration)
2. Task stats are out of sync

**Solution:**
1. Use the plan a few times to build selection history
2. Ranking will improve as you interact with plans
3. Manually select frequently-used plans to boost their ranking

## Technical Details (for advanced users)

### Database Tables

**`project_active_plan`**
- Stores one active plan per project
- Primary key: `project_id`
- Foreign keys: `ideation_session_id` (cascades on delete)

**`plan_selection_stats`**
- Tracks selection frequency and recency per project-plan pair
- Composite primary key: `(project_id, ideation_session_id)`
- Columns: `selected_count`, `last_selected_at`, `last_selected_source`

### API Commands

- `get_active_plan(project_id)` → Returns session ID or null
- `set_active_plan(project_id, session_id, source)` → Sets active plan and records selection
- `clear_active_plan(project_id)` → Removes active plan for project

### State Management

Frontend state is managed by `planStore` (Zustand):
- `activePlanByProject: Record<string, string | null>` — Maps project IDs to active session IDs
- `planCandidates: PlanCandidate[]` — Cached list of selectable plans with stats
- `loadCandidates(projectId, query?)` — Loads and ranks plans (backend call)
- `setActivePlan(projectId, sessionId, source)` — Updates active plan (backend + store)
- `clearActivePlan(projectId)` — Clears active plan (backend + store)

### Selection Source Tracking

Every plan selection records its source:
- `kanban_inline` — Selected from Kanban toolbar
- `graph_inline` — Selected from Graph controls
- `quick_switcher` — Selected via `Cmd+Shift+P` palette
- `ideation` — Auto-selected when accepting a session

This data powers interaction scoring and enables future analytics features.
