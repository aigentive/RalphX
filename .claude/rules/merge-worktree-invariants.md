---
paths:
  - "src-tauri/src/domain/state_machine/transition_handler/side_effects/**"
  - "src-tauri/src/domain/state_machine/transition_handler/merge_helpers.rs"
  - "src-tauri/src/application/chat_service/chat_service_merge.rs"
  - "src-tauri/src/application/git_service/**"
  - "src-tauri/src/application/task_transition_service.rs"
  - "src-tauri/src/commands/plan_branch_commands.rs"
  - "src-tauri/src/http_server/handlers/git.rs"
  - "frontend/src/api/plan-branch.ts"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Merge Worktree Invariants

**Archetype addressed:** #1 (Merge worktree lifecycle)

Every change that touches merge worktree creation, cleanup, or lifecycle management MUST satisfy all invariants below.

## NON-NEGOTIABLE Checklist

| # | Invariant | Must Hold | Pass? |
|---|-----------|-----------|-------|
| 1 | Branch exists | Verify branch exists in git before calling worktree create | yes / no |
| 2 | Worktree cleanup | Every worktree create has a matching cleanup on ALL exit paths (success, error, timeout, cancel) | yes / no |
| 3 | No retry on phantom | If branch reference is invalid, clear DB state — do NOT retry | yes / no |
| 4 | Validation before delete | Never delete worktree while validation processes are running | yes / no |
| 5 | Single owner | Only one process may own a worktree at a time — check lock before touching | yes / no |
| 6 | Reconciler safe | Reconciler must not race with active merge — check phase before acting | yes / no |

All 6 must be yes before merging. Any missed exit path is a serious regression risk in a historically fragile area.

## When This Applies

| Trigger | Apply checklist? |
|---------|-----------------|
| New merge path added | yes |
| Worktree create/delete logic changed | yes |
| Reconciler changes | yes |
| Merge phase transitions modified | yes |
| Error/timeout handling in merge | yes |
| Unrelated Rust backend changes | skip |

## Failure Mode Reference

| Symptom | Root Cause | Invariant |
|---------|------------|-----------|
| "phantom branch" loop | Retry on invalid git ref | #3 — No retry on phantom |
| Leaked worktree directories | Missing cleanup on error path | #2 — Worktree cleanup |
| Race condition on reconcile | Reconciler acting on active merge | #6 — Reconciler safe |
| Concurrent worktree corruption | Two processes writing same worktree | #5 — Single owner |
