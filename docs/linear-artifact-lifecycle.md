# Linear Artifact Lifecycle

This document describes the lifecycle of the Linear artifact surface in the Agents workspace, how it overlaps with existing artifact surfaces, and the edge cases that matter for debugging.

## Scope

The Linear “artifact” is not a row in the generic `artifacts` table. It is an artifact-pane tab mounted alongside Plan, Verification, Proposals, Tasks, Jira, and Commit & Publish. Its durable state is a per-conversation Linear issue link stored in `agent_conversation_linear_issue_links`.

Primary code paths:

- Frontend tab shell: `frontend/src/components/agents/AgentsArtifactPane.tsx`
- Linear panel: `frontend/src/components/agents/AgentsLinearIssuePanel.tsx`
- Linear API client wrappers: `frontend/src/api/linear.ts`
- Linear tab query keys: `frontend/src/components/agents/agentLinearIssueQueries.ts`
- Backend commands: `src-tauri/src/commands/linear_commands.rs`
- Link lifecycle helpers: `src-tauri/src/application/agent_conversation_linear_issue.rs`
- Chat auto-assignment/prompt merge: `src-tauri/src/application/chat_service/mod.rs`
- Queued-message auto-assignment/prompt merge: `src-tauri/src/application/chat_service/chat_service_queue.rs`
- Persistence: `src-tauri/src/infrastructure/sqlite/sqlite_agent_conversation_linear_issue_repo.rs`
- Migration/backfill: `src-tauri/src/infrastructure/sqlite/migrations/v20260618181405_agent_conversation_linear_issue_links.rs`
- Webhook reconciliation: `src-tauri/src/application/linear_webhook_reconciliation_service.rs`

## Conceptual Model

There are three separate concepts that can look like one thing in the UI:

1. Artifact-pane tabs:
   Plan, Verification, Proposals, Tasks, Jira, Linear, and Commit & Publish share one right-side pane and one active-tab state.

2. Ideation artifacts:
   Plan/Verification/Proposals/Tasks are derived from an attached ideation session and plan branch. These are “real” ideation artifacts or task surfaces.

3. External issue links:
   Linear and Jira are external integration tabs. The Linear tab reads and writes an assigned Linear issue link for the selected conversation.

The Linear tab therefore overlaps the artifact pane, but it does not participate in ideation artifact availability. It is always external-integration state scoped to `conversation_id`.

## Lifecycle

### 1. Integration Configuration

Linear availability starts in backend integration settings.

The frontend calls `linearApi.getSettings()`, which invokes `get_linear_integration_settings`. The returned settings include:

- `enabled`
- `hasApiToken`
- `validationStatus`
- `issueSearchAvailable`
- `lastValidatedAt`
- `lastError`
- `updatedAt`

The artifact pane shows the Linear tab only when:

- the conversation is not a direct ideation conversation, and
- Linear settings are enabled, and
- issue search is available.

Code: `AgentsArtifactPane.tsx` computes `showLinearTab` from `linearApi.getSettings()`.

Edge cases:

- If settings are still loading, the Linear tab may appear after the pane initially renders.
- If validation fails or `issueSearchAvailable` is false, the Linear tab is absent even if a conversation already has a stored Linear link.
- Direct ideation conversations suppress Linear/Jira tabs; project/workspace conversations can show them.

### 2. Tab Assembly And Active Tab Selection

`AgentsArtifactPane.tsx` assembles visible tabs in this order:

1. Ideation tabs: Plan, Verification, Proposals, Tasks
2. Jira, if configured
3. Linear, if configured
4. Commit & Publish, if the workspace supports publishing

The pane receives `activeTab` from the per-conversation artifact state. If that tab is not currently visible, `effectiveActiveTab` falls back to the first visible tab, or `plan`.

Important overlap:

- Linear shares the same persisted active-tab state as Plan/Tasks/etc.
- A stale persisted `linear` tab can hide newly available Plan/Verification/Tasks unless sanitized.
- Current behavior sanitizes persisted stale external tabs when ideation tabs are available, preferring `tasks`, then `plan`, then the first ideation tab. Optimistic/manual user tab clicks still win, so explicitly clicking Linear remains valid.

### 3. Opening The Linear Tab

When `effectiveActiveTab === "linear"`, `ArtifactContent` lazy-loads `AgentsLinearIssuePanel`.

The panel receives:

- `conversationId`
- `projectId`

No ideation session id, plan branch id, or execution plan id is used by the Linear panel.

Edge cases:

- If `conversationId` is null, the panel displays “No conversation selected.”
- The tab can exist without an assigned issue; in that state it shows search.
- Switching conversations changes the query key, so the panel reads the selected conversation’s link.

### 4. Loading Existing Assignment

`AgentsLinearIssuePanel` runs:

```ts
linearApi.getAgentConversationLinearIssue({ conversationId })
```

Backend command:

```rs
get_agent_conversation_linear_issue
```

Storage lookup:

```sql
SELECT * FROM agent_conversation_linear_issue_links
WHERE conversation_id = ?
```

If a link exists, the panel renders issue details. If it does not, the panel renders search.

Edge cases:

- A link can be present but `refresh_status` may be `not_loaded`, meaning it has metadata from assignment/backfill but not fully fetched Linear content.
- `comments` and `attachments` are persisted JSON fields but currently returned as generic JSON arrays; the panel does not render a rich comment/attachment feed.

### 5. Search

When no issue is assigned, or when the user clicks Reassign, the panel shows a search input.

Search runs only when:

- there is a `conversationId`,
- search mode is visible,
- trimmed query length is at least 2.

Frontend:

```ts
linearApi.searchIssues({ query, limit: 12 })
```

Backend:

```rs
search_linear_issues
```

The backend short-circuits blank queries and otherwise calls `LinearIntegrationService::search_issues`.

Edge cases:

- Empty/whitespace query returns no results without calling Linear.
- Disabled/misconfigured Linear can make search fail through the integration service.
- Search results are cached by trimmed query for 10 seconds.
- Multiple issues with same key/id are keyed by `linear:${id}:${key}` or `linear:${id}` in the result list.

### 6. Manual Assignment

Clicking a search result calls:

```ts
linearApi.assignAgentConversationLinearIssue({
  conversationId,
  projectId,
  issueId,
  issueKey,
  title,
  issueUrl,
})
```

Backend command:

```rs
assign_agent_conversation_linear_issue
```

Backend behavior:

1. Parses and validates `conversationId`.
2. Rejects blank issue ids and issue ids containing null, newline, or carriage return.
3. Resolves `project_id` from explicit input, workspace, or project conversation context.
4. Builds a manual link with `manually_assigned = true`.
5. Upserts by `conversation_id`.
6. Refreshes the issue by default unless `refresh: false` is passed.

Persistence behavior:

- `upsert` uses `conversation_id` as the primary key.
- A manual assignment replaces any previous Linear issue assignment for that conversation.
- `assigned_at`, `created_at`, and `updated_at` are stored on the link.

Edge cases:

- If no project can be resolved, assignment fails.
- If refresh fails, the link can still exist with `refresh_status = error`.
- Manual assignment can overwrite an automatically assigned issue.

### 7. Automatic Assignment From Composer References

The composer can produce Linear integration references from `@linear:` selections. These travel as `composer_integration_references`.

Automatic assignment runs after user message creation in both immediate and queued send paths:

- `ChatService::auto_assign_primary_linear_issue_from_turn`
- queue handling in `chat_service_queue.rs`

Backend behavior:

1. If there are no integration references, do nothing.
2. Resolve the conversation’s project id from workspace or project context.
3. Extract the first valid Linear reference from the turn.
4. Insert only if absent.
5. Refresh through `LinearIntegrationService` when available.

Important overlap with Jira:

- Jira and Linear assignments are provider-scoped and can coexist.
- The backend first assigns/merges Jira, then assigns/merges Linear.
- Linear deduplication only dedupes Linear references by issue id or case-insensitive key.

Edge cases:

- Only the first valid Linear reference is used as the primary conversation assignment.
- Invalid Linear references are ignored, not fatal.
- Existing conversation assignment is preserved; auto-assignment uses insert-if-absent.
- If a message includes both Jira and Linear references, both can be assigned.

### 8. Assigned Reference Injection Into Runtime Prompts

When composing runtime prompt content, the chat service loads assigned Jira and Linear issue links for the conversation.

It then:

1. Merges the assigned Jira issue into turn references.
2. Merges the assigned Linear issue into the result.
3. Expands integration references into runtime prompt context.

This means a previously assigned Linear issue can remain visible to the agent even when the current user message does not explicitly mention `@linear:...`.

Edge cases:

- The assigned Linear issue is prepended before current turn Linear references.
- Duplicate Linear references are removed if they match by id or key.
- Cross-provider references are preserved.
- If the assigned link is stale, the prompt can include stale metadata until refresh updates it.

### 9. Refresh

Refresh paths:

- Silent auto-refresh in the panel when `refreshStatus === "not_loaded"`.
- Manual refresh button in the panel.
- Refresh during manual assignment unless disabled.
- Refresh during automatic assignment when integration service is available.

Backend refresh:

```rs
refresh_linear_issue_link
```

On success:

- updates `issue_id`, key, URL, title, status, assignee, reporter, remote updated timestamp,
- stores body as both `description_markdown` and `description_text`,
- resets comments/attachments to empty arrays,
- sets `last_refreshed_at`,
- sets `refresh_status = loaded`,
- clears `refresh_error`.

On failure:

- preserves the link,
- sets `refresh_status = error`,
- stores `refresh_error`,
- updates `updated_at`.

Edge cases:

- A failed refresh should not remove assignment.
- Panel displays refresh errors under issue details.
- If Linear API returns a canonical id/key different from the selected summary, refresh can update the stored identity fields.

### 10. Display

The panel displays:

- key or id
- title
- status
- assignee
- creator/reporter
- remote updated timestamp
- markdown or text description
- refresh error

The header actions are:

- open Linear issue URL in a new browser tab,
- refresh,
- unlink.

Edge cases:

- If there is no `issueUrl`, the open button is hidden.
- `updatedAtRemote` is formatted with `Date`; invalid values display raw.
- Markdown is rendered through the same chat markdown components, so untrusted content must stay within markdown rendering constraints.

### 11. Clear / Unlink

Unlink calls:

```ts
linearApi.clearAgentConversationLinearIssue({ conversationId })
```

Backend command:

```rs
clear_agent_conversation_linear_issue
```

Storage:

```sql
DELETE FROM agent_conversation_linear_issue_links
WHERE conversation_id = ?
```

Frontend cache is set to `null`, search mode returns, and the panel shows “Linear issue unlinked.”

Edge cases:

- Clearing a missing link is effectively successful.
- Clearing Linear does not clear Jira, ideation artifacts, publish state, or task links.

### 12. Migration And Backfill

Migration `v20260618181405_agent_conversation_linear_issue_links` creates the Linear link table and backfills from existing user message metadata.

Backfill behavior:

1. Finds agent/workspace conversations with project context.
2. Skips conversations that already have a Linear link.
3. Scans user messages in chronological order.
4. Extracts the first primary Linear issue from composer metadata.
5. Inserts a `not_loaded`, non-manual link.
6. Stops after the first inserted link for that conversation.

Edge cases:

- Backfill depends on old metadata being parseable.
- Backfill does not fetch Linear content; the panel later silently refreshes `not_loaded` links.
- If a conversation has multiple historical Linear references, only the first primary one is linked.

### 13. Webhook Reconciliation

Linear webhooks are separate from the conversation Linear artifact tab. They reconcile external Linear issue changes into task/workflow state through external issue links.

Webhook lifecycle:

1. Verify signing secret.
2. Parse webhook body.
3. Enforce freshness window.
4. Deduplicate delivery.
5. For Issue events, map Linear state to workflow status.
6. If an external issue link maps to a task, transition the task.
7. For comment/attachment activity, record activity.

Important distinction:

- Conversation Linear issue links identify the issue assigned to a chat.
- Webhook external issue links identify Linear issues linked to tasks.
- These may refer to the same Linear issue but are separate storage/lifecycle systems.

Edge cases:

- Missing/invalid signature rejects the webhook.
- Stale timestamp rejects the webhook.
- Duplicate delivery returns a duplicate outcome without reprocessing.
- If no task is linked, the webhook records/no-ops rather than affecting the conversation artifact.
- If Linear state has no workflow mapping, no task transition occurs.

## Overlap With Existing Artifact Surfaces

### Plan / Verification / Proposals / Tasks

These tabs depend on an attached ideation session with a plan artifact. Tasks additionally depend on an accepted/converted/execution-backed plan.

Linear does not require:

- `attachedSessionId`
- `planArtifactId`
- `linkedPlanBranchId`
- `executionPlanId`

Overlap risk:

- All tabs share a single right pane and a single `activeTab`.
- Linear settings can load after ideation tabs, changing visible tabs.
- Persisted `linear` can hide plan/task tabs unless stale external tabs are sanitized.

Expected UX:

- If a top-level workspace conversation has active ideation artifacts, opening the artifact pane should prioritize Plan/Tasks over stale Linear.
- The user can still explicitly click Linear.

### Jira

Jira and Linear are sibling external issue tabs.

Shared patterns:

- Settings-gated tab availability.
- Per-conversation issue link.
- Search, assign, refresh, clear.
- Auto-assignment from composer references.
- Assigned reference injection into runtime prompt.

Differences:

- Jira uses Atlassian settings and Jira-specific issue model.
- Linear uses Linear settings and Linear-specific webhook reconciliation.
- Jira and Linear assignments coexist; neither should overwrite the other.

### Commit & Publish

Commit & Publish is workspace/PR lifecycle state, not issue state.

Overlap:

- It is also a tab in the same artifact pane.
- The header hides the publish shortcut while any artifact pane is open.
- Persisted `publish` can be a stale external tab in ideation contexts and is sanitized like Jira/Linear when ideation tabs are available.

### Header Shortcuts

Header artifact shortcuts only expose ideation tabs from `availableArtifactTabs`. They do not expose Linear.

Implication:

- Linear is reachable from the right artifact pane tab row, not the header shortcut row.
- Header shortcut behavior can differ from pane tab availability.

## Edge Case Matrix

| Area | Edge Case | Expected Behavior | Risk |
|---|---|---|---|
| Settings | Linear disabled after a link exists | Linear tab hidden; stored link remains | User may not see assigned issue until re-enabled |
| Settings | Settings query resolves late | Linear tab appears after initial render | Active tab fallback may change visible content |
| Tab state | Persisted `linear` from prior session | Sanitized to Tasks/Plan when ideation tabs exist | Without sanitization, it hides active ideation artifacts |
| Tab state | User explicitly clicks Linear | Optimistic state keeps Linear active | Manual choice must not be sanitized away |
| Direct ideation | Conversation context is direct ideation | Linear tab hidden | External issue assignment is conversation/workspace-oriented |
| Assignment | Empty/invalid issue id | Backend rejects assignment | Prevents malformed persisted link |
| Assignment | No project can be resolved | Backend rejects assignment | Manual panel needs project/workspace context |
| Auto-assignment | Multiple Linear references in one message | First valid reference wins | Later references are only runtime context, not primary assignment |
| Auto-assignment | Existing link already present | Insert-if-absent preserves it | New `@linear:` does not replace primary link automatically |
| Reassign | User manually assigns different issue | Upsert replaces current conversation link | Manual action overrides auto-assigned link |
| Refresh | Linear API unavailable | Link remains with `refresh_status = error` | UI shows stale/minimal metadata plus error |
| Refresh | Not-loaded backfilled link | Panel silently refreshes | First open may briefly show partial metadata |
| Prompt context | Current message has no Linear reference | Assigned issue is still injected | Agent remains aware of assigned issue |
| Prompt context | Current message references same Linear issue | Dedupes by id/key | Prevents duplicate context |
| Cross-provider | Jira and Linear both assigned | Both are merged into prompt context | Ordering is Jira merge then Linear merge |
| Webhook | Linear issue state changes | Task sync only if external issue link maps to task | Conversation Linear tab is not automatically the task link |
| Webhook | Duplicate delivery | No repeated action | Delivery store must be durable |
| Migration | Historical metadata has multiple Linear refs | First primary ref backfilled | May not match current user intent |
| Cache | Query key uses conversation id | Switching conversation loads different link | Bad/missing conversation id can show empty state |

## Debug Checklist

When the Linear tab hides expected Plan/Verification/Tasks:

1. Check `availableArtifactTabs` from `useAgentsAttachedIdeation`.
2. Check persisted artifact state for the conversation in `agentSessionStore.artifactByConversationId`.
3. Confirm `useResolvedAgentArtifactState(..., availableArtifactTabs)` is receiving the available tabs in `AgentsArtifactPaneRegion`.
4. Confirm the user has not explicitly clicked Linear in the current session, because optimistic tab state intentionally wins.
5. Confirm the workspace link points to the productive ideation session/plan branch, not a blank continuation session.

When Linear tab is missing:

1. Check `linearApi.getSettings()`.
2. Confirm `enabled === true`.
3. Confirm `issueSearchAvailable === true`.
4. Confirm the selected conversation is not a direct ideation conversation.
5. Confirm the pane is mounted and `showLinearTab` is true.

When assignment is missing:

1. Query `agent_conversation_linear_issue_links` by `conversation_id`.
2. Inspect user message metadata for `composer_integration_references`.
3. Check whether auto-assignment had a project id.
4. Check refresh status and refresh error.

## Current Alignment Notes

- Linear is correctly modeled as a sibling external tab, not as an ideation artifact.
- The highest-risk overlap is shared active-tab state across external tabs and ideation tabs.
- The current tab-state sanitizer should preserve expected UX: active ideation work shows Plan/Tasks instead of stale Linear, while explicit Linear selection remains possible.
- Webhook reconciliation should not be treated as updating the conversation Linear artifact; it updates task/workflow state through external issue links.
