# Agent Orchestration User Guide

RalphX doesn't execute your tasks directly — it orchestrates a team of specialized AI agents. Each agent has a specific role, a constrained set of tools, and a defined place in the pipeline. This guide explains what agents are, how they work together, and how to interact with them effectively.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| What runs my task? | A **worker** agent orchestrates — it delegates actual coding to parallel **coder** agents. |
| Who reviews my code? | A **reviewer** agent analyzes the diff and calls pass/fail. You then approve or request changes. |
| What handles merge conflicts? | A **merger** agent resolves git conflicts programmatically. That lane is configured in **Settings → General → Execution Agents**. |
| How do agents get their instructions? | Via the task context, plan artifact, and their built-in system prompts. You don't write agent prompts. |
| Can RalphX give agents workflow-specific guidance? | Yes. RalphX can inject allowlisted [internal skills](internal-skills.md) for narrow product workflows. |
| Can I talk to a running agent? | Yes — use the chat panel to send messages to active agents. They receive your messages mid-execution. |
| What if the AI asks me a question? | An in-line **User Question** dialog appears — answer it before the agent continues. |
| What if an agent asks to do something sensitive? | A **Permission Request** dialog appears — you approve or deny the specific action. |
| What is the MCP? | The messaging layer between RalphX agents and the backend. Agents read task context and report progress via MCP tools. |

---

## Table of Contents

1. [Overview](#overview)
2. [Agent Types](#agent-types)
3. [Agent Lifecycle](#agent-lifecycle)
4. [The Execution Pipeline](#the-execution-pipeline)
   - [Worker and Coder Agents](#worker-and-coder-agents)
   - [Supervisor Service](#supervisor-service)
   - [Reviewer Agent](#reviewer-agent)
   - [Merger Agent](#merger-agent)
5. [Ideation Agents](#ideation-agents)
   - [Solo Orchestrator](#solo-orchestrator)
   - [Team Mode: Research Teams and Debate Teams](#team-mode-research-teams-and-debate-teams)
   - [Specialist Agents](#specialist-agents)
6. [QA Agents](#qa-agents)
7. [Communicating with Agents](#communicating-with-agents)
8. [Permission Requests](#permission-requests)
9. [User Questions](#user-questions)
10. [Team View](#team-view)
11. [Session Recovery](#session-recovery)
12. [MCP Architecture](#mcp-architecture)
13. [Agent Tool Scopes](#agent-tool-scopes)
14. [Troubleshooting](#troubleshooting)

---

## Overview

Every action in RalphX is performed by a configured AI agent harness. When you move a task to **Ready**, a worker agent is spawned automatically. When execution finishes, a reviewer agent runs. If there's a merge conflict, a merger agent resolves it. You don't start agents manually — the pipeline handles orchestration based on task state.

Harness selection is lane-based, not app-wide. Claude remains the broadest-coverage default, but execution, review, and merge lanes can also be routed to Codex when you configure them that way.

```
You describe a feature (Ideation Studio)
          |
          v
  ralphx-ideation
  researches, plans, proposes tasks
          |
          v
  Tasks appear on Kanban
          |
          v
  Worker agent executes (with coder sub-agents)
          |
          v
  Reviewer agent reviews
          |
     ┌────┴────┐
  Approved   Changes needed
     |              |
     v              v
  Merger agent   Worker re-executes
  merges to main
```

Agents are ephemeral. They start when a task enters a state that requires them, run until their job is done, and exit. RalphX automatically manages spawning, monitoring, and recovery.

The chat UI surfaces that runtime choice directly:

- conversation history shows harness badges and stored-session routing hints
- the active chat header shows the current harness plus provider-session lineage
- assistant messages show provider metadata when the conversation is continuing on a stored harness session

---

## Agent Types

RalphX uses specialized agents across execution, review, merge, ideation, QA, and support roles.

| Agent | Role | Model | Spawned when |
|-------|------|-------|--------------|
| **ralphx-execution-worker** | Orchestrates task execution; decomposes work and delegates to coders | Sonnet | Task enters Executing |
| **ralphx-execution-coder** | Implements a scoped sub-task with exclusive file ownership | Sonnet | Dispatched by worker |
| **ralphx-execution-reviewer** | Reviews the git diff; approves, requests changes, or escalates | Sonnet | Task enters Reviewing |
| **ralphx-execution-merger** | Resolves git merge conflicts that programmatic rebase couldn't handle | Opus | Task enters Merging with conflicts |
| **ralphx-qa-prep** | Generates acceptance criteria and test steps from the task spec | Sonnet | Task enters Ready (background) |
| **ralphx-qa-refiner** | Adapts test criteria to match the actual implementation | Sonnet | Task enters QaRefining |
| **ralphx-qa-executor** | Runs browser-based acceptance tests | Sonnet | Task enters QaTesting |
| **ralphx-ideation** | Runs solo ideation sessions: research → plan → propose | Sonnet | Ideation session starts (Solo mode) |
| **ralphx-ideation-team-lead** | Coordinates research/debate teams during ideation | Opus | Ideation session starts (Team mode) |
| **ralphx-ideation-advocate** | Argues for a specific approach in Debate Team mode | Sonnet | Spawned by team lead |
| **ralphx-ideation-critic** | Stress-tests all proposals in Debate Team mode | Sonnet | Spawned by team lead |
| **ralphx-ideation-specialist-backend** | Researches Rust/Tauri backend patterns | Sonnet | Spawned by team lead |
| **ralphx-ideation-specialist-frontend** | Researches React/TS frontend patterns | Sonnet | Spawned by team lead |
| **ralphx-ideation-specialist-infra** | Researches infrastructure and configuration | Sonnet | Spawned by team lead |
| **deep-researcher** | In-depth codebase or web research | Sonnet | Invoked in research contexts |
| **ralphx-project-analyzer** | Analyzes project structure for setup/validation commands | Sonnet | Project added or re-analyzed |
| **ralphx-memory-capture** | Saves agent learnings to project memory after execution | Sonnet | After task completes |
| **ralphx-memory-maintainer** | Organizes and deduplicates project memory | Sonnet | Periodic / on demand |
| **ralphx-utility-session-namer** | Generates a concise name for a new ideation session | Haiku | Session created |
| **ralphx-chat-task** | Handles chat interactions in the context of a specific task | Sonnet | Task chat opened |
| **ralphx-chat-project** | Handles chat interactions at the project level | Sonnet | Project chat opened |
| **review-chat** | Handles chat about a specific review | Sonnet | Review chat opened |

---

## Agent Lifecycle

Every agent goes through the same lifecycle:

```
Spawned (by RalphX backend)
     |
     v
Initializing (reads task context, plan artifact, environment)
     |
     v
Executing (reads code, writes files, runs commands, calls MCP tools)
     |
     v
Completing (runs validation, commits changes, reports outcome via MCP)
     |
     v
Exiting
```

**What triggers a spawn:** Task state transitions trigger agent spawns. For example, when a task moves from PendingReview to Reviewing, the backend spawns a reviewer agent automatically.

**What happens on failure:** If an agent exits without completing its job (no output, process crash, timeout), the reconciler detects the missing agent and respawns it within the retry budget. See [Recovery and Retry](execution.md#recovery-and-retry) in the Execution Pipeline guide.

**Conversation history:** Each agent has a persistent conversation thread. When an agent is respawned — due to a crash, pause/resume, or a revision cycle — it continues in the same conversation. The agent can see its prior work and any feedback given.

---

## The Execution Pipeline

### Worker and Coder Agents

When a task reaches **Executing**, the worker agent is the main orchestrator. It doesn't write code directly. Instead, it:

1. Reads the task context and implementation plan via MCP.
2. Decomposes the task into sub-scopes — groups of related files with no overlap.
3. Dispatches parallel **coder** sub-agents, one per sub-scope, each with exclusive write access to its assigned files.
4. Waits for coders to finish, then runs a gate: typecheck + tests + lint on modified files.
5. If the gate passes, dispatches the next wave. If not, the gate failure is addressed before proceeding.
6. After all waves, validates the full implementation and marks execution complete.

```
Worker
  │
  ├── Wave 1: Coder A (new data layer files)
  │           Coder B (new test files)        ← dispatched in parallel
  │
  ├── [Gate: typecheck + tests pass]
  │
  └── Wave 2: Coder A (wire into existing code)
              Coder B (update existing tests)  ← dispatched in parallel
```

**Why parallel coders?** Speed. Each coder owns its files exclusively — no merge conflicts. Multiple waves of work happen concurrently, cutting execution time significantly.

**What you see:** The task's conversation panel shows the worker's activity. Step progress updates in the task detail view as each sub-scope starts and completes.

### Supervisor Service

Supervisor monitoring is owned by the backend supervisor service rather than a spawned agent. The settings and alert types remain in place for future execution-lane integration, but there is no dedicated supervisor agent in the live catalog today.

| Pattern detected | What the supervisor does |
|-----------------|--------------------------|
| Same tool called 3+ times with similar args | Injects guidance into the worker's conversation |
| No git diff changes for ~2.5 minutes | Injects guidance or escalates |
| Same error repeating 3+ times | Injects guidance; at 4+ repetitions, pauses the task |
| High token usage with no progress | Pauses and notifies you |
| Critical loop detected | Kills the agent and analyzes the failure |

When the supervisor service is wired into live execution, its alerts appear in the task conversation or task detail view.

### Reviewer Agent

After execution (or QA, if enabled), the reviewer agent analyzes what the worker built:

1. Reads the task's acceptance criteria and implementation plan.
2. Reads the full git diff between the task branch and the base branch.
3. Runs validation commands (typecheck, tests, lint) on the modified paths.
4. Applies a review checklist: code quality, test coverage, security, performance, spec adherence.
5. **Must** call `complete_review` with one of three outcomes:

| Outcome | Meaning | What you see |
|---------|---------|--------------|
| **Approved** | Implementation meets all criteria | "Awaiting your approval" — click Approve or Request Changes |
| **Needs changes** | Fixable issues found | Task auto-transitions to ReExecuting; worker gets structured issue list |
| **Escalate** | Reviewer couldn't decide (e.g., breaking change, ambiguous requirements) | Escalation reason shown — you decide |

The reviewer runs in the **same conversation** as prior execution cycles, so it can see the full history of what was tried and what feedback was given.

### Merger Agent

After you approve a task, the [merge pipeline](merge.md) attempts to rebase and merge the task branch automatically. If there are git conflicts that the programmatic merge couldn't resolve, a **merger agent** is spawned on the configured merge lane.

The merger:
1. Gets the correct source and target branches via MCP.
2. Reads all conflicting files and analyzes both sides of each conflict.
3. Resolves each conflict, keeping the correct combination of changes.
4. Runs validation to confirm the merged code compiles.
5. Stages, commits, and exits — the backend detects the clean state and auto-transitions the task to Merged.

If the merger can't resolve a conflict (ambiguous intent, architectural incompatibility), it calls `report_conflict` with an explanation. You'll be notified to resolve the conflict manually.

---

## Ideation Agents

### Solo Orchestrator

In **Solo mode**, a single `ralphx-ideation` agent handles the entire ideation session:

1. **Recover** — checks if a plan and proposals already exist (for session resume).
2. **Understand** — reads your message and determines what you want.
3. **Explore** — launches up to 3 parallel read-only agents to research the codebase. Grounds every suggestion in actual code.
4. **Plan** — synthesizes findings into 2-4 implementation options, selects the best, and creates a plan artifact.
5. **Confirm** — presents the plan to you. Waits for approval before creating tasks.
6. **Propose** — breaks the plan into atomic tasks with priorities and dependencies.
7. **Finalize** — analyzes the dependency graph, shows the critical path, and offers to adjust.

The orchestrator cannot modify code — it only reads and creates plan artifacts and proposals.

### Team Mode: Research Teams and Debate Teams

For complex features, RalphX switches to **team mode**. The `ralphx-ideation-team-lead` (Opus) coordinates a group of specialist agents.

Team mode is currently a Claude-only capability. If the effective ideation harness is Codex, RalphX keeps the session in solo mode instead of attempting a partial team-mode emulation.

**Research Team** — used when a feature touches multiple layers (frontend + backend, UI + database):

```
Team Lead (Opus)
    │
    ├── Frontend Researcher (Sonnet) — explores React/TS patterns
    ├── Backend Researcher (Sonnet) — explores Rust/Tauri service layer
    └── Infrastructure Specialist (Sonnet) — explores config, DB schema
```

**Debate Team** — used for architectural decisions (e.g., "Should we use WebSockets or SSE?"):

```
Team Lead (Opus)
    │
    ├── Approach A Advocate — builds the strongest case for option A
    ├── Approach B Advocate — builds the strongest case for option B
    └── Devil's Advocate — stress-tests all approaches, finds weaknesses
```

**Team approval gate:** Before any team is spawned, the lead presents the proposed team composition to you for approval. You see each teammate's role, model, and what they'll research. You approve before any API calls are made.

After all research is complete, the lead synthesizes findings into a master plan — then follows the same Confirm → Propose → Finalize flow as Solo mode.

### Specialist Agents

During team ideation, the team lead spawns specialists with custom prompts tailored to the session. Specialists:
- Are read-only — they can't modify code, only research it.
- Post findings to shared team artifacts via MCP tools.
- Receive context from the team lead if one specialist's discovery affects another's scope.
- Are gracefully shut down by the team lead after their work is done.

---

## QA Agents

When QA is enabled for a task, three agents handle the automated testing phase:

**qa-prep** — Starts in the background when the task enters Ready. Generates acceptance criteria and browser test steps from the task spec. Runs concurrently with execution — results are ready by the time the worker finishes.

**qa-refiner** — After execution, adapts the qa-prep criteria to match what was actually built. Reviews the git diff and updates test steps to reflect the real implementation.

**qa-executor** — Runs browser-based tests using the refined criteria. Captures screenshots, records pass/fail per criterion. If all pass → task proceeds to review. If any fail → task re-executes with the failure details as feedback.

---

## Communicating with Agents

### The Chat Panel

Every agent has a persistent conversation visible in the task detail view. You can send messages to a running agent directly — your message is injected into the agent's context and the agent will see it on its next turn.

Use this to:
- Give the agent clarifying information mid-execution ("The API uses snake_case, not camelCase")
- Point out something specific ("Focus on the `src/auth/` directory first")
- Override a direction ("Don't refactor the test setup — just fix the failing test")

### @-Mentions

In the chat panel, you can @-mention specific agents in a multi-agent context (e.g., ideation team mode) to direct your message to a particular participant.

### Queued Messages

If you send a message while an agent is between turns (thinking, tool calls in progress), the message is queued and delivered as soon as the agent is ready. You'll see a "pending" indicator until the message is delivered.

### Message Timing

Messages are injected at the agent's next opportunity. For a worker executing a long coder wave, your message may not be read until the current wave completes. This is expected — agents read incoming messages at natural pause points.

---

## Permission Requests

Some operations require explicit user approval before an agent proceeds. When an agent attempts a sensitive action, RalphX shows a **Permission Request** dialog.

**What triggers permission requests:**
- Writing or modifying files outside the task's expected scope
- Running shell commands with elevated impact (e.g., modifying system-level config)
- Making network requests to external services
- Operations that could affect your local environment beyond the task worktree

**What you see:**
- The specific action the agent wants to take
- The file path or command involved
- Options to **Allow** (once), **Allow always** (for this session), or **Deny**

**What happens on deny:** The agent receives a denial signal and will try to find an alternative approach. If no alternative exists, the agent reports the blocker and may pause or fail the task.

> **Note:** Agents run in isolated git worktrees. Most file operations are safe by default — permission requests cover operations that reach outside that isolation boundary.

---

## User Questions

Agents can ask you questions when they need clarification to proceed. This appears as a blocking **User Question** dialog in the task detail view.

**When agents ask questions:**
- The task spec is ambiguous ("The task says 'update the auth flow' — should this use JWT or session cookies?")
- A decision has significant architectural impact
- The agent found multiple valid approaches and needs your preference

**How to respond:**
1. The dialog shows the agent's question and typically 2-4 concrete options.
2. Select an option or type a custom answer.
3. Click **Submit** — the agent receives your answer and continues.

**If you don't respond:** The agent will wait. The task stays in its current state until you answer. There is no timeout — the agent waits indefinitely for your input.

> Unlike permission requests (which are about safety), user questions are about **intent clarification**. The agent knows what it *can* do — it's asking what you *want* it to do.

---

## Team View

The `/team` route (accessible from the sidebar) shows all active agent teams and their activity.

### What you see

| Element | Description |
|---------|-------------|
| **TeammateCard** | One card per active agent — shows name, model, current status, and recent activity |
| **TeamFilterTabs** | Filter by team, project, or agent type |
| **TeamActivityPanel** | Live stream of agent messages and tool calls across all active agents |

### What "idle" means

Agents go idle between turns — this is normal. An idle agent is waiting for input (a message, a task assignment, or a user question response). Idle does not mean stopped or crashed.

### Sending messages from Team View

You can send messages to any active agent directly from the Team Activity Panel. Click the agent's card and use the message input at the bottom.

---

## Session Recovery

Harness sessions can expire or become stale after long idle periods. RalphX handles this automatically.

**What happens when a session expires:**
1. The agent's output stream stops.
2. RalphX detects the expired session via the reconciler.
3. A new session is started for the same agent on the selected harness.
4. The **conversation history is preserved** — the new session is initialized with the full prior context.
5. The agent continues from where it left off.

From your perspective, you may see a brief pause in the agent's activity, then normal operation resumes. The conversation thread is unbroken.

**Startup recovery:** If RalphX is closed while agents are running, they are automatically respawned on the next launch. Tasks in agent-active states (Executing, Reviewing, QaTesting, etc.) have their agents re-spawned, and conversation history is restored.

See [Recovery and Retry](execution.md#recovery-and-retry) for the full recovery logic per state.

---

## MCP Architecture

Agents communicate with the RalphX backend through the **Model Context Protocol (MCP)** — a standardized messaging layer.

```
Selected Harness Agent (running locally)
         |
         | MCP Protocol (JSON over stdio/HTTP)
         v
  ralphx-mcp-server (TypeScript proxy)
         |
         | HTTP → localhost:3847
         v
  Tauri Backend (Rust)
         |
         v
  SQLite Database / Git Service / Task State Machine
```

**What agents do via MCP:**
- Read task context, plan artifacts, and project analysis
- Report step progress (start, complete, skip)
- Submit review outcomes
- Report merge conflicts
- Ask user questions
- Create ideation proposals and plan artifacts

**What agents do directly (not via MCP):**
- Read and write files (in the task worktree)
- Run shell commands (git, npm, cargo, etc.)
- Browse the web
- Spawn sub-agents

The MCP layer ensures agents stay within their authorized scope. Each agent type has a specific allowlist of MCP tools — a worker can't submit a review, and a reviewer can't create proposals.

---

## Agent Tool Scopes

Each agent type has a restricted set of actions. This is enforced at three layers:
1. **Agent-level** — the agent's system prompt defines its role and constraints.
2. **Tool allowlist** — the MCP server only processes calls from authorized tools for each agent type.
3. **Context type** — the backend validates that the calling agent matches the expected context type for each operation.

### MCP Tool Access by Agent

| Agent | Key MCP tools |
|-------|--------------|
| **worker** | `get_task_context`, `get_task_steps`, `start_step`, `complete_step`, `get_review_notes`, `get_task_issues`, `mark_issue_in_progress`, `mark_issue_addressed`, `get_project_analysis` |
| **coder** | Same as worker (coder is a scoped variant of worker) |
| **reviewer** | `get_task_context`, `get_review_notes`, `get_task_steps`, `get_task_issues`, `complete_review`, `get_project_analysis` |
| **merger** | `get_merge_target`, `get_task_context`, `report_conflict`, `report_incomplete`, `get_project_analysis` |
| **ralphx-ideation** | `create_plan_artifact`, `update_plan_artifact`, `create_task_proposal`, `list_session_proposals`, `analyze_session_dependencies` |
| **ralphx-ideation-team-lead** | All orchestrator tools + `request_team_plan`, `create_team_artifact`, `get_team_artifacts`, `save_team_session_state` |
| **ralphx-chat-task** | `update_task`, `add_task_note`, `get_task_details` |
| **ralphx-chat-project** | `suggest_task`, `list_tasks` |

### File System Access

All execution agents (worker, coder, reviewer) operate inside the task's **git worktree** — an isolated copy of the repository at `~/ralphx-worktrees/{project}/task-{id}`. They cannot read or modify your main checkout.

Reviewers are read-only — they examine the worktree but don't commit changes.

---

## Troubleshooting

### Agent stuck — no activity for a long time

**What it means:** The agent may be in a loop, waiting on a tool call, or the process died silently.

**What to do:**
1. Open the task's conversation to check the last agent output.
2. The supervisor service thresholds are intended to flag stalls (after ~2.5 minutes with no git diff) once live monitoring is wired in.
3. The reconciler will auto-restart the agent if the process died.
4. If the issue persists, click **Stop** then **Restart** for a clean agent respawn.

### Agent keeps asking the same question

**What it means:** The agent needs information it can't infer from the task spec or codebase.

**What to do:** Answer the User Question dialog with a specific, unambiguous answer. If it asks again, it may not have received the answer — check if the question dialog has a pending state or try sending the answer via the chat panel directly.

### Reviewer keeps escalating

**What it means:** The implementation has complexity or ambiguity the AI can't resolve with confidence.

**What to do:** Read the escalation reason in the task detail view. If the implementation looks correct, click **Approve**. If something needs changing, click **Request Changes** and add a note explaining what you want fixed.

### Merger reports unresolvable conflict

**What it means:** The merger agent found conflicts too complex to resolve automatically — both sides changed the same logic in incompatible ways.

**What to do:**
1. Read the merger's conflict report — it explains exactly which files have conflicts and why.
2. Resolve the conflicts manually in the task worktree (`~/ralphx-worktrees/{project}/task-{id}`).
3. After resolving, commit the changes and use the **Mark Resolved** action in the task detail view.

### Ideation team not spawning

**What it means:** The team composition request may have been denied, or the backend couldn't create the team.

**What to do:**
1. Check if the Team Approval dialog appeared and was dismissed without approving.
2. Re-start the ideation session — the orchestrator will re-request team approval.
3. If the issue persists, try Solo mode for the session.

### Agent spawned but immediately failed

**What it means:** Common causes: git worktree couldn't be created, pre-execution setup failed (e.g., `npm install`), or the task context is missing.

**What to do:**
1. Check the task detail view for a specific error message.
2. For worktree errors, see the [Execution Troubleshooting guide](execution.md#troubleshooting).
3. Click **Retry** to re-queue the task — transient errors usually clear on the next attempt.

### Messages sent to agent not being received

**What it means:** The agent may be between turns or processing a long tool call.

**What to do:** Messages are queued and delivered at the agent's next turn. If you see a "pending" indicator on your message, wait for the agent to complete its current operation. If the agent appears stuck and not progressing, use Stop + Restart to get a fresh spawn that will pick up the message from history.

---

## See Also

- [Execution Pipeline](execution.md)
- [Merge Pipeline](merge.md)
- [Task State Machine](task-state-machine.md)
