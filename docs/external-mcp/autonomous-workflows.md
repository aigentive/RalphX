# Autonomous Agent Workflows

This document describes how autonomous Claude Code CLI agents navigate the full RalphX pipeline using the External MCP tools and webhooks.

**Key principle:** Agents are fully autonomous — they do NOT mirror RalphX's internal state
machine. Instead, they observe pipeline events via webhooks, query current state via MCP tools,
and act accordingly.

Important distinctions:
- `review_passed` is the approval-decision point
- `merge:ready` / `pending_merge` are merge-pipeline progress signals after approval
- session delivery is complete when `delivery_status = "delivered"` and all tasks are merged

---

## Agent Architecture

```
ReefAgent Gateway (:18789)
  ├─ /hooks/ralphx          ← receives webhook events from RalphX
  ├─ Event Dispatcher       ← routes events to appropriate agents
  └─ Claude Code CLI agents (fully autonomous, MCP-equipped)
       ├─ PM Agent          ← manages ideation, scheduling, attention
       ├─ SWE Agent         ← monitors execution, annotates tasks
       └─ Reviewer Agent    ← spawned on review:ready events
```

Agents are spawned as Claude Code CLI processes (`claude --plugin-dir ... -p "..."`) with full access to External MCP tools. Each agent is given context (task ID, event type, current state) and navigates the pipeline autonomously from there.

---

## Startup Sequence

On MCP connection, agents follow this bootstrap:

```typescript
// 1. Fetch the agent guide — formatting rules + workflow patterns
const guide = await mcp.call('v1_get_agent_guide');
// Inject guide content into agent system prompt

// 2. Register webhook for real-time event delivery
const { webhook_id } = await mcp.call('v1_register_webhook', {
  url: 'http://127.0.0.1:18789/hooks/ralphx',
});
// Store webhook_id for cleanup on disconnect

// 3. Backfill missed events (after reconnect)
const { events } = await mcp.call('v1_get_recent_events', {
  project_id: 'proj-xyz',
  cursor: lastSeenCursor,
});
// Process missed events through dispatcher

// 4. Check for pending attention items
const { escalated_reviews, failed_tasks, merge_conflicts } = await mcp.call(
  'v1_get_attention_items',
  { project_id: 'proj-xyz' },
);
// Spawn agents for any items requiring action
```

---

## Full Autonomous Pipeline Flow

### Phase 1: Ideation → Plan → Schedule

```
PM Agent
  │
  ├─ v1_start_ideation(project_id, description)
  │    → starts ideation session; agent sends messages to shape the plan
  │
  ├─ v1_send_ideation_message(session_id, message)  [loop until plan ready]
  │
  ├─ v1_get_plan(session_id)
  │    → review plan proposals
  │
  ├─ v1_trigger_plan_verification(session_id)
  │    → adversarial review loop; wait for verified status
  │
  ├─ v1_get_plan_verification(session_id)
  │    → poll until status = "verified" (or failed/cancelled)
  │
  └─ v1_accept_plan_and_schedule(session_id)
       → tasks enter Backlog; RalphX auto-schedules execution
       → webhook fires: ideation:proposals_ready
```

**Ideation webhook events:**

| Event | When | Agent action |
|-------|------|--------------|
| `ideation:session_created` | New session opened | Log / no-op |
| `ideation:plan_created` | Orchestrator drafted plan | Trigger verification |
| `ideation:verified` | Plan passed adversarial review | Call accept_plan_and_schedule |
| `ideation:proposals_ready` | Tasks created in backlog | Monitor pipeline |
| `ideation:auto_propose_sent` | Auto-propose message sent successfully | Log / no-op |
| `ideation:auto_propose_failed` | Task creation failed | Re-trigger or alert human |

---

### Phase 2: Execution Monitoring

Once tasks are scheduled, SWE agents monitor execution via webhooks. Agents do NOT intervene in execution — they observe and annotate.

```
Webhook: task:execution_started
  │
  SWE Agent spawned with task context
  │
  ├─ v1_get_task_detail(task_id)
  │    → understand what task is being executed
  │
  ├─ v1_get_task_steps(task_id)
  │    → track step progress
  │
  └─ [wait for task:execution_completed or task:status_changed]

Webhook: task:step_completed
  │
  └─ v1_create_task_note(task_id, "Step N completed: <title>")
       → annotate for human review

Webhook: task:execution_completed
  │
  └─ v1_get_task_detail(task_id)
       → verify task moved to PendingReview
       → log completion
```

**Task webhook events:**

| Event | When | Agent action |
|-------|------|--------------|
| `task:created` | Task added to backlog | No-op (scheduled automatically) |
| `task:status_changed` | Pipeline state change | Check new_status; act if needed |
| `task:execution_started` | Worker agent began | Spawn SWE observer |
| `task:step_completed` | Step finished | Annotate via v1_create_task_note |
| `task:execution_completed` | Worker finished | Verify PendingReview; log |

---

### Phase 3: Review

When a task enters review, a Reviewer agent is spawned autonomously.

```
Webhook: review:ready
  │
  Reviewer Agent spawned
  │
  ├─ v1_get_task_detail(task_id)
  ├─ v1_get_task_diff(task_id)
  ├─ v1_get_review_summary(task_id)
  │
  ├─ [if changes needed]
  │    v1_request_changes(task_id, comments)
  │    → task re-enters execution queue
  │
  └─ [if approval authority exists in this integration/policy]
       v1_approve_review(task_id)
       → task enters PendingMerge
       → webhook fires: review:approved

If the current integration does NOT grant approval authority, the agent should report the review
result and wait for the delegated decider. Do not assume every external agent may approve. Do not
assume every external agent must ask a human.
```

**Review webhook events:**

| Event | When | Agent action |
|-------|------|--------------|
| `review:ready` | Task queued for review | Spawn Reviewer agent |
| `review:approved` | Approval decision made; merge pipeline continues | Monitor merge pipeline |
| `review:changes_requested` | Changes needed | Log; SWE monitors re-execution |
| `review:escalated` | Exceptional triage needed | Alert human/owner; spawn Senior agent if useful |

**Escalation handling:**

```typescript
// On review:escalated
async function handleEscalation(event: ReviewEscalatedEvent) {
  // 1. Notify human operator / owner
  await notifyHuman({
    message: `Task ${event.task_id} escalated for human review`,
    task_url: buildTaskUrl(event.task_id),
  });

  // 2. Agent can investigate but should not resolve unless explicitly delegated
  const detail = await mcp.call('v1_get_task_detail', { task_id: event.task_id });
  await mcp.call('v1_create_task_note', {
    task_id: event.task_id,
    note: `Escalation: ${detail.escalation_reason}. Human review required.`,
  });

  // 3. Delegated resolver decides whether to call v1_resolve_escalation
}
```

---

### Phase 4: Merge

`merge:ready` is a merge-pipeline event, not a second generic review/approval ceremony.
By this point the approval decision has already happened. External agents should monitor merge
progress and only surface exceptions such as `merge:conflict`.

```
Webhook: merge:ready
  │
  PM Agent
  │
  ├─ v1_get_merge_pipeline(project_id)
  │    → see tasks entering merge orchestration
  │
  └─ Await merge outcome
       → webhook fires: merge:completed
       or merge:conflict
```

**Merge webhook events:**

| Event | When | Agent action |
|-------|------|--------------|
| `merge:ready` | Task entering merge pipeline | Monitor; no generic user prompt required |
| `merge:completed` | Branch merged to main | Log; update delivery status |
| `merge:conflict` | Merge conflict detected | Alert human; provide context via note |

When all tasks from a session are merged, `v1_get_session_tasks` returns
`delivery_status: "delivered"`. That is the correct moment to report "plan delivered".

---

## Attention-Driven Monitoring

When webhooks are unavailable, PM agents fall back to polling `v1_get_attention_items`:

```typescript
async function runAttentionCheck(projectId: string) {
  const { escalated_reviews, failed_tasks, merge_conflicts } = await mcp.call(
    'v1_get_attention_items',
    { project_id: projectId },
  );

  // Handle escalated reviews — exceptional triage required
  for (const item of escalated_reviews) {
    await handleEscalation(item);
  }

  // Handle failed tasks — retry if appropriate
  for (const item of failed_tasks) {
    await mcp.call('v1_retry_task', { task_id: item.task_id });
  }

  // Handle merge conflicts — notify human
  for (const item of merge_conflicts) {
    await notifyHuman(item);
  }
}

// Run every 10 minutes as fallback (primary is webhooks)
setInterval(() => runAttentionCheck(PROJECT_ID), 10 * 60 * 1000);
```

---

## Pipeline Control Tools

Agents can intervene in the pipeline when necessary:

| Tool | When to use |
|------|-------------|
| `v1_pause_task` | Detected problem mid-execution; needs human review |
| `v1_cancel_task` | Task superseded or requirements changed |
| `v1_retry_task` | Stalled task; worker crashed unexpectedly |
| `v1_resume_scheduling` | Unpaused project after human intervention |
| `v1_resolve_escalation` | After human resolves an escalated issue |
| `v1_create_task_note` | Annotate any task with agent progress or observations |

**Conservative intervention:** Agents should prefer observation over intervention. Only pause or cancel with a documented reason via `v1_create_task_note`.

```typescript
// ✅ Pause with documented reason
await mcp.call('v1_create_task_note', {
  task_id,
  note: 'Pausing: detected dependency on task-xyz which is not yet complete.',
});
await mcp.call('v1_pause_task', { task_id });

// ❌ Pause without explanation
await mcp.call('v1_pause_task', { task_id });
```

---

## Batch Status Monitoring

For projects with many tasks, use batch status checking efficiently:

```typescript
// Check up to 50 tasks in one call
const { tasks } = await mcp.call('v1_batch_task_status', {
  task_ids: ['task-1', 'task-2', ...],
});

// Tasks needing attention
const needsAction = tasks.filter(t =>
  t.status === 'Escalated' || t.status === 'Blocked'
);
```

---

## Reconnect and Resilience

When the MCP connection drops and reconnects:

```typescript
async function onMcpReconnect() {
  // 1. Re-register webhook (idempotent — same URL returns same webhook_id)
  await mcp.call('v1_register_webhook', { url: GATEWAY_URL });

  // 2. Backfill missed events via cursor
  const { events } = await mcp.call('v1_get_recent_events', {
    project_id: PROJECT_ID,
    cursor: lastSeenCursor,
    limit: 100,
  });

  // 3. Process missed events in order
  for (const event of events) {
    await dispatcher.dispatch(event);
    lastSeenCursor = event.id;
  }

  // 4. Check attention items for anything requiring immediate action
  await runAttentionCheck(PROJECT_ID);
}
```

---

## Design Principles

| Principle | Detail |
|-----------|--------|
| **Autonomous navigation** | Agents use MCP tools to read current state — no internal state mirror |
| **Approval authority is policy-driven** | Agents may approve at `review_passed` only when their current integration/policy grants authority |
| **Observe before act** | Annotate with v1_create_task_note before any intervention |
| **Fire-and-forget dispatch** | Webhook events route to agents; agents decide what to do |
| **Graceful degradation** | Webhooks primary; cursor polling as fallback |
| **Conservative intervention** | Pause/cancel only with documented reason |
| **Idempotent reconnect** | Re-register webhook + backfill cursor on every reconnect |
