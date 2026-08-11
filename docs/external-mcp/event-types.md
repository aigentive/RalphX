# External MCP Event Types

Complete catalog of all pipeline events emitted by RalphX. Events are retrieved via `v1_get_recent_events` or `v1_subscribe_events`. Each event has a discriminated `event_type` field for safe narrowing in TypeScript.

TypeScript interfaces: `plugins/app/ralphx-external-mcp/src/tools/events.ts` — `RalphXEvent` union type.

---

## Two-Channel Delivery Guarantee

All 9 lifecycle events (`task:execution_started`, `task:execution_completed`, `review:ready`,
`review:approved`, `review:changes_requested`, `review:escalated`, `merge:ready`,
`merge:completed`, `merge:conflict`) are delivered via **both** channels on every transition:

| Channel | Mechanism | Consumer |
|---|---|---|
| **SSE / poll** | Persisted to `external_events` table | `v1_get_recent_events`, `v1_subscribe_events` |
| **HTTP push** | Delivered via `WebhookPublisher` to registered webhook endpoints | Any registered webhook URL |

Both channels are guaranteed to fire together. External agents need not poll if they have a registered webhook; polling consumers need not register a webhook.

---

## Task Events

### `task:created`

Emitted when a new task is created in the project.

```json
{
  "event_type": "task:created",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "title": "Add user authentication",
  "timestamp": "2026-03-20T14:00:00Z"
}
```

### `task:status_changed`

Emitted when a task transitions between pipeline statuses (for example `backlog → executing`,
`executing → pending_review`).

```json
{
  "event_type": "task:status_changed",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "old_status": "executing",
  "new_status": "pending_review",
  "timestamp": "2026-03-20T14:15:00Z"
}
```

### `task:step_completed`

Emitted when an individual execution step within a task is marked complete.

```json
{
  "event_type": "task:step_completed",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "step_id": "step-001",
  "step_title": "Write unit tests",
  "timestamp": "2026-03-20T14:10:00Z"
}
```

### `task:execution_started`

Emitted when a worker agent begins executing a task.

```json
{
  "event_type": "task:execution_started",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T14:05:00Z"
}
```

### `task:execution_completed`

Emitted when a worker agent finishes execution and the task moves out of the `executing` state.

```json
{
  "event_type": "task:execution_completed",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T14:14:00Z"
}
```

---

## Review Events

### `review:ready`

Emitted when a task enters `pending_review` and is queued for the reviewer agent.

```json
{
  "event_type": "review:ready",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T14:16:00Z"
}
```

### `review:approved`

Emitted when the approval decision is made for a reviewed task and it moves toward merge.

```json
{
  "event_type": "review:approved",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T14:30:00Z"
}
```

### `review:changes_requested`

Emitted when the reviewer agent requests changes, sending the task back to re-execution.

```json
{
  "event_type": "review:changes_requested",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T14:28:00Z"
}
```

### `review:escalated`

Emitted when the reviewer agent escalates a task for exceptional attention. Requires manual triage
or delegated resolution before the task can continue.

```json
{
  "event_type": "review:escalated",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "reason": "Repeated execution failures exceeded retry limit",
  "timestamp": "2026-03-20T14:29:00Z"
}
```

---

## Merge Events

### `merge:ready`

Emitted when a task enters `pending_merge` and is queued for the merger agent. This is a
merge-pipeline progress event, not a second generic approval ceremony.

```json
{
  "event_type": "merge:ready",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T14:31:00Z"
}
```

### `merge:completed`

Emitted when the merger agent successfully merges the task branch.

```json
{
  "event_type": "merge:completed",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T14:45:00Z"
}
```

### `merge:conflict`

Emitted when the merger agent encounters a conflict that requires human resolution.

```json
{
  "event_type": "merge:conflict",
  "task_id": "task-abc123",
  "project_id": "proj-xyz",
  "conflict_type": "git_merge_conflict",
  "timestamp": "2026-03-20T14:40:00Z"
}
```

---

## Ideation Events

### `ideation:session_created`

Emitted when a new ideation chat session is created.

```json
{
  "event_type": "ideation:session_created",
  "session_id": "session-def456",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T13:00:00Z"
}
```

### `ideation:plan_created`

Emitted when the orchestrator creates a plan artifact in an ideation session.

```json
{
  "event_type": "ideation:plan_created",
  "session_id": "session-def456",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T13:20:00Z"
}
```

### `ideation:verified`

Emitted when the authoritative Verify Plan action records exact proof for the current plan artifact.

```json
{
  "event_type": "ideation:verified",
  "session_id": "session-def456",
  "project_id": "proj-xyz",
  "plan_artifact_id": "artifact-current",
  "status": "verified",
  "timestamp": "2026-03-20T13:45:00Z"
}
```

### `ideation:proposals_ready`

Emitted when the orchestrator finalizes proposals in an ideation session.

```json
{
  "event_type": "ideation:proposals_ready",
  "session_id": "session-def456",
  "project_id": "proj-xyz",
  "proposal_count": 3,
  "timestamp": "2026-03-20T13:50:00Z"
}
```

### `ideation:auto_propose_sent`

Emitted when the auto-propose pipeline successfully creates tasks from finalized proposals.

```json
{
  "event_type": "ideation:auto_propose_sent",
  "session_id": "session-def456",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T13:51:00Z"
}
```

### `ideation:session_accepted`

Emitted when the ideation session is accepted and enters delivery tracking.

```json
{
  "event_type": "ideation:session_accepted",
  "session_id": "session-def456",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T13:52:00Z"
}
```

---

## Delivery Semantics

`merge:completed` is a per-task event. Session delivery completion is derived separately via
`v1_get_session_tasks`:

- `delivery_status: "pending_review"` → some tasks are waiting on review/approval
- `delivery_status: "in_progress"` → tasks still executing, queued, or merging
- `delivery_status: "partial"` → some merged, the rest ended terminally
- `delivery_status: "delivered"` → all tasks merged to main

External agents should report "plan delivered" only when the session delivery status is
`delivered`.

### `ideation:session_accepted`

Emitted when all proposals in a session are finalized and applied as tasks, transitioning the session to the Accepted state. External agents can use this event as a trigger to begin monitoring the resulting tasks.

```json
{
  "event_type": "ideation:session_accepted",
  "session_id": "session-def456",
  "project_id": "proj-xyz",
  "timestamp": "2026-03-20T13:52:00Z"
}
```

### `ideation:auto_propose_failed`

Emitted when the auto-propose pipeline fails to create tasks from proposals.

```json
{
  "event_type": "ideation:auto_propose_failed",
  "session_id": "session-def456",
  "project_id": "proj-xyz",
  "error": "dependency resolution failed: missing prerequisite task",
  "timestamp": "2026-03-20T13:51:00Z"
}
```

---

## System Events

### `system:webhook_unhealthy`

Emitted when a registered webhook fails repeated delivery attempts. Check the webhook configuration and endpoint availability.

```json
{
  "event_type": "system:webhook_unhealthy",
  "webhook_id": "wh-ghi789",
  "project_id": "proj-xyz",
  "failure_count": 5,
  "timestamp": "2026-03-20T15:00:00Z"
}
```

### `system:rate_limit_warning`

Emitted when an API key approaches its rate limit threshold.

```json
{
  "event_type": "system:rate_limit_warning",
  "project_id": "proj-xyz",
  "api_key_id": "rxk_live_abc",
  "timestamp": "2026-03-20T15:05:00Z"
}
```

---

## Summary Table

| Event Type | Category | Key Fields |
|---|---|---|
| `task:created` | Task | `task_id`, `title` |
| `task:status_changed` | Task | `task_id`, `old_status`, `new_status` |
| `task:step_completed` | Task | `task_id`, `step_id`, `step_title` |
| `task:execution_started` | Task | `task_id` |
| `task:execution_completed` | Task | `task_id` |
| `review:ready` | Review | `task_id` |
| `review:approved` | Review | `task_id` |
| `review:changes_requested` | Review | `task_id` |
| `review:escalated` | Review | `task_id`, `reason` |
| `merge:ready` | Merge | `task_id` |
| `merge:completed` | Merge | `task_id` |
| `merge:conflict` | Merge | `task_id`, `conflict_type` |
| `ideation:session_created` | Ideation | `session_id` |
| `ideation:plan_created` | Ideation | `session_id` |
| `ideation:verified` | Ideation | `session_id` |
| `ideation:proposals_ready` | Ideation | `session_id`, `proposal_count` |
| `ideation:session_accepted` | Ideation | `session_id` |
| `ideation:auto_propose_sent` | Ideation | `session_id` |
| `ideation:auto_propose_failed` | Ideation | `session_id`, `error` |
| `system:webhook_unhealthy` | System | `webhook_id`, `failure_count` |
| `system:rate_limit_warning` | System | `api_key_id` |

All events include `project_id` and `timestamp` (ISO 8601 UTC).

---

## TypeScript Narrowing Example

```typescript
import type { RalphXEvent } from 'ralphx-external-mcp/tools/events';

function handleEvent(event: RalphXEvent): void {
  switch (event.event_type) {
    case 'task:status_changed':
      console.log(`Task ${event.task_id}: ${event.old_status} → ${event.new_status}`);
      break;
    case 'review:escalated':
      console.log(`Escalation reason: ${event.reason}`);
      break;
    case 'ideation:proposals_ready':
      console.log(`${event.proposal_count} proposals ready in session ${event.session_id}`);
      break;
    default:
      // TypeScript exhaustiveness check — all cases handled
      break;
  }
}
```
