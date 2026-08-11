# Graph View User Guide

The Graph View visualizes your project's tasks and their dependencies as an interactive node graph. See the full shape of your work — what's blocked, what's running, what's on the critical path — at a glance.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| How do I open the Graph View? | Open **Agents**, choose a conversation, then select the **Graph** tab in its Tasks artifact. |
| Nothing shows — why? | You need to select an active plan first. The graph only shows tasks for the current plan. |
| How do I zoom in/out? | Scroll wheel, or use the zoom controls (bottom-left). |
| How do I fit everything in view? | Click the **fit view** button in the zoom controls. |
| Why are nodes tiny? | Compact mode is on. Groups with 8+ tasks auto-compact. Toggle Standard/Compact in the left filter panel. |
| What does the orange glowing line mean? | It's on the **critical path** — the longest dependency chain that determines overall completion time. |
| How do I see task details? | Click a node to open the Agents task detail overlay. |

---

## Table of Contents

1. [Overview](#overview)
2. [Navigating the Graph](#navigating-the-graph)
3. [Understanding Nodes](#understanding-nodes)
4. [Understanding Edges](#understanding-edges)
5. [Grouping](#grouping)
6. [Timeline Panel](#timeline-panel)
7. [Filtering](#filtering)
8. [Active Plan Context](#active-plan-context)
9. [Graph + Timeline Layout](#graph--timeline-layout)
10. [Keyboard Shortcuts](#keyboard-shortcuts)
11. [Tips & Best Practices](#tips--best-practices)

---

## Overview

The Graph View shows every task in the active plan as a node, with arrows representing dependencies between them. Tasks flow from dependencies (top/left) toward dependents (bottom/right) in a hierarchical layout computed automatically by the dagre algorithm.

**When to use Graph View vs Kanban:**

| Use Graph View when... | Use Kanban when... |
|------------------------|--------------------|
| You need to see dependency relationships | You want to manage status/priority |
| You're planning execution order | You're reviewing what's in each state |
| You want to identify the critical path | You're doing daily task triage |
| You're investigating a blocked task | You want a linear, column-based view |

### Layout Direction

The graph can be arranged in two directions, toggled via the left filter panel:

| Direction | Description |
|-----------|-------------|
| **TB** (top-to-bottom) | Dependencies flow downward. Default. |
| **LR** (left-to-right) | Dependencies flow rightward. Better for wide plans. |

---

## Navigating the Graph

### Pan and Zoom

- **Scroll** — zoom in/out
- **Click and drag on canvas** — pan
- **Zoom controls** (bottom-left) — zoom in, zoom out, fit view, reset zoom
- **Fit View button** — fits all visible nodes into the viewport

The graph enforces zoom limits of 0.6×–1× to keep nodes readable.

### Selecting Nodes

- **Single click on a task node** — selects it; opens the detail panel on the right and centers the node in view
- **Single click on a plan group header** — selects the plan group; centers the group in view
- **Click on canvas (empty space)** — deselects everything
- **Double-click on a task node** — opens the Agents task detail overlay

### Node Highlighting

When you click a task in the [Timeline Panel](#timeline-panel), the corresponding node is **highlighted** in the graph with a distinct ring — useful for correlating timeline events to graph position.

---

## Understanding Nodes

Each task is a node. Nodes have two display modes:

| Mode | When used | What shows |
|------|-----------|------------|
| **Standard** | Default; fewer than 8 tasks per group | Title, status badge, step progress dots, category |
| **Compact** | Auto-enabled at 8+ tasks per group | Title and status indicator only (smaller footprint) |

You can override the auto mode at any time with the Standard/Compact toggle in the filter panel.

### What Each Node Shows (Standard Mode)

| Element | Description |
|---------|-------------|
| **Border color** | Reflects task status — each status has a distinct color (see Legend) |
| **Background** | Subtle fill matching the status category |
| **Title** | Task title, truncated if long |
| **Status badge** | Current status in human-readable form |
| **Step dots** | Row of small dots showing step-level progress within the task |
| **Critical path marker** | Orange glow on the node border when it's on the critical path |
| **Highlight ring** | Bright ring when the node is highlighted via timeline click |
| **Focus ring** | Keyboard focus indicator |

### Step Dot Color Coding

| Color | Meaning |
|-------|---------|
| **Gray** | Completed step |
| **Green** | Completed step in a terminal-complete task (merged/approved) |
| **Orange** | Step currently in progress |
| **Red** | Failed step |
| **Dark gray** | Pending step |
| **Muted gray** | Skipped step |

### Context Menu (Right-click)

Right-clicking a node reveals status-appropriate actions:

| Action | Available when |
|--------|----------------|
| View Details | Always |
| Start Execution | Task is Ready |
| Block / Unblock | Task is in a blockable state |
| Approve | Task is in Review Passed |
| Request Changes | Task is under review |
| Mark Resolved | Task has a merge conflict |
| Remove | Always (with confirmation) |

---

## Understanding Edges

Arrows between nodes represent dependencies — an arrow from A to B means "B depends on A" (A must complete before B can start).

### Edge Styles

| Style | Meaning |
|-------|---------|
| **Dashed gray, 1px** | Standard dependency |
| **Solid orange, 2px + glow** | Critical path dependency — part of the longest chain |
| **Animated dotted orange** | Dependency from a currently executing task |

The critical path — the longest chain of dependencies that determines the earliest possible completion date — is highlighted in orange. Tasks on the critical path are the ones where delays have the most impact on overall progress.

### Edge Tooltip

Hover over the small **center dot** on any edge to see a tooltip showing the relationship:

```
Source Task Title  →  Target Task Title
```

This is useful when edges overlap or when you need to verify which tasks are connected.

### Cross-Plan Edges

When a dependency crosses plan group boundaries (one task depends on a task from a different plan), the edge is rendered on top of group containers so it remains visible.

---

## Grouping

The graph supports two grouping axes, both enabled by default:

### By Plan

Tasks are grouped into **Plan containers** — one container per ideation plan. Each plan group shows:

- The plan name as a header
- Task count and completion progress (e.g., "3 / 8 complete")
- A collapse/expand toggle

**Collapse behavior:** All plan groups except the first incomplete one are collapsed by default. Expanding a group expands all its tier sub-groups too. Your manual expand/collapse choices are remembered across data refreshes.

### By Tier

Within each plan group, tasks are further grouped into **Tier containers** based on their execution tier (dependency depth). Tier 1 tasks have no dependencies; Tier 2 tasks depend on Tier 1 tasks; and so on.

- Tiers let you see "what can run in parallel" at a glance — all tasks in the same tier can potentially execute simultaneously
- Within a collapsed plan group, its tiers are also collapsed
- Tiers can be expanded/collapsed independently; use **Expand All / Collapse All** in the tier group header to manage all tiers in a plan at once

### Ungrouped Tasks

Tasks not associated with any plan are shown in a special **Uncategorized** group. This group is controlled by the "Uncategorized" toggle in the Grouping options (only available when "By Plan" is active).

### Grouping Options

| Option | Description | Default |
|--------|-------------|---------|
| **By Plan** | Group tasks into plan containers | On |
| **By Tier** | Sub-group by execution tier | On |
| **Uncategorized** | Show tasks without a plan | On |
| **None** | Flat list, no grouping | Off |

---

## Timeline Panel

The right panel of the Graph View shows the **Execution Timeline** — a chronological log of all task events in the project.

### What It Shows

Each entry in the timeline represents a significant event: a status change, an agent action, a review, an escalation, a merge. Entries are sorted newest-first.

| Entry type | Examples |
|------------|---------|
| **Execution events** | Task started, step completed, task finished |
| **Review events** | Review started, approved, changes requested |
| **Escalation events** | Escalated to human, decision made |
| **QA events** | QA started, QA passed/failed |
| **Merge events** | Merge started, merged, conflict detected |
| **Plan events** | Plan created, tasks added |

### Filtering the Timeline

Click the **filter icon** in the timeline header to filter events by category. Filters apply immediately. The available categories match the event types above.

### Graph ↔ Timeline Interaction

Clicking a timeline entry **highlights the corresponding node** in the graph with a distinct ring, and centers the viewport on it. This lets you quickly locate where in the graph a particular event occurred.

---

## Filtering

The **floating filter panel** (left side of the canvas, vertically centered) controls what you see on the graph. It has four controls:

### Status Filter

Click **Status** (funnel icon) to filter nodes by status. When active, the button shows the count of selected statuses in orange (e.g., `Status (3)`).

- Statuses are grouped into categories: Idle, Blocked, Executing, QA, Review, Merge, Complete, Terminal
- Click a category header to toggle all statuses in that category at once
- Individual statuses can be toggled independently
- **Clear all** removes all status filters (shows everything)
- Filtered-out nodes and their edges disappear from the graph

### Layout Direction

Toggles between **TB** (top-to-bottom) and **LR** (left-to-right) layout. The graph re-layouts immediately.

### Node Mode

Toggles between **Standard** and **Compact** node display:

- **Standard** — full node with title, status badge, step dots
- **Compact** — minimal node, title only; better for large graphs
- **Auto (orange badge)** — compact was triggered automatically because a group has 8+ tasks; clicking Standard overrides this

### Grouping

Click the **layers icon** to open the grouping dropdown. Combine "By Plan" and "By Tier" for the richest layout, or switch to "None" for a flat graph.

### Show Archived

A checkbox at the bottom of the Status filter panel toggles whether archived tasks are fetched and displayed. Off by default.

---

## Active Plan Context

The Graph View is **plan-scoped**: it only shows tasks belonging to the currently active plan.

### No Plan Selected

If no plan is active, the graph shows a "No plan selected" state with a **plan selector** to choose one.

### Active Plan Indicator

When a plan is active, a **PlanSelectorInline** button appears at the top-center of the canvas showing the current plan name. Click it to switch plans, or press `P` to open the plan quick-switcher palette.

### Switching Plans

Switching the active plan instantly updates the graph to show only tasks from the new plan, and centers the viewport on the new plan's group. The same plan is active across both Graph View and Kanban — they share plan state.

---

---

## Graph + Timeline Layout

The Graph View uses a split layout:

| Panel | Content |
|-------|---------|
| **Left (main)** | Graph canvas with filter controls |
| **Right** | Execution Timeline |

Selecting a node opens the Agents-owned task detail overlay over the artifact.
The Graph layout itself keeps the timeline as its only right-side content.

- On wide screens, the timeline is shown beside the graph.
- On compact/narrow screens, the timeline opens as an overlay.
- Use the surrounding Agents conversation to inspect or message the active agent.

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `↑` / `↓` / `←` / `→` | Navigate between nodes |
| `Enter` | Open the Agents task detail overlay for the focused node |
| `Space` | Select focused node |
| `Escape` | Deselect / close detail panel |
| `Delete` / `Backspace` | Delete selected task or plan group (with confirmation) |
| `+` / `-` | Zoom in / out |
| `0` | Fit view (show all nodes) |

---

## Tips & Best Practices

**Find your bottleneck:** The orange critical-path edges trace the longest dependency chain. Any delay on a critical-path task delays everything that follows. Focus execution resources on critical-path tasks to accelerate completion.

**Use Tier grouping to parallelize:** All tasks in the same tier have no unsatisfied dependencies between them. If multiple Tier N tasks are in Backlog, they can all be started simultaneously.

**Compact mode for big plans:** For plans with 20+ tasks, switch to Compact mode (or let it auto-engage). The graph fits more tasks on screen without scrolling.

**Status filters for focus:** If you only care about what's failing or blocked, filter to just those statuses. The graph becomes a targeted view of problems only.

**Timeline as a progress log:** The Timeline Panel is your project journal. Scroll back to see the sequence of agent actions, reviews, and state changes in chronological order — useful for debugging or reviewing what happened during a long execution run.

**Right-click for quick actions:** Right-clicking a node is faster than opening the full detail overlay for common actions like approving, blocking, or starting execution. No need to navigate away from the graph.

**Plan groups collapse for navigation:** On large projects with multiple plans, keep non-active plans collapsed. The heuristic collapses all-complete groups automatically — you only see work that's still in progress by default.

---

## See Also

- [Kanban Board](kanban.md)
- [Ideation Studio](ideation-studio.md)
- [Task State Machine](task-state-machine.md)
