# Kanban Board User Guide

RalphX's Kanban board is the primary workspace for managing tasks through their entire lifecycle — from ideation through execution, review, and merge. This guide explains the board layout, how to create and manage tasks, what each card element means, and how to get the most out of the board.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| How do I create a task from the Kanban? | Click the **+** button in the Backlog or Ready column header, or use the **New Task** action. |
| How do I move a task to a different state? | Drag and drop the card to the target column, or open the task and use the **Actions** menu. |
| What are the valid moves from a given state? | The drag-and-drop system only allows valid transitions. See [Valid Transitions](#valid-transitions). |
| How do I filter by plan? | Use the **Plan Switcher** in the top bar to activate a plan — the board shows only tasks for that plan. |
| How do I see only certain states? | Use the **Filters** panel (funnel icon) to filter by status, assignee, or other criteria. |
| What happens when I click a task card? | The **Agents task detail overlay** opens, showing the state-specific views for the task. |
| How do I change the columns? | Columns are driven by your project's active **workflow/methodology**. Configure it in project settings. |

---

## Table of Contents

1. [Overview](#overview)
2. [Board Layout](#board-layout)
   - [Column Groups](#column-groups)
   - [State-to-Column Mapping](#state-to-column-mapping)
3. [Task Cards](#task-cards)
   - [Card Elements](#card-elements)
   - [Status Pills](#status-pills)
   - [Priority Indicators](#priority-indicators)
4. [Creating Tasks](#creating-tasks)
5. [Managing Tasks](#managing-tasks)
   - [Drag and Drop](#drag-and-drop)
   - [Valid Transitions](#valid-transitions)
   - [Quick Actions](#quick-actions)
6. [Task Detail Views](#task-detail-views)
   - [The 12 Specialized Views](#the-12-specialized-views)
7. [Filtering and Search](#filtering-and-search)
   - [Filter Panel](#filter-panel)
8. [Active Plan](#active-plan)
9. [Workflows and Methodologies](#workflows-and-methodologies)
10. [Tips and Best Practices](#tips-and-best-practices)

---

## Overview

The Kanban board is the task artifact inside **Agents**. It is the real-time
view of every task in the project. Each column represents a group of task
states; cards move through columns as work progresses. The board is live — it
updates automatically as agents execute, review, and merge tasks.

### High-Level Lifecycle

```
Backlog → Ready → Planned → Executing → Pending Review → Reviewing → Approved → Pending Merge → Merging → Merged
```

Every state transition is handled by the RalphX backend state machine — drag-and-drop on the board triggers the same validated transitions that agents use. You can't accidentally put a task in an invalid state.

---

## Board Layout

### Column Groups

The board is divided into named columns. Each column corresponds to one or more task states from the [24-state task lifecycle](#state-to-column-mapping). The visible columns and their names depend on your project's active **workflow** — the default layout maps directly to the task lifecycle phases.

| Column | Phase | Description |
|--------|-------|-------------|
| **Backlog** | Discovery | Tasks captured but not yet scheduled |
| **Ready** | Scheduling | Tasks queued for the next available execution slot |
| **Planned** | Preparation | Tasks in an active plan, awaiting scheduling |
| **Executing** | Execution | Agent actively implementing the task |
| **In Review** | Review | AI or human reviewing the implementation |
| **Approved** | Pre-merge | Implementation accepted, waiting for merge pipeline |
| **Merging** | Merge | Code being merged into the target branch |
| **Done** | Complete | Successfully merged tasks |
| **Paused / Stopped** | Halted | Tasks that have been paused or manually stopped |
| **Failed** | Failure | Tasks that failed after retry exhaustion |

> Column names and groupings can be customized via the project's workflow settings. See [Workflows and Methodologies](#workflows-and-methodologies).

### State-to-Column Mapping

The 24 task states map to columns as follows:

| State | Default Column | Agent active? |
|-------|---------------|---------------|
| `backlog` | Backlog | No |
| `ready` | Ready | No (slot pending) |
| `planned` | Planned | No |
| `executing` | Executing | Yes |
| `re_executing` | Executing | Yes |
| `qa_refining` | Executing | Yes |
| `qa_testing` | Executing | Yes |
| `qa_passed` | Executing | No (auto-transition) |
| `qa_failed` | Executing | No (auto-transition) |
| `pending_review` | In Review | No (auto-transition) |
| `reviewing` | In Review | Yes |
| `review_passed` | In Review | No (awaiting you) |
| `escalated` | In Review | No (awaiting you) |
| `revision_needed` | In Review | No (auto-transition) |
| `approved` | Approved | No |
| `pending_merge` | Merging | No |
| `merging` | Merging | Yes |
| `merge_incomplete` | Merging | No (needs attention) |
| `merge_conflict` | Merging | No (needs attention) |
| `merged` | Done | No |
| `paused` | Paused | No |
| `stopped` | Stopped | No |
| `failed` | Failed | No |
| `cancelled` | (hidden) | No |

---

## Task Cards

Each card on the board represents one task. Cards show the most important context at a glance, so you can understand the task's status, priority, and progress without opening it.

### Card Elements

| Element | What it shows | Example |
|---------|--------------|---------|
| **Title** | Task name | "Implement OAuth login flow" |
| **Status badge** | Current task state with color coding | `Executing` (blue) · `Review Passed` (green) · `Merge Conflict` (red) |
| **Priority indicator** | P1/P2/P3 dot or label | `●` (critical) |
| **Branch name** | Git branch for this task | `ralphx/myapp/task-abc123` |
| **Execution progress** | Step count during execution | `3/7 steps` |
| **Merge phase** | Current phase during merge | `Validation` |
| **Blocking badge** | Dependent tasks waiting on this one | `Blocks 2` |
| **Blocked badge** | Tasks this one is waiting on | `Blocked by 1` |
| **Plan tag** | Which ideation plan this task belongs to | `Auth Overhaul` |

### Status Pills

Each task card has a colored status pill that instantly communicates the task's state:

| Color | States | Meaning |
|-------|--------|---------|
| **Gray** | `backlog`, `planned` | Not yet active |
| **Blue** | `ready`, `executing`, `re_executing`, `qa_*` | Running or queued |
| **Purple** | `pending_review`, `reviewing`, `review_passed`, `escalated` | Review phase |
| **Orange** | `approved`, `pending_merge` | Waiting for merge |
| **Teal** | `merging` | Actively merging |
| **Green** | `merged` | Complete |
| **Yellow** | `paused`, `stopped` | Halted |
| **Red** | `failed`, `merge_conflict`, `merge_incomplete` | Needs attention |

### Priority Indicators

| Priority | Display | Meaning |
|----------|---------|---------|
| **Critical** | Red dot `●` | Must be addressed immediately |
| **High** | Orange dot `●` | Important, schedule soon |
| **Medium** | Yellow dot `●` | Normal priority (default) |
| **Low** | Gray dot `●` | Nice to have, schedule when convenient |

Priority affects task ordering within a column. Higher-priority tasks are shown first.

---

## Creating Tasks

You can create tasks directly from the Kanban board without going through ideation.

### Manual Task Creation

1. Click the **+** icon in any column header (typically Backlog or Ready).
2. Enter the task title in the inline input.
3. Press Enter to create — the task appears in that column at the top.
4. Click the task to open the detail view and fill in a full description, priority, and plan assignment.

### From Ideation

Tasks created through the Ideation Studio automatically appear in the **Planned** or **Backlog** column depending on their initial state. See the Ideation Studio guide for details on how chat-created tasks flow to the board.

### Task Fields on Creation

| Field | Required? | Notes |
|-------|-----------|-------|
| **Title** | Yes | Short, imperative description of the work |
| **Description** | Recommended | Context for the agent — the more detail, the better the output |
| **Priority** | No | Defaults to Medium |
| **Plan** | No | Assign to an ideation plan to group related tasks |
| **Dependencies** | No | Add tasks this one must wait for before executing |

---

## Managing Tasks

### Drag and Drop

Drag any card to a new column to trigger a state transition. The board:

- **Shows valid targets** — Columns that the task can transition to are highlighted as you drag.
- **Blocks invalid targets** — Columns that aren't valid for that task's current state reject the drop.
- **Applies immediately** — The transition is submitted to the backend on drop. The state machine validates it; if rejected, the card snaps back.

> Drag-and-drop for **priority** reordering is also available within a column. Drag a card up or down within its column to change its relative priority.

### Valid Transitions

The backend enforces which transitions are valid from each state. The key user-initiated transitions are:

| From | To | How |
|------|----|-----|
| `backlog` | `ready` | Drag to Ready column, or "Move to Ready" action |
| `backlog` | `planned` | Assign to a plan |
| `ready` | `backlog` | Drag back to Backlog |
| `ready` | `executing` | Automatic (scheduler picks it up) |
| `review_passed` | `approved` | **Approve** button in detail view |
| `review_passed` | `revision_needed` | **Request Changes** in detail view |
| `escalated` | `approved` | **Approve** button |
| `escalated` | `revision_needed` | **Request Changes** |
| `merge_incomplete` | `pending_merge` | **Retry Merge** in detail view |
| `merge_conflict` | `pending_merge` | **Retry Merge** |
| `executing` | `paused` | **Pause** action |
| `paused` | (previous state) | **Resume** action |
| Any | `stopped` | **Stop** action |
| `stopped` | (restart) | **Restart** action |
| `failed` | `ready` | **Retry** action |

For the full list of 24 states and all transitions, see [task-state-machine.md](task-state-machine.md).

### Quick Actions

Right-click any card (or open the card's context menu) for quick actions without opening the full detail view:

- **Pause / Resume** — Halt or continue execution
- **Stop** — Halt the task (requires manual restart)
- **Retry** — Re-queue a failed task
- **Cancel** — Cancel the task permanently
- **Set Priority** — Change priority inline
- **Open Branch** — Jump to the git branch in your terminal

---

## Task Detail Views

Click any task card to open the **Agents task detail overlay** — a full-screen panel showing all task context and state-specific controls.

### The 12 Specialized Views

RalphX shows a different detail view depending on the task's current state. Each view is optimized for the information and actions relevant to that phase:

| State(s) | View | What you see |
|----------|------|-------------|
| `backlog`, `ready`, `planned` | **BasicTaskDetail** | Title, description, priority, plan, dependencies, history |
| `executing`, `re_executing` | **ExecutionTaskDetail** | Live agent conversation, step progress timeline, worktree info, supervisor alerts |
| `qa_refining`, `qa_testing` | **QaTaskDetail** | QA acceptance criteria, browser test output, pass/fail results |
| `qa_failed` | **QaFailedDetail** | Failed test details, criteria, transition to revision |
| `pending_review`, `reviewing` | **ReviewingTaskDetail** | Reviewer agent conversation, review criteria checklist |
| `review_passed`, `escalated` | **ReviewPassedDetail** | AI review summary, structured issues list, **Approve** / **Request Changes** buttons |
| `revision_needed`, `re_executing` | **RevisionDetail** | Reviewer feedback, issue list by severity, re-execution progress |
| `approved` | **ApprovedDetail** | Approval summary, branch info, merge pipeline entry status |
| `pending_merge` | **PendingMergeDetail** | Merge progress phases (real-time), source→target branch flow, deferral status |
| `merging` | **MergingTaskDetail** | Merger agent conversation, branch flow, conflict files (if any), validation banner |
| `merge_incomplete`, `merge_conflict` | **MergeIncompleteDetail** | Error context, conflict files, ConflictDiffViewer, **Retry Merge** / **Retry (Skip Validation)** |
| `merged` | **MergedDetail** | Merge commit SHA, deleted branch tag, commit history, validation history |

> **ConflictDiffViewer** — In `merge_conflict` state, click any conflict file in the detail view to open the inline diff viewer showing the conflict markers.

---

## Filtering and Search

### Filter Panel

Click the **funnel icon** (top-right of the board) to open the filter panel. Active filters are indicated by a badge on the icon.

| Filter | Options | Effect |
|--------|---------|--------|
| **Plan** | Any active plan, or "All Plans" | Show only tasks belonging to the selected plan |
| **Status** | Any subset of the 24 states | Show only tasks in those states |
| **Assignee** | Agent ID or "Unassigned" | Filter by which agent is currently handling the task |
| **Priority** | Critical / High / Medium / Low | Show only tasks at the selected priority levels |

Filters are additive — selecting Status: Executing AND Priority: High shows only executing high-priority tasks.

### Plan Quick Switcher

The **PlanQuickSwitcher** in the top bar provides a fast way to switch which plan's tasks you see. Unlike the full filter panel, the plan switcher is a single-click toggle:

- **Select a plan** — Board shows only tasks from that ideation session
- **"All Plans"** — Board shows tasks from all plans (default)

The active plan selection is persisted — navigating away and back preserves your plan filter. See [Active Plan](#active-plan) for more detail.

---

## Active Plan

The Active Plan feature lets you focus the board on a single ideation session's tasks. This is especially useful when you have multiple plans in parallel (e.g., "Auth Overhaul" and "Billing Revamp") and want to see the progress of just one.

### How It Works

1. **Activate a plan** — Use the plan switcher in the Agents Tasks artifact or Graph tab.
2. **Board filters automatically** — Only tasks linked to that plan appear on the Kanban.
3. **Column counts reflect the plan** — Task counts in column headers are scoped to the active plan.
4. **Deactivate** — Switch to "All Plans" to return to the full board.

### Plan Task States

Within an active plan, tasks progress from execution on individual task branches through to a **plan-merge task** that merges the entire plan branch into `main`. The plan-merge task appears as a special card in the Merging column when all plan tasks are complete.

For a full explanation of plan branching and plan-merge tasks, see the [Merge Pipeline User Guide](merge.md#branch-management).

---

## Workflows and Methodologies

RalphX's Kanban columns are driven by the project's active **workflow**. Workflows define which columns appear, what they're called, and which task states they map to.

### Default Workflow

The default workflow maps directly to the RalphX task lifecycle phases with one column per major phase (Backlog → Ready → Executing → Review → Merging → Done).

### Custom Methodologies

You can configure alternative methodologies in project settings:

| Methodology | Description |
|-------------|-------------|
| **Default** | Full lifecycle visible — all phases shown |
| **Simplified** | Collapsed view — fewer columns for smaller teams |
| **Agile Sprint** | Sprint-oriented columns with backlog, sprint, and done groupings |

Column customization options:
- Rename columns
- Hide columns not relevant to your workflow
- Group multiple states into a single column
- Reorder columns

> Changes to the workflow affect the board display only — the underlying state machine and valid transitions are unchanged.

---

## Tips and Best Practices

### Keep the Backlog Groomed

Tasks in Backlog are not scheduled until you move them to Ready. Regularly review your Backlog to:
- Prioritize upcoming work
- Add descriptions so agents have full context when execution starts
- Assign dependencies before execution begins (prevents partial merges)

### Use Plans for Related Work

Group related tasks under an ideation plan. This gives you:
- Focused board view via the plan filter
- Coordinated branching (all tasks merge to the plan branch first)
- A single plan-merge task to land everything at once

### Watch the "Needs Attention" Cards

Cards in `merge_conflict` or `merge_incomplete` have red badges and require your action. Check the board daily for red cards — they block the merge pipeline for those tasks.

### Approve Promptly After AI Review

Tasks in `review_passed` are waiting for your decision. The longer they wait, the more the task branch diverges from `main`, increasing the chance of merge conflicts. Approve or request changes as soon as you see the notification.

### Add Context Before Retrying

When retrying a failed or stuck task, add a **restart note** in the task detail view before clicking Retry. The restart note is appended to the agent's next prompt (one-shot) — use it to explain what went wrong or what to try differently.

### Use Drag-and-Drop for Priority, Not State

While drag-and-drop supports state transitions, for most state changes it's cleaner to use the action buttons in the task detail view (Approve, Retry, etc.) — they're explicit and provide confirmation. Reserve drag-and-drop for priority reordering within a column, where the directness is an advantage.

---

## See Also

- [Graph View](graph-view.md)
- [Task State Machine](task-state-machine.md)
- [Execution Pipeline](execution.md)
