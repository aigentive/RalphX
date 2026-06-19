# Ideation Artifact Lifecycle

This document describes how Plan, Verification, Proposals, and Tasks appear in the Agents workspace right-side artifact pane, how they overlap with Commit & Publish and the separate Jira/Linear linked-issues overlay, and the edge cases that matter when a top-level workspace conversation fails to surface ideation artifacts.

## Scope

The ideation artifact tabs are:

- Plan
- Verification
- Proposals
- Tasks

They are shown in the same right-side artifact pane as Commit & Publish. Jira and Linear are not artifact-pane tabs; they live in a separate linked-issues overlay opened from the Ticket button in the chat header.

Primary code paths:

- Parent workspace ideation hydration: `frontend/src/components/agents/useAgentsAttachedIdeation.ts`
- Attached session extraction: `frontend/src/components/agents/attachedIdeationSession.ts`
- Tab availability: `frontend/src/components/agents/agentArtifactTabs.ts`
- Pane shell and content: `frontend/src/components/agents/AgentsArtifactPane.tsx`
- Pane region and active-tab state: `frontend/src/components/agents/AgentsArtifactPaneRegion.tsx`
- Artifact state resolver: `frontend/src/components/agents/agentArtifactState.ts`
- Controller auto-open/tab repair: `frontend/src/components/agents/useAgentsViewController.ts`
- Workspace sync command: `src-tauri/src/commands/unified_chat_commands.rs`
- Plan branch and execution-plan lookup: `frontend/src/api/plan-branch.ts`, `src-tauri/src/commands/plan_branch_commands.rs`

## Conceptual Model

There are two ways a conversation can own ideation artifacts:

1. Direct ideation conversation:
   The active conversation itself has `contextType === "ideation"`. Its `contextId` is the ideation session id.

2. Top-level project/workspace conversation:
   The conversation is project-scoped, but it has a linked or discoverable child ideation session. The workspace row may store:
   - `linked_ideation_session_id`
   - `linked_plan_branch_id`

The top-level workspace case is the fragile one. The UI has to resolve the productive child session from the workspace link and/or transcript, fetch that session’s rich data, then expose ideation tabs in the parent conversation’s right pane.

## Lifecycle

### 1. Workspace Eligibility

`useAgentsAttachedIdeation` decides whether to hydrate ideation data.

Hydration is enabled when:

- the active conversation is a direct ideation conversation, or
- the active conversation is a project conversation and one of these is true:
  - active workspace mode is `ideation`,
  - active workspace mode is `plan`,
  - workspace has `linkedIdeationSessionId`,
  - workspace has `linkedPlanBranchId`.

Edge cases:

- If workspace mode is `edit` and no linked ids exist, ideation hydration does not run.
- If the active workspace query is stale or missing, the UI can temporarily think no ideation session exists.
- A project conversation can have a running plan in the backend but still render no ideation tabs if its workspace row is not linked and recent transcript history no longer contains productive ideation tool output.

### 2. Conversation History Hydration

For project conversations, the hook loads recent conversation history:

```ts
useConversationHistoryWindow(activeConversation.id, { pageSize: 40 })
```

It merges the selected in-memory messages with the fetched history and sorts by `createdAt`.

Purpose:

- The visible message window may not contain the tool call that spawned or resumed ideation.
- Recent history can include tool outputs with session ids, proposal counts, task ids, and plan artifact ids.

Edge cases:

- Only a 40-message window is used. If the productive ideation session appears earlier than that, transcript recovery can fail.
- If the workspace link points to a blank continuation session and history no longer contains the productive session, the frontend has no reliable way to discover the older productive session.
- Message parsing depends on tool call/result shapes remaining compatible.

### 3. Attached Session Resolution

`resolveAttachedIdeationSessionId` resolves the session id.

Rules:

1. Direct ideation conversation returns `conversation.contextId`.
2. Project conversation scans tool calls and content blocks in reverse chronological order.
3. Candidate sessions are scored:
   - proposal/task counts score highly,
   - accepted status scores highly,
   - plan artifact id scores,
   - delivery status scores.
4. The best non-fallback productive candidate wins.
5. If no productive candidate exists, the first candidate wins.
6. If no candidate exists, the workspace fallback `linkedIdeationSessionId` wins.

Edge cases:

- A stale workspace fallback can win if no better candidate is found.
- A blank continuation session can become the fallback and produce no Plan/Verification/Tasks.
- Tool outputs that mention a verification child can be candidates, but child ids score low unless they look like productive planning sessions.
- Text-only UUID extraction scores low but can still win when no structured candidates exist.

### 4. Rich Session Data Fetch

Once a session id is resolved, the UI fetches rich data:

```ts
ideationApi.sessions.getWithData(attachedSessionId)
```

The response is expected to contain:

- session
- proposals
- messages or related session data

The hook rejects stale query data unless `data.session.id === attachedSessionId`.

Refetching runs every 3 seconds while:

- verification is in progress, or
- acceptance status is pending.

Edge cases:

- If the resolved session id is a blank continuation session, `getWithData` succeeds but has no plan artifact/proposals/tasks, so tab availability stays empty.
- If session data is still loading, available tabs can be empty for a short window.
- A stale query response for a previous session is ignored.

### 5. Plan Artifact Gate

Ideation tabs are visible only when there is an attached session and a plan artifact.

`getVisibleIdeationArtifactTabs` returns no tabs unless:

- `hasAttachedIdeationSession === true`, and
- `hasPlanArtifact === true`.

`hasPlanArtifact` is true when:

- `session.planArtifactId` exists, or
- `session.inheritedPlanArtifactId` exists.

When visible, the base tabs are:

- Plan
- Verification
- Proposals

Edge cases:

- A session with proposals but no plan artifact does not show ideation tabs.
- A blank continuation session with no inherited plan artifact shows no ideation tabs.
- Verification tab is shown whenever a plan artifact exists; it no longer depends on verification having already run.

### 6. Tasks Gate

Tasks tab appears when the base plan tabs are visible and `hasExecutionTasks` is true.

`hasExecutionTasks` is true when any of these are true:

- workspace has `linkedPlanBranchId`,
- any proposal has `createdTaskId`,
- session status is `accepted`,
- session acceptance status is `accepted`,
- session has `convertedAt`.

Edge cases:

- If `linkedPlanBranchId` is missing, accepted/converted session state can still expose Tasks.
- If the workspace points to a blank session, `linkedPlanBranchId` is usually missing, so Tasks disappears.
- If proposals were converted but `createdTaskId` is not present in the API response, Tasks relies on accepted/converted/session status or plan branch linkage.

### 7. Auto-Open And Pane State

`hasAutoOpenArtifacts` opens the pane by default when an attached session has meaningful artifact state:

- plan artifact,
- inherited plan artifact,
- pending acceptance,
- execution tasks,
- verification in progress,
- verification status other than `unverified`.

The pane state lives in two places:

- persisted Zustand store: `agentSessionStore.artifactByConversationId`,
- optimistic in-memory UI store: `agentArtifactUiStore`.

Resolution order:

1. optimistic state wins,
2. persisted state wins,
3. default state opens if `hasAutoOpenArtifacts` is true.

Edge cases:

- Optimistic active tab is intentionally preserved because it represents a current-session user click.
- Persisted stale external artifact tabs (`linear`, `jira`, `publish`) are sanitized to Tasks/Plan when ideation tabs are available. `linear` and `jira` are legacy artifact states after the linked-issues overlay split.
- Snapshot reads used outside the hook can miss `availableArtifactTabs` unless explicitly passed, so repair effects must be checked carefully.

### 8. Artifact Pane Tab Assembly

`AgentsArtifactPane.tsx` assembles visible tabs in this order:

1. Plan / Verification / Proposals / Tasks
2. Commit & Publish

`effectiveActiveTab` is:

- `activeTab`, if visible,
- otherwise the first visible tab,
- otherwise `plan`.

Edge cases:

- If ideation tabs are empty and Commit & Publish is visible, `effectiveActiveTab` can become `publish`.
- If a legacy persisted `linear` or `jira` artifact tab is present, it is not visible and falls back to the preferred ideation tab when ideation tabs exist.
- Jira/Linear settings can load independently, but they only affect the linked-issues overlay and cannot replace artifact pane content.

### 9. Plan Tab Content

The Plan tab renders `AgentPlanPanel`.

Plan content is loaded from:

- `artifactApi.getSessionPlan(attachedSessionId)` for planning session flow, or
- `artifactApi.get(planArtifactId)` otherwise.

Plan tab also shows plan actions and verification state.

Edge cases:

- Plan artifact id can exist while artifact content is still fetching; the pane shows a loading/empty state.
- Session flow changes which backend query is used.
- Plan update writes are pushed into both artifact and session-plan query caches.

### 10. Verification Tab Content

The Verification tab renders `VerificationPanel` for the attached session.

Status is composed from:

- displayed child status from the panel,
- `useVerificationStatus(attachedSessionId)`,
- session `verificationStatus`,
- default `unverified`.

Verification query is only enabled when the active tab is Verification.

Edge cases:

- Verification tab can be visible while verification status is still `unverified`.
- The visible state can be overridden by child panel display callbacks.
- Switching active tab controls whether the verification query runs.

### 11. Proposals Tab Content

The Proposals tab renders proposals from `getWithData`.

It also loads dependency graph data when active tab is Proposals or Tasks:

```ts
useDependencyGraph(attachedSessionId)
```

Edge cases:

- Proposals tab can be visible with zero proposals if a plan exists; the content then shows “No proposals yet.”
- Dependency graph is scoped by attached session id, not execution plan id.
- Proposal-created task links affect Tasks tab visibility.

### 12. Tasks Tab Content

Tasks tab renders either:

- embedded TaskBoard, or
- embedded TaskGraphView.

The task surface receives:

- `projectId`
- `ideationSessionId`
- optionally `executionPlanId` resolved from `linkedPlanBranchId`.

When an execution plan id is available, task views use that stronger execution-plan scope and suppress session scope.

Edge cases:

- Without `executionPlanId`, the embedded task view can fall back to ideation-session filtering.
- If the parent workspace has a linked plan branch but frontend cannot resolve its execution plan id, tasks may still appear by session but can be incomplete.
- If the linked session is stale/blank, the task board can show wrong or empty tasks.

### 13. Workspace Link Sync

When a project conversation resolves a productive attached ideation session, the frontend calls:

```ts
syncAgentConversationWorkspaceIdeationLink(conversationId, attachedSessionId)
```

Backend behavior:

- validates project conversation and plan/ideation workspace,
- verifies session belongs to project,
- finds plan branch by session id,
- updates `linked_ideation_session_id` and `linked_plan_branch_id`.

Guard:

- If the workspace already has a plan branch and the requested session has no plan branch, sync is ignored to avoid downgrading a productive plan link to a blank continuation session.

Edge cases:

- The guard only helps once the workspace has a productive plan branch.
- If the workspace is already poisoned with a blank session and no branch, backend cannot infer the productive old session unless transcript or another durable relation exposes it.
- Frontend sync can race from both `useAgentsAttachedIdeation` and `AgentsArtifactPane`.

## Overlap With Linked Issues And Publish

Artifact tabs share the artifact pane and active-tab state. Jira/Linear no longer share that state.

Issue tabs differ from ideation tabs:

- Linear/Jira are settings-gated and rendered in the linked-issues overlay.
- Linear/Jira can appear without an ideation session, but only through the Ticket button.
- Publish is workspace/PR-gated.
- Ideation tabs are session/plan-gated.

The primary legacy overlap bug class is stale external active-tab state:

- A persisted artifact `linear` / `jira` value can exist from older builds.
- Ideation tabs can be temporarily empty because attached session data is still loading or points to a stale session.
- If stale external active tab handling regresses, the right pane can look like ideation artifacts are missing.

Expected UX:

- When ideation tabs are available, stale persisted Linear/Jira/Publish should not hide Plan/Tasks.
- Explicit user clicks on Linear/Jira should open the separate linked-issues overlay.
- Explicit user clicks on Commit & Publish should still be honored in the artifact pane.

## Edge Case Matrix

| Area | Edge Case | Expected Behavior | Risk |
|---|---|---|---|
| Workspace | No linked ids and no recent transcript candidates | No ideation tabs | Running backend tasks may not surface in parent conversation |
| Workspace | Linked session is blank continuation | No plan/tabs unless inherited plan exists | Artifact pane can fall back to Publish or empty state |
| Workspace | Linked branch exists but linked session stale | Backend sync should avoid branch downgrade | UI may still fetch stale session if branch cannot identify session |
| History | Productive tool output older than 40 messages | Resolver misses it | Falls back to stale workspace link |
| Session | `getWithData` returns stale session data | UI ignores it by id check | Temporary empty tabs while loading |
| Plan | Session has no plan artifact | No ideation tabs | Proposals/tasks cannot surface through artifact pane |
| Verification | Plan exists but verification unverified | Verification tab still visible | Content can show empty/unverified state |
| Tasks | Plan accepted but no branch linked | Tasks visible via accepted/converted flags | Task scope may depend on session id |
| Tasks | Branch linked but execution id unresolved | Task view falls back to session scope | Could miss execution-plan-only tasks |
| Tab State | Persisted Linear active | Sanitized to Tasks/Plan when tabs are available | Legacy persisted state can still appear in stores |
| Issue Overlay | User opens Linear | Linked-issues overlay opens over the right edge | Artifact pane remains on ideation/publish content |
| Settings | Linear loads before ideation | Ticket button can appear first | Does not change artifact pane content |
| Direct Ideation | context type is ideation | session id is context id | Linear/Jira hidden |

## Debug Checklist

For a top-level conversation that should show Plan/Verification/Tasks but does not:

1. Check workspace row:
   - `linked_ideation_session_id`
   - `linked_plan_branch_id`
   - `mode`
2. Check linked session:
   - `plan_artifact_id`
   - `inherited_plan_artifact_id`
   - `status`
   - `acceptance_status`
   - `converted_at`
3. Check plan branch:
   - branch id matches workspace,
   - branch session id matches productive session,
   - branch has execution plan id.
4. Check recent transcript:
   - productive session id appears in last 40 messages,
   - tool result includes plan/proposal/task signals.
5. Check frontend state:
   - `availableArtifactTabs` is non-empty,
   - `hasAutoOpenArtifacts` is true,
   - active tab is not optimistic `linear`.
6. Check linked issues:
   - Jira/Linear availability may show the Ticket button, but it should not affect artifact-pane tab selection.
   - If the visible surface is Linear, confirm whether the linked-issues overlay is open rather than the artifact pane.
