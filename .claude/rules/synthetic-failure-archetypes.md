---
paths:
  - "src-tauri/src/domain/state_machine/**"
  - "src-tauri/src/application/task_transition_service.rs"
  - "src-tauri/src/application/task_scheduler_service/**"
  - "src-tauri/src/application/chat_service/**"
  - "src-tauri/src/commands/**"
  - "src-tauri/src/http_server/**"
  - "frontend/src/hooks/useAgentEvents.ts"
  - "frontend/src/components/Chat/**"
  - "agents/ralphx-ideation/**"
  - "agents/ralphx-ideation-specialist-*/**"
  - "agents/ralphx-general-explorer/**"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Synthetic Failure Archetype Reference

> Heuristic reference derived from prior RalphX incidents, regressions, and hardening work.
> Use this file to cross-check proposed changes against known recurring failure patterns BEFORE implementation begins.

## Purpose

Evaluation specialists and plan critics MUST check proposed changes against these 5 archetypes during ideation verification. Treat the archetypes as guardrail heuristics grounded in prior failures in this repo, not as a substitute for reading the actual code or incident logs.

---

## Archetype 1: Merge Worktree Lifecycle

| Field | Detail |
|-------|--------|
| **Trigger** | New merge path leaves worktree state inconsistent (create without matching cleanup, phantom branch reference, concurrent access) |
| **Affected Files** | `src-tauri/src/domain/state_machine/transition_handler/side_effects/`, `src-tauri/src/application/chat_service/chat_service_merge.rs` |
| **Evidence** | Repeated merge/worktree regressions and cleanup failures across prior hardening work |
| **Still Active?** | Yes — historically one of the highest-risk lifecycle areas |
| **Check** | Does this change add or modify a worktree creation/deletion path? → `.claude/rules/merge-worktree-invariants.md` |

**Key failure modes:**
- Worktree created but cleanup skipped on error/timeout/cancel exit paths
- Branch reference checked once, stale by the time worktree create runs
- Reconciler races with active merge phase, clobbers in-flight state

---

## Archetype 2: Auto-Transition Churn

| Field | Detail |
|-------|--------|
| **Trigger** | Auto-transition fires while agent is still live, OR fires repeatedly on app restart |
| **Affected Files** | `src-tauri/src/application/task_transition_service.rs`, `src-tauri/src/domain/state_machine/transition_handler/on_enter_states/` |
| **Evidence** | Repeated transition-loop and restart-replay regressions in prior fixes |
| **Still Active?** | Yes — still an active failure class |
| **Check** | Does this change modify state transitions or add new pipeline stages? → Verify single-fire guard exists on every auto-transition |

**Key failure modes:**
- Transition fires on Executing state when worker already running (double-fire)
- Restart replays all pending transitions without idempotency guard
- New state added without on_enter handler falls through to wrong default

---

## Archetype 3: SQLite Concurrent Access

| Field | Detail |
|-------|--------|
| **Trigger** | New async fn accesses SQLite without db.run wrapper |
| **Affected Files** | Any new async fn touching the database layer |
| **Evidence** | Repeated DB lock/starvation fixes and async access regressions |
| **Still Active?** | Periodic — flares up when new DB access code is added |
| **Check** | Does this add async DB access? → DbConnection rule in CLAUDE.md (Rule #16) |

**Key failure modes:**
- Direct conn.lock().await in async context holds lock across await points
- Blocking query on async executor thread starves other tasks
- Lock starvation causes 60s timeout cascades across unrelated operations

---

## Archetype 4: Agent Status Desync

| Field | Detail |
|-------|--------|
| **Trigger** | New session or agent type added without wiring UI store key, event handlers, or status transitions |
| **Affected Files** | `frontend/src/components/Chat/IntegratedChatPanel.tsx`, `src/hooks/useAgentEvents.ts`, `src-tauri/src/commands/execution_commands.rs` |
| **Evidence** | Prior agent-status / UI-state mismatches and silent-exit cleanup bugs |
| **Still Active?** | Periodic — flares up with each new agent type |
| **Check** | Does this add a new agent/session type? → `.claude/rules/event-coverage-checklist.md` |

**Key failure modes:**
- New context type has no store key → status stays idle even when agent running
- agent:run_completed not handled → spinner never clears
- Silent exit (no execution_complete call) → task stuck in Executing forever

---

## Archetype 5: Incomplete Event Coverage

| Field | Detail |
|-------|--------|
| **Trigger** | New feature ships without ALL exit paths (error, timeout, cancel, user action) emitting the required UI events |
| **Affected Files** | Ideation pipeline handlers, `src-tauri/src/commands/`, canonical agent prompt/config files under `agents/` |
| **Evidence** | Prior missing-event and gate-bypass regressions in pipeline handlers |
| **Still Active?** | Yes — surface area grows with every new pipeline feature |
| **Check** | Does this add pipeline functionality, a new MCP tool, or a new agent type? → `.claude/rules/event-coverage-checklist.md` |

**Key failure modes:**
- Happy path emits event; error path silently swallows exception with no UI feedback
- Timeout path never fires cleanup → UI shows stale in-progress state
- Gate condition checked only on one code path; alternate route bypasses guard

---

## Quick Reference: Archetype → Guard Mapping

| Archetype | Trigger Signal | Guard Checklist |
|-----------|---------------|-----------------|
| #1 Merge Worktree Lifecycle | Touches worktree create/delete paths | `.claude/rules/merge-worktree-invariants.md` |
| #2 Auto-Transition Churn | Modifies state transitions or adds pipeline stage | Verify single-fire guard on every auto-transition |
| #3 SQLite Concurrent Access | New async fn with DB access | `CLAUDE.md` Rule #16 (DbConnection) |
| #4 Agent Status Desync | New agent/session/context type | `.claude/rules/event-coverage-checklist.md` |
| #5 Incomplete Event Coverage | New pipeline feature, MCP tool, or agent type | `.claude/rules/event-coverage-checklist.md` |

---

## Evidence Source

This file is a compact memory of recurring failure classes seen in RalphX hardening work.
- Use it as a planning heuristic.
- Do NOT cite the labels here as authoritative metrics unless the underlying incident data is checked separately.
- When severity matters, prefer code evidence, current logs, and reproducible failure paths over the archetype summary itself.
