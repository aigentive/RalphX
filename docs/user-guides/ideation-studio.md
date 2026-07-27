# Ideation Studio User Guide

## Agent conversation planning

Use **Plan** for supervised native planning. After approving the exact current
draft, choose **Create Proposals** to enter **Tasks** and review decomposition.
No execution begins until you choose **Start Tasks**.

The **Tasks** artifact stays attached to the same conversation, branch, and pull request. After
execution unlocks you can return to it, and you can request a small follow-up on
the same open PR without creating another plan. Closed or merged work starts
again in Plan.

**Autopilot** is an optional Agent capability in Settings. It allows native
conversations to plan and start orchestration with minimal supervision. It is
hidden and rejected by native APIs by default; external MCP integrations retain
their existing autonomous flow.

Ideation is embedded in the Agents workspace. Plan, proposal, and task
artifacts stay attached to the conversation that created them; there is no
separate Ideation root route.

The Ideation Studio is where every feature in RalphX begins. You describe what you want to build, and a team of AI agents researches your codebase, designs an implementation plan, and creates a set of ready-to-execute tasks. Those tasks then flow automatically through execution, review, and the merge pipeline — turning an idea into merged code with minimal manual intervention.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| How do I start? | Open **Agents**, start a conversation, and choose **Plan** to describe what you want to build. |
| Which mode should I choose? | **Solo** for quick fixes and simple features. **Research Team** for anything touching 2+ layers. **Debate Team** for architecture decisions. |
| What happens after I approve the plan? | RalphX creates proposals for review in the conversation's **Tasks** artifact; execution begins only after you choose **Start Tasks**. |
| Can I edit a proposal before tasks are created? | Yes — review and revise proposals in **Tasks** after **Create Proposals**, before choosing **Start Tasks**. |
| Can I message individual team members? | Yes — use the Team Activity panel to send a message to the lead or any specialist directly. |
| What is an Active Plan? | A focused filter. Selecting an accepted plan makes the embedded task views show only tasks from that plan. |
| Where do tasks go after I choose **Start Tasks**? | Into the execution pipeline: Pending → Executing → QA → Review → Approved → PendingMerge → Merged. |
| What's the full journey? | Agents → Plan → Tasks → Start Tasks → Execution → Review → **Merge Pipeline** → Done. |

---

## Table of Contents

1. [Overview](#overview)
2. [Starting an Ideation Session](#starting-an-ideation-session)
3. [The Orchestrator Workflow](#the-orchestrator-workflow)
4. [The Plan Artifact](#the-plan-artifact)
   - [Reviewing the Plan](#reviewing-the-plan)
6. [Proposals and the CONFIRM Gate](#proposals-and-the-confirm-gate)
7. [Accepting the Plan and Creating Tasks](#accepting-the-plan-and-creating-tasks)
8. [Active Plan: Tracking Your Work](#active-plan-tracking-your-work)
9. [The Downstream Journey](#the-downstream-journey)
   - [Execution Pipeline](#execution-pipeline)
   - [Review Cycle](#review-cycle)
   - [Merge Pipeline](#merge-pipeline)
10. [End-to-End Flow Diagram](#end-to-end-flow-diagram)
11. [Troubleshooting](#troubleshooting)
12. [Configuration Reference](#configuration-reference)

---

## Overview

RalphX structures feature development as a gated pipeline. The Ideation Studio is the entry point — the stage where a human idea becomes a concrete, costed, dependency-ordered set of tasks:

```
You describe a feature
        |
        v
  Agents workspace
        |
        v
  Plan artifact
        |
        v
  Tasks artifact
  (Active Plan; Kanban / Graph modes)
        |
        v
  Execution  →  Review  →  Merge Pipeline  →  Merged Code
```

The AI orchestrator does the research and planning work, using bounded exploration delegates when useful. You stay in control through two hard checkpoints: the **CONFIRM gate** (approve the plan before proposals are created) and the **Review gate** (approve code before it merges).

---

## Starting an Ideation Session

### How to Start

1. Open **Agents** in the left sidebar
2. Start a new conversation and choose **Plan**
3. Describe what you want to build in the composer
4. Review the generated plan in the conversation's **Plan** artifact
5. Approve the current plan, choose **Create Proposals**, and review the decomposition in its **Tasks** artifact

The orchestrator begins immediately. You will see the session's chat panel fill with activity as it researches your codebase.

## The Orchestrator Workflow

Whether you use Solo or Team mode, the orchestrator follows a gated workflow with 6 active phases (UNDERSTAND through FINALIZE), preceded by a RECOVER phase (Phase 0) that runs on every session start or resume. Each phase must complete before the next begins.

### Phase 0: RECOVER

On session start (or resume), the orchestrator loads existing session state: prior plan artifact, prior proposals, parent context, team artifacts. If the session is new, this phase is near-instant. If you are resuming an interrupted session, the orchestrator reconstructs its context before continuing.

### Phase 1: UNDERSTAND

The orchestrator parses your intent, determines complexity, and chooses any useful reasoning lenses.

### Phase 2: EXPLORE

The orchestrator can launch bounded read-only exploration delegates to investigate independent parts of the codebase in parallel.

Research agents have read-only access (no file writes). They use Read, Grep, Glob, Bash, WebFetch, and WebSearch.

You can watch research progress in real time in the session chat panel.

### Phase 3: PLAN

The orchestrator synthesizes all research findings into a structured implementation plan and publishes it as a **plan artifact**.

The plan artifact is a versioned document — if you reject the plan and ask for changes, a new version is created while prior versions are preserved.

### Phase 4: CONFIRM

**This is the first human checkpoint.** The orchestrator presents the plan to you and waits for explicit approval. It will never create proposals until you approve.

You can:
- **Approve** the plan → moves to PROPOSE
- **Request changes** → the orchestrator revises the plan and returns to CONFIRM
- **Reject** the plan entirely → start over or end the session

### Phase 5: PROPOSE

After you approve the plan, the orchestrator creates **task proposals** — one per implementation task. Each proposal includes:
- Title and detailed description
- Estimated effort
- Dependencies on other proposals (auto-suggested)
- Links to the plan artifact

You can review, edit, add, or delete proposals before they become tasks.

### Phase 6: FINALIZE

The orchestrator performs dependency analysis, determines the critical path, and prepares the session for acceptance. Once finalized, you can accept the session to convert proposals into live tasks.

---

## The Plan Artifact

The plan artifact is the structured output of the PLAN phase. It is a versioned document stored in the database and linked to all proposals that stem from it.

### Reviewing the Plan

The plan is presented in the session chat panel at the CONFIRM phase. It includes:
- **Architecture overview** — how the feature fits into the existing codebase
- **Implementation approach** — the strategy chosen and why
- **Task breakdown** — how the work is split across proposals
- **Dependencies** — sequencing requirements between tasks

## Proposals and the CONFIRM Gate

After the plan is approved, the orchestrator creates proposals — one per task. Proposals are shown in the session detail view with their full content, estimated effort, and dependency suggestions.

### Reviewing Proposals

Before accepting the session you can:
- **Edit** any proposal's title, description, or estimated effort
- **Delete** proposals you don't want to implement
- **Add** proposals manually if you want work items the orchestrator missed
- **Reorder** proposals (affects the suggested execution sequence)
- **Review and edit proposal dependencies** in the dependency graph view

### The CONFIRM Guarantee

> The orchestrator will **never** create proposals before receiving explicit approval at the CONFIRM phase. This is enforced at the agent level — it cannot skip the gate.

If you close the app or navigate away during the CONFIRM phase, the session remains in the CONFIRM state. When you return, the orchestrator resumes from the CONFIRM checkpoint rather than re-running the PLAN phase.

---

## Accepting the Plan and Creating Tasks

When you are satisfied with the proposals in **Tasks**, choose **Start Tasks** to convert the plan into live tasks and begin scheduling.

What happens on Start Tasks:
1. Each proposal becomes a **Task** in the database
2. Dependency edges from the proposal graph are preserved as task dependencies
3. The session status changes to `accepted`
4. The session is **automatically set as the Active Plan** for the project
5. Tasks appear immediately in the conversation's **Tasks** artifact; switch between its **Kanban** and **Graph** modes
6. Ready tasks (no unmet dependencies) are scheduled for execution

---

## Active Plan: Tracking Your Work

After **Start Tasks**, the ideation session becomes your **Active Plan** — a focused filter that makes the **Tasks** artifact's Kanban and Graph modes show only the tasks from that plan.

### Switching Plans

| Method | How |
|--------|-----|
| **Inline selector** | Click the plan selector in the Tasks artifact's Kanban toolbar or Graph controls |
| **Quick switcher** | Press `Cmd+Shift+P` (Mac) / `Ctrl+Shift+P` (Windows/Linux) |

Plans are ranked by your interaction frequency, active task count, and recency — plans you actively work on appear at the top automatically.

### Active Plan Lifecycle

| Event | Effect on Active Plan |
|-------|----------------------|
| Start Tasks (session accepted) | Auto-set as active plan |
| Reopen accepted session (re-ideate) | Active plan is cleared; the conversation's Tasks artifact shows an empty state |
| Manually switch to another plan | New plan becomes active; old plan loses filter |
| Clear selection | No active plan; the Tasks artifact's views show empty state |

---

## The Downstream Journey

Once tasks are created from the accepted plan, they flow automatically through RalphX's execution, review, and merge pipeline.

### Execution Pipeline

```
Pending
   │
   v
Executing  ←── Worker agent (ralphx-execution-worker) orchestrates implementation
   │             └── Delegates to coder agents (ralphx-execution-coder) in parallel waves
   v
QA          ←── QA prep agent generates acceptance criteria;
   │              QA executor runs browser tests
   v
Review      ←── Reviewer agent runs automated code review
```

- The **worker agent** decomposes the task into sub-tasks and delegates to up to 3 parallel coder agents
- Tasks with unmet dependencies remain in **Pending** until their dependencies reach **Merged** or **Cancelled**
- You can message a task directly via the task chat panel to provide direction or corrections at any stage

### Review Cycle

When a task reaches **Review**, the `ralphx-execution-reviewer` agent performs a structured code review and produces a list of findings. You see the review in the task detail view.

| Review outcome | What happens |
|----------------|--------------|
| Review passes (no critical issues) | Task moves to **Review Passed** — awaiting your approval |
| Review fails | Task may re-execute to address findings |
| You approve | Task transitions to **Approved** → immediately enters the merge pipeline |
| You reject | Task re-enters execution for another implementation cycle |

### Merge Pipeline

Once a task is **Approved**, it enters the merge pipeline automatically. The pipeline handles everything:

1. **Preparation** — Resolves source and target branches
2. **Branch freshness** — Ensures branches are up-to-date (merges main into plan branch if behind)
3. **Programmatic merge** — Attempts the merge using your project's configured strategy (default: RebaseSquash)
4. **Validation** — Runs your project's test/lint/typecheck commands (default mode: Block — reverts on failure)
5. **Finalization** — Commits the merge, deletes the task branch and worktree

If a conflict arises, a **merger agent** (`ralphx-execution-merger`) is spawned to resolve it. If validation fails in AutoFix mode, a **fixer agent** is spawned to repair the code.

> For complete details on the merge pipeline — states, strategies, recovery, and UI — see the **[Merge Pipeline User Guide](./merge.md)**.

---

## End-to-End Flow Diagram

```
You describe a feature
         │
         v
  ┌──────────────────────────────────────────────┐
  │              Agents workspace                │
  │  ┌─────────────────────────────────────────┐ │
  │  │  Plan: UNDERSTAND → EXPLORE → PLAN      │ │
  │  │  (orchestrator or team lead + teammates)│ │
  │  └─────────────────────────────────────────┘ │
  │                    │                         │
  │                    v                         │
  │           CONFIRM GATE (you approve plan)    │
  │                    │                         │
  │                    v                         │
  │  ┌─────────────────────────────────────────┐ │
  │  │  PROPOSE → FINALIZE                     │ │
  │  │  (orchestrator creates proposals)       │ │
  │  └─────────────────────────────────────────┘ │
  └──────────────────────────────────────────────┘
         │
         v  (you choose Create Proposals)
  ┌──────────────────────────────────────────────┐
  │              Tasks artifact                  │
  │  Review → Start Tasks                        │
  │  Kanban + Graph are embedded modes           │
  └──────────────────────────────────────────────┘
         │
         v
  ┌──────────────────────────────────────────────┐
  │             Execution Pipeline               │
  │   Pending → Executing → QA → Review          │
  │   (worker + coder agents implement the code) │
  └──────────────────────────────────────────────┘
         │
         v  (you click Approve in Review)
  ┌──────────────────────────────────────────────┐
  │             Merge Pipeline                   │
  │   Approved → PendingMerge → Merged           │
  │   (merger agent resolves any conflicts)      │
  └──────────────────────────────────────────────┘
         │
         v
    Code is on the target branch ✓
```

### Plan Branch Flow

For tasks that belong to an ideation plan, RalphX uses a **three-level branch hierarchy**:

```
main
  └── plan/feature-auth          ← Plan branch (one per ideation session)
        ├── ralphx/project/task-abc123   ← Task branch
        ├── ralphx/project/task-def456   ← Task branch
        └── ralphx/project/task-ghi789   ← Task branch
```

Each task merges into the plan branch. When all tasks in the plan are merged, RalphX automatically creates a final **plan-merge task** that appears in the Tasks artifact's **Kanban** mode and merges the plan branch into `main` once all sibling tasks reach Merged or Cancelled. This approach prevents partial feature merges and allows the full plan to be validated as a unit before touching `main`.

---

## Troubleshooting

### Session stuck in EXPLORE phase

**What it means:** The research agents are taking longer than expected, or one failed silently.

**What to do:**
1. Check the session chat for error messages from the orchestrator
2. You can message the orchestrator to ask for a status update
3. In RX-native Team mode, check the delegated task cards and activity stream for stalled work

### Orchestrator asked a question but I didn't answer

**What it means:** The `ask_user_question` MCP tool was called, surfacing a question in the session chat. The orchestrator is waiting for your reply before proceeding.

**What to do:** Answer in the session chat panel. The orchestrator resumes automatically when it receives your reply.

### Plan was approved but no proposals appeared

**What it means:** The PROPOSE phase may still be running, or a proposal creation error occurred.

**What to do:**
1. Wait a moment — proposal creation is usually fast but not instant
2. Refresh the session detail view
3. If proposals still don't appear, message the orchestrator: "Please create the proposals now"

### I want to change the plan after accepting

**What it means:** You accepted the session and tasks were created, but you now want a different approach.

**What to do:**
1. Open **Agents** and select the conversation that owns the accepted plan
2. Open the plan artifact and choose **Reopen**
3. The active plan is automatically cleared — the Tasks artifact reflects the empty state
4. Chat with the orchestrator to revise the plan; the existing task proposals are preserved for reference
5. Choose **Start Tasks** again when ready — new tasks are created

> **Note:** Reopening a session does not automatically cancel tasks that were already created. Cancel unwanted tasks manually from the Kanban board.

### Session recovery after app restart

If the app closes during an ideation session, it recovers automatically when you reopen it:
- **UNDERSTAND / EXPLORE phase**: The orchestrator re-reads its session context and resumes research
- **PLAN / CONFIRM phase**: The plan artifact is persisted; the orchestrator resumes from the last completed phase
- **Team mode**: The lead re-reads persisted team state (team composition, team artifacts, prior findings) and re-spawns teammates with context injection. Each teammate receives a structured summary of prior research findings (≤4,000 tokens injected context), not the full message history

### "No plan selected" after accepting a session

**What it means:** The active plan was cleared (e.g., another user action or app restart).

**What to do:**
1. Press `Cmd+Shift+P` to open the quick switcher
2. Type the plan name and select it
3. The Tasks artifact's Kanban and Graph modes will filter to that plan immediately

### Validation Commands (Project Settings)

After tasks merge, the merge pipeline validates your code. These are configured at the project level, not the ideation session level. See the [Merge Pipeline User Guide — Configuration Reference](./merge.md#configuration-reference) for details.

### Active Plan Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd+Shift+P` (Mac) / `Ctrl+Shift+P` (Win/Linux) | Open plan quick switcher |
| `↑` / `↓` | Navigate plans in selector |
| `Enter` | Select highlighted plan |
| `Escape` | Close selector |

---

## See Also

- [Kanban Board](kanban.md)
- [Graph View](graph-view.md)
- [Execution Pipeline](execution.md)
