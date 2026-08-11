---
paths:
  - "frontend/src/components/tasks/detail-views/**"
  - "frontend/src/components/tasks/TaskDetailView*"
  - "frontend/src/components/agents/task-details/**"
---

# Task Detail Views Registry

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

## InternalStatus → View Mapping

| InternalStatus | View Component | Purpose |
|----------------|----------------|---------|
| `backlog` | BasicTaskDetail | Idle task in backlog |
| `ready` | BasicTaskDetail | Ready for execution |
| `blocked` | BasicTaskDetail | Waiting on dependency |
| `executing` | ExecutionTaskDetail | Live AI execution progress |
| `re_executing` | ExecutionTaskDetail | Re-execution after revision |
| `qa_refining` | BasicTaskDetail | QA refinement (no specialized view) |
| `qa_testing` | BasicTaskDetail | QA testing (no specialized view) |
| `qa_passed` | BasicTaskDetail | QA passed (no specialized view) |
| `qa_failed` | BasicTaskDetail | QA failed (no specialized view) |
| `pending_review` | WaitingTaskDetail | Work done, awaiting AI review |
| `reviewing` | ReviewingTaskDetail | AI review in progress |
| `review_passed` | HumanReviewTaskDetail | AI approved, human confirmation if required |
| `escalated` | EscalatedTaskDetail | AI escalated to human |
| `revision_needed` | RevisionTaskDetail | Changes requested |
| `approved` | CompletedTaskDetail | Task completed |
| `pending_merge` | MergingTaskDetail | Programmatic merge in progress |
| `merging` | MergingTaskDetail | Agent-assisted merge in progress |
| `waiting_on_pr` | MergingTaskDetail | PR-based merge waiting on external PR |
| `updating_plan_branch` | BranchUpdateTaskDetail | Plan branch update in progress |
| `updating_task_branch` | BranchUpdateTaskDetail | Task branch update in progress |
| `branch_update_blocked` | BranchUpdateTaskDetail | Branch update blocked, needs attention |
| `merge_incomplete` | MergeIncompleteTaskDetail | Non-conflict merge failure, retry/resolve |
| `merge_conflict` | MergeConflictTaskDetail | Merge conflicts, manual resolution |
| `merged` | MergedTaskDetail | Successfully merged |
| `failed` | BasicTaskDetail | Execution failed |
| `cancelled` | BasicTaskDetail | Task cancelled |
| `paused` | BasicTaskDetail | Execution paused |
| `stopped` | BasicTaskDetail | Execution stopped |

## File Locations

| Component | Path |
|-----------|------|
| Registry definition | `frontend/src/components/agents/task-details/AgentsTaskDetailPanel.tsx` — `TASK_DETAIL_VIEWS` map |
| View selection logic | `frontend/src/components/agents/task-details/AgentsTaskDetailPanel.tsx` — `TASK_DETAIL_VIEWS[status] ?? BasicTaskDetail` |
| Entry point (Agents Tasks artifact) | `frontend/src/components/agents/task-details/AgentsTaskDetailOverlay.tsx` |
| View components | `frontend/src/components/agents/task-details/detail-views/*.tsx` |

## Agents Fork

| Rule | Detail |
|------|--------|
| Path | Agents-owned detail views live under `src/components/agents/task-details/**`; do not assume generic `src/components/tasks/**` guidance is sufficient |
| Shell | Agents right panel uses a one-column `TwoColumnLayout` compatibility shell: summary → stage body → evidence → context → actions |
| Validation | Agents validation evidence belongs in state/evidence slots; do not re-add a global validation footer in the shell |
| History | `StateTimelineNav` is runtime stage navigation: preserve repeated execution/review/merge attempts when transitions distinguish them |
| Transcript focus | Historical stage with `conversationId` must set `taskHistoryState` and focus the main Agents chat on the matching runtime `contextType`; no `conversationId` → show no-transcript copy and do not borrow another transcript |
| Historical actions | Mutation actions stay hidden/disabled in historical mode |

## View Components (13 total)

| Component | States Handled | Key Features |
|-----------|----------------|--------------|
| BasicTaskDetail | backlog, ready, blocked, qa_*, failed, cancelled, paused, stopped | Steps list, description |
| ExecutionTaskDetail | executing, re_executing | Live progress, step tracking, revision feedback |
| WaitingTaskDetail | pending_review | Work summary, completion stats |
| ReviewingTaskDetail | reviewing | AI review progress, step indicators |
| HumanReviewTaskDetail | review_passed | AI summary, approve/reject actions |
| EscalatedTaskDetail | escalated | Escalation reason, human decision buttons |
| RevisionTaskDetail | revision_needed | Review feedback, parsed issues, attempt count |
| CompletedTaskDetail | approved | Approval details, review history, diff viewer |
| MergingTaskDetail | pending_merge, merging, waiting_on_pr | Agent merge progress spinner / PR wait |
| BranchUpdateTaskDetail | updating_plan_branch, updating_task_branch, branch_update_blocked | Branch update progress/blocked recovery |
| MergeConflictTaskDetail | merge_conflict | Conflict files, resolution steps, resolve button |
| MergeIncompleteTaskDetail | merge_incomplete | Error context, recovery steps, retry/resolve buttons |
| MergedTaskDetail | merged | Merge completion details |

## Wiring

```
AgentsTaskDetailOverlay (Agents Tasks artifact)
  → AgentsTaskDetailPanel (TaskDetailViewMode + TaskDetailContextProvider)
    → TASK_DETAIL_VIEWS[status] ?? BasicTaskDetail
      → TwoColumnLayout → TaskContextRail + state-specific body
```

**Props:** `useViewRegistry` activates registry | `viewAsStatus` enables historical state viewing | views receive `isHistorical`; common plan/branch/PR context comes from `TaskContextRail`

## Common Context Rail

| Rule | Detail |
|------|--------|
| Required | Registry-backed detail views must use `TwoColumnLayout`; do not hand-roll a separate left column |
| Left rail | `TaskContextRail` owns description, plan/proposal, branch, PR, merge summary, and historical lens note |
| State body | View components own status banner, progress, validation, recovery, review, and actions only |
| Plan/PR cards | Prefer shared rail data; one-off right-column plan/PR cards are allowed only for status-specific action/context |
| Historical views | Current implementation is a historical status lens over latest task data; label plan/branch/PR values as latest unless backend snapshots exist |

## Adding New Views

1. Create `src/components/agents/task-details/detail-views/NewStatusTaskDetail.tsx`
2. Implement `TaskDetailProps` interface: `{ task: Task; isHistorical?: boolean; viewStatus?: InternalStatus }`
3. Render content inside `TwoColumnLayout`; the common rail is injected by `TaskDetailContextProvider`
4. Add to `TASK_DETAIL_VIEWS` map in `AgentsTaskDetailPanel.tsx`
