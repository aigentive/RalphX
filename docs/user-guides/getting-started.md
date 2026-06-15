# Getting Started with RalphX

RalphX is a native Mac application for AI-driven software development. You describe features in natural language, and a multi-agent system researches your codebase, writes code, reviews it, and merges it — all in isolated git worktrees that never touch your working directory. This guide walks you through setup, your first project, and your first end-to-end workflow.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| What do I need to install? | macOS, Node.js 18+, Rust 1.70+, and at least one supported agent harness CLI |
| How do I run RalphX? | `cd frontend && npm install && npm run tauri dev` |
| What is a project? | A git repository you've registered with RalphX |
| How do I start building a feature? | Open **Ideation**, click **New Session**, describe what you want |
| Where do I watch tasks run? | **Kanban** — tasks move left-to-right through the execution pipeline |
| Do agents touch my files? | Never — all execution happens in isolated git worktrees |
| What's an Active Plan? | A filter that scopes Kanban and Graph to one ideation session's tasks |
| What happens after I approve a task? | It enters the merge pipeline and is merged to your base branch automatically |

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Creating Your First Project](#creating-your-first-project)
4. [Navigating the Interface](#navigating-the-interface)
5. [Key Concepts](#key-concepts)
6. [Your First Workflow](#your-first-workflow)
7. [Next Steps](#next-steps)

---

## Prerequisites

| Requirement | Minimum version | Notes |
|-------------|----------------|-------|
| macOS | 12 (Monterey) | ARM and Intel supported |
| Node.js | 18 | For frontend build and `npm` commands |
| Rust | 1.70 | Install via [rustup.rs](https://rustup.rs) |
| Claude CLI or Codex CLI | latest | Install at least one supported harness runtime; Claude remains the default |

### Installing a harness CLI

RalphX launches external harness CLIs for ideation, execution, review, and merge flows. Claude remains the default harness, while Codex can be selected for supported lanes.

#### Claude CLI

```bash
# Install via npm
npm install -g @anthropic-ai/claude-code

# Verify
claude --version
```

Agents must authenticate to Anthropic. Run `claude` once interactively to complete the login flow before using RalphX.

#### Codex CLI

```bash
# Verify an existing installation
codex --version
```

If you plan to use Codex-backed lanes, make sure the `codex` CLI is installed and authenticated before selecting it in Settings.

Recommended first rollout:

1. enable Codex for ideation
2. enable Codex for ideation verification
3. leave execution/review/merge on Claude initially

See [Agent Harnesses](agent-harnesses.md) for the current harness matrix and limitations.

---

## Installation

```bash
# Clone the repository
git clone <your-ralphx-repo-url>
cd ralphx

# Install dependencies
cd frontend
npm install

# Start in development mode
npm run tauri dev
```

The first build compiles the Rust backend — this takes 2–5 minutes. Subsequent starts are fast.

### Building a Release Binary

```bash
npm run build
npm run tauri build
```

The resulting `.app` bundle (~10MB) is in `src-tauri/target/release/bundle/macos/`.

### Runtime characteristics

| Metric | Value |
|--------|-------|
| Bundle size | ~10 MB |
| RAM usage (idle) | ~30 MB |
| Database | SQLite at `src-tauri/ralphx.db` |
| Agent worktrees | `~/ralphx-worktrees/` (configurable) |

---

## Creating Your First Project

A **project** in RalphX is a git repository you register so agents can work inside it. On first launch, the project creation wizard opens automatically.

### Wizard walkthrough

**Step 1 — Location**

Click **Browse** and select a folder. The folder must be a git repository with at least one commit. RalphX reads your branches immediately after selection.

- If the folder is not a git repo, a validation error appears and you cannot proceed.
- Project name is inferred from the folder name. Override it if you want a different display name.

**Step 2 — Git Settings**

| Field | What to set | Default |
|-------|------------|---------|
| Base branch | The branch tasks will merge into (usually `main`) | Auto-detected from repo |
| Worktree location | Where agent working directories are created | `~/ralphx-worktrees` |

RalphX detects your repository's default branch and pre-selects it. If you have both `main` and `master`, it prefers `main`.

The worktree path preview shows exactly where task files will be created — for example, `~/ralphx-worktrees/my-app/task-abc123`. Your original checkout is never modified.

**Step 3 — Create**

Click **Create Project**. RalphX validates the repository, creates its internal database record, and opens the main interface.

---

## Navigating the Interface

RalphX has six main views, accessible from the left sidebar.

| View | Path | Purpose |
|------|------|---------|
| **Kanban** | `/kanban` | Primary task board — columns track execution state |
| **Graph** | `/graph` | Dependency graph — see how tasks relate and block each other |
| **Ideation** | `/ideation` | Create features — describe what to build, get a structured plan |
| **Team** | `/team` | Agent activity — see running agents, spawn counts, live output |
| **Activity** | `/activity` | Audit log — full history of every state change and event |
| **Settings** | `/settings` | Project config — merge strategy, concurrency, validation, and agent harness settings |

### Sidebar

The left sidebar shows your projects. Click a project to make it active — Kanban and Graph filter to that project's tasks. The active project is shown in the header.

### Header

The header shows the current project and, if you've selected an Active Plan, its name. The Active Plan filter scopes Kanban and Graph to only the tasks from that ideation session.

---

## Key Concepts

### Tasks

A task is a unit of work: a description, an implementation plan, a git branch, and a lifecycle state. Tasks are created either manually or by the Ideation pipeline.

### 24-State Lifecycle

Tasks move through a defined set of states. The states you'll see most often:

| State | Meaning |
|-------|---------|
| **Backlog** | Not yet scheduled |
| **Ready** | Queued for execution |
| **Executing** | Worker agent is writing code |
| **Reviewing** | AI reviewer is checking the implementation |
| **ReviewPassed** | AI approved — waiting for your decision |
| **Approved** | You approved — task enters the merge pipeline |
| **PendingMerge** | Programmatic merge running |
| **Merged** | Code is on the base branch |

For the full state diagram, see the [Execution Pipeline guide](execution.md) and [Merge Pipeline guide](merge.md).

### Agents

Each stage of the pipeline uses a specialized agent:

| Agent | Stage | Role |
|-------|-------|------|
| ralphx-ideation | Ideation | Researches codebase, produces implementation plan |
| ralphx-execution-worker | Execution | Decomposes task into sub-scopes, delegates to coders |
| ralphx-execution-coder | Execution | Implements a single file-scoped sub-task |
| ralphx-execution-reviewer | Review | Analyzes code quality; recommends approve or changes |
| merger | Merge | Resolves git conflicts if programmatic merge fails |

Agents run via the configured harness for each lane. Claude is still the default runtime for the broadest feature coverage, while Codex can be selected for supported lanes. See [Agent Harnesses](agent-harnesses.md).

### Plans and Active Plan

An ideation session produces a **plan**: an ordered set of tasks with dependencies. When you accept a plan, RalphX creates the tasks and lets you set it as the **Active Plan**. The Active Plan filter scopes Kanban and Graph to just those tasks, giving you a focused view of one feature's progress.

### Merge Strategies

Each project uses one merge strategy for all its tasks:

| Strategy | History | Best for |
|----------|---------|---------|
| **RebaseSquash** (default) | One commit per task, linear | Most projects |
| **Squash** | One commit per task | Projects that don't need rebase |
| **Rebase** | All commits, linear | Preserving individual commits |
| **Merge** | Merge commits visible | Full history |

Change the strategy in **Settings → Git**. It applies to all future merges.

---

## Your First Workflow

This is the end-to-end path from idea to merged code.

### 1. Open Ideation

Navigate to **Ideation** and click **New Session**.

Choose a mode:

| Mode | When to use |
|------|------------|
| **Solo** | Simple, well-scoped tasks |
| **Research Team** | Features touching multiple layers |
| **Debate Team** | Architecture decisions with trade-offs |

### 2. Describe what you want to build

Type a description of the feature. Be specific: include the expected behavior, affected components, and any constraints. The orchestrator reads your codebase and produces a structured plan.

Example: *"Add keyboard shortcut Cmd+K to open the task search panel. Should work from any view and close on Escape or when focus leaves the panel."*

### 3. Review and accept the plan

When the orchestrator finishes, a plan artifact appears with:
- A list of proposed tasks in dependency order
- Estimated scope for each task
- A summary of what will change

If the plan looks correct, click **Accept Plan**. RalphX creates the tasks and offers to set this as your Active Plan.

If the plan needs changes, type feedback in the chat. The orchestrator revises and re-proposes.

### 4. Watch execution on Kanban

Navigate to **Kanban**. Your tasks appear in the **Backlog** or **Ready** column (depending on dependencies). RalphX schedules up to 10 tasks concurrently by default. As agents work, tasks move right:

```
Backlog → Ready → Executing → Reviewing → ReviewPassed → Approved → PendingMerge → Merged
```

Open a task card to see the live agent conversation and step-by-step progress.

### 5. Review and approve

When a task reaches **ReviewPassed**, the AI reviewer has approved the implementation. You receive a notification. Open the task detail view to see:

- The diff of changes made
- The reviewer's summary and any findings
- The full agent conversation

Click **Approve** to send the task to the merge pipeline. Click **Request Changes** to send it back for revision.

### 6. Merge completes automatically

After approval, RalphX handles the rest:

1. Creates a worktree for the merge
2. Runs the merge strategy (default: RebaseSquash — one clean commit)
3. Runs your project's validation commands if configured
4. Merges to your base branch
5. Deletes the task branch and worktree

The task moves to **Merged**. The merge commit SHA is shown in the task detail view.

---

## Next Steps

| Guide | What it covers |
|-------|---------------|
| [Ideation Studio](ideation-studio.md) | Session modes, team configuration, plan artifacts, task creation |
| [Kanban Board](kanban.md) | Board layout, task cards, drag-and-drop transitions, filtering |
| [Graph View](graph-view.md) | Dependency graph, critical path, timeline panel, battle mode |
| [Execution Pipeline](execution.md) | Worker/coder/reviewer agents, concurrency, revision cycles, recovery |
| [Merge Pipeline](merge.md) | Merge strategies, validation, conflict resolution, recovery |
| [Project Skills](project-skills.md) | Learned skills, Promote Memory, import, export, approval, and troubleshooting |
| [Task State Machine](task-state-machine.md) | All 24 states, transitions, and state invariants |
| [Configuration](configuration.md) | Project settings, model config, supervisor, ralphx.yaml reference |
