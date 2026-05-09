/**
 * Static guide content for v1_get_agent_guide.
 * All content is pure markdown — no backend dependency, no state dependency.
 *
 * IMPORTANT: ALL_TOOL_NAMES must stay in sync with TOOL_CATEGORIES in index.ts.
 * The bidirectional sync test validates: TOOL_CATEGORIES ↔ ALL_TOOL_NAMES ↔ FULL_GUIDE content.
 */
export const GUIDE_SECTIONS = {
    setup: `## Setup: Project Registration (1 tool)

| Tool | Purpose | Required Args | Permission | Next Step |
|------|---------|---------------|------------|-----------|
| v1_register_project | Register a folder as a RalphX project | working_directory | CREATE_PROJECT (bit 8) | v1_list_projects |

### Notes

- Requires \`CREATE_PROJECT\` permission (bit 8). Keys with only READ/WRITE/ADMIN cannot call this tool.
- \`working_directory\` must be an absolute path under the user's home directory. System paths (/etc, /usr, /var, /tmp, /private) are rejected.
- Directory is created automatically if it doesn't exist (including parent directories).
- Git is initialized automatically if no \`.git\` directory exists. An empty initial commit is created if no commits exist.
- The creating API key automatically gets access to the new project — no need to update key scope manually.
- **Stale permission cache**: Permission changes take up to 30s to take effect (TTL cache). If you just granted CREATE_PROJECT to a key, wait 30s before calling this tool.
- After successful registration, the scope cache is immediately invalidated — subsequent calls see the new project scope without waiting for TTL expiry.
`,
    overview: `## Overview: How RalphX Works

RalphX is an autonomous software development platform. You are an engineer-agent connecting via the External MCP API. RalphX manages the full dev lifecycle: ideation → task creation → autonomous code execution → review → merge.

### The Workflow (5 Steps)

1. **Discover** — Find your projects and current state
   - \`v1_list_projects\` → pick project_id
   - \`v1_get_project_status\` → see task counts and agent activity
   - \`v1_get_pipeline_overview\` → see tasks by stage

2. **Ideate** — Create a session and describe what to build
   - \`v1_start_ideation\` → spawns an orchestrator agent with your prompt
   - Poll \`v1_get_ideation_status\` until \`agent_status: "waiting_for_input"\`
   - \`v1_get_ideation_messages\` → read orchestrator replies
   - \`v1_send_ideation_message\` → refine the plan

3. **Plan** — Review and verify the plan
   - \`v1_list_proposals\` → see proposed tasks
   - \`v1_get_plan\` → read full plan artifact
   - \`v1_trigger_plan_verification\` → start adversarial review
   - \`v1_get_plan_verification\` → check verification status

4. **Accept** — Commit the plan and start execution
   - \`v1_accept_plan_and_schedule\` → creates tasks + starts pipeline (idempotent — returns existing task IDs if already accepted)
   - On failure: \`v1_resume_scheduling\` → resume from last successful step

5. **Supervise** — Monitor and manage the pipeline
   - \`v1_get_session_tasks\` → track delivery progress (delivery_status + task list)
   - \`v1_get_attention_items\` → tasks needing your action
   - \`v1_get_review_summary\` + \`v1_get_task_diff\` → inspect completed work
   - \`v1_approve_review\` or \`v1_request_changes\` → make the review approval decision when your integration/policy allows it
   - \`v1_get_recent_events\` → cursor-based real-time activity

### Flow 0: Onboarding (1 tool)

| Tool | Purpose | Required Args | Next Step |
|------|---------|---------------|-----------|
| v1_get_agent_guide | Read this guide | — (optional: section) | v1_list_projects |
`,
    discovery: `## Flow 1: Discovery (3 tools)

| Tool | Purpose | Required Args | Preconditions | Next Step |
|------|---------|---------------|---------------|-----------|
| v1_list_projects | Find your projects | — | Valid API key | v1_get_project_status |
| v1_get_project_status | Project details + task counts + agent status | project_id | — | v1_start_ideation |
| v1_get_pipeline_overview | Tasks grouped by pipeline stage | project_id | — | v1_get_task_detail |

### Notes

- \`v1_list_projects\` returns only projects accessible to your API key
- \`v1_get_project_status\` includes: active tasks, queued tasks, running agents, last activity
- \`v1_get_pipeline_overview\` groups tasks by stage: executing, pending_review, reviewing, pending_merge, merging, completed
`,
    ideation: `## Flow 2: Ideation & Planning (15 tools)

| Tool | Purpose | Required Args | Preconditions | Next Step |
|------|---------|---------------|---------------|-----------|
| v1_start_ideation | Create session + spawn orchestrator | project_id, prompt | — | v1_get_ideation_status |
| v1_get_ideation_status | Session status + agent state + proposal count | session_id | Session exists | v1_get_ideation_messages |
| v1_send_ideation_message | Message the orchestrator | session_id, message | agent_status = waiting_for_input | v1_get_ideation_status |
| v1_get_ideation_messages | Read orchestrator replies | session_id | Session exists | v1_send_ideation_message |
| v1_list_ideation_sessions | List sessions for a project | project_id | — | v1_get_ideation_status |
| v1_get_session_tasks | List tasks created from a session | session_id | Session accepted | v1_get_task_detail |
| v1_append_task_to_plan | Append one-off task to accepted active plan | session_id, title, steps, acceptance_criteria | Session accepted; plan still open, including open PR wait | v1_get_session_tasks |
| v1_list_proposals | Proposals in session | session_id | Session has proposals | v1_get_proposal_detail |
| v1_get_proposal_detail | Full proposal + steps + acceptance criteria | proposal_id | — | v1_modify_proposal |
| v1_get_plan | Plan artifact content | session_id | Session has plan | v1_trigger_plan_verification |
| v1_modify_proposal | Update proposal before acceptance | proposal_id, changes | Session active | — |
| v1_analyze_dependencies | Proposal dependency graph | session_id | Has proposals | v1_accept_plan_and_schedule |
| v1_trigger_plan_verification | Start adversarial review loop | session_id | Session has plan | v1_get_plan_verification |
| v1_get_plan_verification | Verification status + gap counts | session_id | Verification triggered | v1_accept_plan_and_schedule |
| v1_accept_plan_and_schedule | Apply proposals → tasks → schedule (saga) | session_id | Plan + proposals ready | v1_get_task_detail |
| v1_get_session_tasks | Tasks created from a session + delivery_status | session_id | Session exists | v1_get_task_detail |

### Polling Pattern

\`\`\`
v1_start_ideation → poll v1_get_ideation_status (5-10s interval)
  → agent_status: "waiting_for_input" → v1_get_ideation_messages
  → v1_send_ideation_message to iterate
  → when satisfied → v1_trigger_plan_verification
  → poll v1_get_plan_verification → verified / converged
  → v1_accept_plan_and_schedule
\`\`\`

### agent_status Values

| Status | Meaning | Action |
|--------|---------|--------|
| \`idle\` | Agent not running | Session may be complete or errored |
| \`generating\` | Agent producing output | Wait, then poll again |
| \`waiting_for_input\` | Agent awaiting your message | Safe to send |

### v1_get_session_tasks Output Shape

After \`v1_accept_plan_and_schedule\`, use \`v1_get_session_tasks\` to track delivery progress.

Response:
\`\`\`json
{
  "session_id": "...",
  "task_count": 3,
  "delivery_status": "in_progress",
  "tasks": [
    {
      "id": "task-uuid",
      "title": "Add dark mode toggle",
      "status": "executing",
      "proposal_id": "proposal-uuid",
      "category": "regular",
      "priority": 50,
      "created_at": "2026-01-01T00:00:00Z"
    }
  ]
}
\`\`\`

### Session Lifecycle & delivery_status

Session statuses: \`active\` → \`accepted\` (after \`v1_accept_plan_and_schedule\`)

After acceptance, track delivery via \`delivery_status\` (returned by both \`v1_get_session_tasks\` and \`v1_get_ideation_status\`):

| delivery_status | Meaning |
|-----------------|---------|
| \`not_scheduled\` | Session accepted but no tasks created yet |
| \`in_progress\` | At least one task is still executing, queued, or in merge pipeline |
| \`pending_review\` | No active tasks; some are awaiting review/approval |
| \`partial\` | Some tasks merged; rest are terminal (cancelled/failed/stopped) |
| \`delivered\` | All tasks merged to main |

### v1_accept_plan_and_schedule Idempotency

Calling \`v1_accept_plan_and_schedule\` on an already-accepted session is safe — it returns the existing task IDs instead of failing. Use this to recover task IDs if the original call response was lost.

### Appending one-off tasks after acceptance

Use \`v1_append_task_to_plan\` only for small follow-up work after a plan has been accepted while the plan branch is still active. Open PR / waiting-on-PR plans are still open and can receive appended tasks. The backend links the new task to the existing session/execution plan and blocks the plan merge on it. Once the PR/plan is closed, merged, terminal, or actively merging/repairing, start a new ideation continuation instead of appending.
`,
    tasks: `## Flow 2b: Task Operations (2 tools)

| Tool | Purpose | Required Args | Preconditions | Next Step |
|------|---------|---------------|---------------|-----------|
| v1_get_task_steps | Task step progress | task_id | — | — |
| v1_batch_task_status | Multiple task statuses (max 50) | task_ids[] | — | v1_get_task_detail |

### Notes

- \`v1_get_task_steps\` returns each step with title, status (pending/in_progress/completed/failed/skipped), and completion notes
- \`v1_batch_task_status\` returns \`tasks[]\` (found) + \`errors[]\` with \`reason: not_found | access_denied\`
- Use \`v1_batch_task_status\` when monitoring many tasks to minimize API calls
`,
    pipeline: `## Flow 3: Pipeline Supervision (11 tools)

| Tool | Purpose | Required Args | Preconditions | Next Step |
|------|---------|---------------|---------------|-----------|
| v1_get_task_detail | Full task info + steps + branch | task_id | — | v1_get_task_diff |
| v1_get_task_diff | Git diff stats for task branch | task_id | Task has branch | v1_get_review_summary |
| v1_get_review_summary | Review findings + notes | task_id | Task in review | v1_approve_review |
| v1_approve_review | Record approval decision → start merge | task_id | ReviewPassed/Escalated state and authority allows | — |
| v1_request_changes | Send back for re-execution with feedback | task_id, feedback | ReviewPassed/Escalated state and authority allows | — |
| v1_resolve_escalation | Handle escalated review | task_id, resolution | Escalated state and authority allows | — |
| v1_get_merge_pipeline | All merge activity for project | project_id | — | — |
| v1_pause_task | Pause running task | task_id | Task running | — |
| v1_cancel_task | Cancel task | task_id | Task active | — |
| v1_retry_task | Retry failed/stopped task | task_id | Task failed/stopped | — |
| v1_resume_scheduling | Resume failed accept_plan_and_schedule | session_id | Previous accept failed | v1_get_task_detail |
| v1_create_task_note | Annotate task with progress note | task_id, note | — | — |

### Task States

\`\`\`
pending → executing → pending_review → reviewing → pending_merge → merging → completed
                         ↓ (escalated)      ↓ (request_changes)
                      escalated          re_executing
\`\`\`

### Review vs Merge

- \`review_passed\` is the approval-decision point
- \`pending_merge\` / \`merge:ready\` are merge-pipeline stages after approval
- \`delivery_status = "delivered"\` means all tasks merged to main

Do not turn \`merge:ready\` into a second generic approval prompt.

### v1_resolve_escalation resolution values
- \`"approve"\` — approve and proceed to merge
- \`"request_changes"\` — send back for re-execution
- \`"cancel"\` — cancel the task
`,
    events: `## Flow 4: Events & Monitoring (8 tools)

| Tool | Purpose | Required Args | Preconditions | Next Step |
|------|---------|---------------|---------------|-----------|
| v1_subscribe_events | SSE stream of state changes | project_id (optional) | — | — |
| v1_get_recent_events | Cursor-based event fetch from DB | last_id, project_id (optional) | — | — |
| v1_get_attention_items | Tasks needing action (reviews, conflicts) | project_id (optional) | — | v1_get_task_detail |
| v1_get_execution_capacity | Can more work run? Running/queued counts | project_id | — | v1_start_ideation |
| v1_register_webhook | Register URL to receive pipeline events via HTTP POST | url, project_id | — | v1_list_webhooks |
| v1_unregister_webhook | Remove a registered webhook URL | webhook_id | — | v1_list_webhooks |
| v1_list_webhooks | List all registered webhooks and their status | project_id (optional) | — | — |
| v1_get_webhook_health | Check delivery health for a webhook | webhook_id | — | — |

### Cursor-Based Polling

\`\`\`typescript
// Start: last_id = 0 for all recent events
let lastId = 0;
while (true) {
  const events = await v1_get_recent_events({ last_id: lastId, project_id });
  for (const event of events) {
    lastId = Math.max(lastId, event.id);
    // process event
  }
  await sleep(30_000); // 30s polling interval
}
\`\`\`

### Event Types

| Event | Meaning |
|-------|---------|
| \`task:status_changed\` | Task moved to new pipeline stage |
| \`task:execution_completed\` | Task execution complete |
| \`review:escalated\` | Review needs exceptional decision |
| \`merge:completed\` | Task merged to main |
| \`ideation:session_created\` | New ideation session started |
`,
    resilience: `## Resilience & Best Practices

### Session Management

**Always check before creating.** Call \`v1_list_ideation_sessions\` before \`v1_start_ideation\` to avoid creating duplicate sessions.

\`\`\`
v1_list_ideation_sessions({ project_id }) → check for active sessions
  → if none needed: v1_start_ideation({ project_id, prompt, idempotency_key: "my-unique-key" })
  → if active session exists: resume via v1_get_ideation_status
\`\`\`

- Use \`idempotency_key\` for safe retries — calling \`v1_start_ideation\` twice with the same key returns the existing session
- Check \`existing_active_sessions\` in the \`v1_start_ideation\` response — the API reports what's already running
- If \`duplicate_detected: true\` in response, use the returned session instead of re-creating

### Read-Before-Write

**Always read before sending.** The API enforces this: \`v1_send_ideation_message\` returns 409 if unread agent responses exist.

\`\`\`
Poll v1_get_ideation_status
  → agent_status: "waiting_for_input" AND unread_message_count > 0
    → MUST call v1_get_ideation_messages first (clears read cursor)
    → then call v1_send_ideation_message
  → agent_status: "waiting_for_input" AND unread_message_count == 0
    → safe to call v1_send_ideation_message directly
\`\`\`

| 409 Error | Meaning | Action |
|-----------|---------|--------|
| \`unread_messages\` | Agent replied; you haven't read it yet | Call \`v1_get_ideation_messages\` then retry |

### Following next_action Hints

**Every response includes \`next_action\`.** Follow it — it tells you exactly what to call next.

| next_action | Call | When |
|-------------|------|------|
| \`poll_status\` | \`v1_get_ideation_status\` | Agent is working or just received your message |
| \`fetch_messages\` | \`v1_get_ideation_messages\` | Agent replied; unread messages exist |
| \`send_message\` | \`v1_send_ideation_message\` | Agent is ready and waiting for input |
| \`wait\` | (sleep 5-10s, then poll) | Agent is generating output |
| \`use_existing_session\` | Resume existing session | Duplicate session detected |

### Capacity Checking

**Check capacity before accepting a plan.** Call \`v1_get_execution_capacity\` before \`v1_accept_plan_and_schedule\` to confirm the system can handle new work.

\`\`\`
v1_get_execution_capacity({ project_id })
  → { can_accept_work: true, running: N, queued: M }
  → if can_accept_work: proceed with v1_accept_plan_and_schedule
  → if !can_accept_work: wait, then re-check
\`\`\`

### Reconnect Pattern

**On MCP client reconnect**, restore state in this order:

\`\`\`
1. v1_list_webhooks({ project_id }) → re-register any missing webhooks (idempotent)
2. v1_get_recent_events({ last_id: lastKnownId, project_id }) → backfill missed events via cursor
3. v1_list_ideation_sessions({ project_id }) → find sessions started before disconnect
4. Resume normal polling loop
\`\`\`

### Session Lifecycle (external_activity_phase)

The \`external_activity_phase\` field in \`v1_get_ideation_status\` tracks session progress for external agents:

| Phase | Meaning | Expected Action |
|-------|---------|-----------------|
| \`created\` | Session started, no messages yet | Send initial prompt |
| \`planning\` | First message sent; agent working on plan | Poll status |
| \`proposing\` | Agent auto-generating task proposals | Poll status |
| \`verifying\` | Plan verification in progress | Poll \`v1_get_plan_verification\` |
| \`ready\` | Plan verified; ready to accept | Call \`v1_accept_plan_and_schedule\` |
| \`error\` | Session encountered an error | Check messages; consider retry |
| \`stalled\` | No activity for extended period | Re-send message or start new session |
| \`null\` | Internal session (not external) | N/A |
`,
    patterns: `## Common Patterns & Anti-Patterns

### Starting a New Feature

\`\`\`
1. v1_list_projects → pick project_id
2. v1_start_ideation({ project_id, prompt: "Add dark mode toggle" })
3. Poll v1_get_ideation_status until agent_status: "waiting_for_input"
4. v1_get_ideation_messages → read orchestrator plan
5. v1_send_ideation_message to iterate (or proceed)
6. v1_trigger_plan_verification → poll v1_get_plan_verification → "converged"
7. v1_accept_plan_and_schedule
8. Monitor via v1_get_recent_events or v1_get_attention_items
\`\`\`

### Reviewing Completed Work

\`\`\`
1. v1_get_attention_items → find tasks needing review
2. v1_get_review_summary → read findings
3. v1_get_task_diff → inspect changes
4. If your integration/policy grants authority: v1_approve_review or v1_request_changes
5. Otherwise report the pending decision instead of asking for a generic merge approval
\`\`\`

### Monitoring Pipeline

\`\`\`
1. v1_get_pipeline_overview → see all tasks by stage
2. v1_get_recent_events → real-time activity (cursor-based)
3. v1_get_execution_capacity → check if more work can run
\`\`\`

### Tracking Delivery After Accept

\`\`\`
1. v1_accept_plan_and_schedule → note returned taskIds (idempotent — safe to re-call)
2. v1_get_session_tasks({ session_id }) → delivery_status + per-task status
   → delivery_status: "in_progress" | "pending_review" | "partial" | "delivered"
3. When delivery_status = "pending_review" → use v1_get_attention_items
4. When delivery_status = "delivered" → all tasks merged; report "plan delivered"
\`\`\`

### Task Reference Formatting

**Rule:** Any task UUID mentioned in a response or status update MUST be paired with the human-readable title.

**Format:** \`task-{short-uuid} ({Title})\`

**Tools to resolve titles:** \`v1_get_task_detail\` (single task) or \`v1_batch_task_status\` (up to 50 tasks)

| ❌ Bare UUID (unreadable) | ✅ UUID + Title (human-readable) |
|--------------------------|----------------------------------|
| \`Executing: task-b15be469\` | \`Executing: task-b15be469 (Regression testing — full suite)\` |
| \`task-a3c91f2d failed\` | \`task-a3c91f2d (Add dark mode toggle) failed\` |

### Anti-Patterns

| ❌ Don't | ✅ Do Instead |
|------------|----------------|
| Create loose project tasks directly | Start with v1_start_ideation; after acceptance, use v1_append_task_to_plan only for small follow-ups while the plan is still open |
| Poll status in tight loop | Use v1_get_recent_events with cursor-based pagination (30s interval) |
| Skip plan verification | Call v1_trigger_plan_verification before accepting |
| Hardcode project_id | Always call v1_list_projects first |
| Send messages without waiting | Check agent_status = "waiting_for_input" before v1_send_ideation_message |
| Accept immediately | Verify plan with v1_trigger_plan_verification first |
| Surface bare task UUIDs | Always include title: \`task-{id} ({Title})\` — resolve via v1_get_task_detail or v1_batch_task_status |

### Sequencing Rules

- \`v1_start_ideation\` → must call before any session tools
- \`v1_send_ideation_message\` → requires \`agent_status: "waiting_for_input"\`
- \`v1_accept_plan_and_schedule\` → requires plan + proposals in session
- \`v1_append_task_to_plan\` → requires an accepted session whose plan branch is active and whose PR/plan is still open; waiting-on-PR is allowed
- \`v1_approve_review\` / \`v1_request_changes\` → requires task in \`review_passed\` or \`escalated\`, plus current authority
- \`v1_trigger_plan_verification\` → requires plan artifact in session
- \`v1_resolve_escalation\` → requires task in \`escalated\`, plus current authority

### Error Handling

| Error | Meaning | Action |
|-------|---------|--------|
| \`missing_argument\` | Required arg not provided | Check inputSchema for required fields |
| \`backend_error\` + status 401 | API key invalid or expired | Check credentials |
| \`backend_error\` + status 403 | Project not in API key scope | Verify project_id |
| \`backend_error\` + status 404 | Resource not found | Verify IDs |
| \`backend_error\` + status 429 | Rate limited | Back off and retry |
| \`backend_error\` + status 503 | Backend unavailable | Retry after delay |
| \`scope_violation\` | project_id not in API key scope | Use v1_list_projects to find accessible projects |
`,
};
export const VALID_SECTIONS = Object.keys(GUIDE_SECTIONS);
export const FULL_GUIDE = `# RalphX Agent Guide

${Object.values(GUIDE_SECTIONS).join("\n\n---\n\n")}`;
/**
 * Canonical list of all MCP tools.
 * Used by tests to verify guide completeness (bidirectional sync with TOOL_CATEGORIES in index.ts).
 *
 * When adding new tools: update TOOL_CATEGORIES in index.ts AND add here AND document in GUIDE_SECTIONS.
 */
export const ALL_TOOL_NAMES = [
    // Setup: Project registration (1)
    "v1_register_project",
    // Flow 0: Onboarding (1)
    "v1_get_agent_guide",
    // Flow 1: Discovery (3)
    "v1_list_projects",
    "v1_get_project_status",
    "v1_get_pipeline_overview",
    // Flow 2: Ideation (15)
    "v1_start_ideation",
    "v1_get_ideation_status",
    "v1_send_ideation_message",
    "v1_get_ideation_messages",
    "v1_list_ideation_sessions",
    "v1_get_session_tasks",
    "v1_append_task_to_plan",
    "v1_list_proposals",
    "v1_get_proposal_detail",
    "v1_get_plan",
    "v1_modify_proposal",
    "v1_analyze_dependencies",
    "v1_trigger_plan_verification",
    "v1_get_plan_verification",
    "v1_accept_plan_and_schedule",
    // Flow 2b: Task Operations (2)
    "v1_get_task_steps",
    "v1_batch_task_status",
    // Flow 3: Pipeline Supervision (11)
    "v1_get_task_detail",
    "v1_get_task_diff",
    "v1_get_review_summary",
    "v1_approve_review",
    "v1_request_changes",
    "v1_resolve_escalation",
    "v1_get_merge_pipeline",
    "v1_pause_task",
    "v1_cancel_task",
    "v1_retry_task",
    "v1_resume_scheduling",
    "v1_create_task_note",
    // Flow 4: Events & Monitoring (8)
    "v1_subscribe_events",
    "v1_get_recent_events",
    "v1_get_attention_items",
    "v1_get_execution_capacity",
    "v1_register_webhook",
    "v1_unregister_webhook",
    "v1_list_webhooks",
    "v1_get_webhook_health",
];
//# sourceMappingURL=guide-content.js.map