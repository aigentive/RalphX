# Artifact Pane Debugging: ClickUp Top-Level Conversation

This report applies the Linear lifecycle and ideation artifact lifecycle to the current ClickUp top-level conversation failure: the right pane showed an empty Linear artifact instead of Plan, Verification, Proposals, and Tasks.

## Observed Failure

Conversation:

- `agent-0e1ea951`
- conversation id: `0e1ea951-b4c1-423b-973a-a9186ac9cfc0`
- title: `ClickUp integration`

Observed UI:

- right pane opens on Linear,
- Linear has no assigned issue,
- Plan / Verification / Proposals / Tasks are not surfaced as the active content even though ClickUp task execution exists.

## Data Check

The dev DB currently has the correct productive linkage:

- `agent_conversation_workspaces.linked_ideation_session_id`
  - `ae4249ec-43c6-4123-8c55-9b5ddd446889`
- `agent_conversation_workspaces.linked_plan_branch_id`
  - `1b1fd88b-69f1-4f8e-a2ec-370936b23005`
- plan branch execution plan:
  - `95b4a47c-f54b-4e28-a4d8-789f2f0dd715`
- linked session plan artifact:
  - `1179ab38-2c62-48fd-9a4d-e308ffcddcc9`
- proposals:
  - 4 total
  - 4 with `created_task_id`
- tasks:
  - 6 tasks scoped to the linked ideation session / execution plan

Therefore the missing tabs are not caused by absent plan/proposal/task rows.

## Expected Frontend State

Given that DB state, `useAgentsAttachedIdeation` should compute:

- `attachedIdeationSessionId = ae4249ec-43c6-4123-8c55-9b5ddd446889`
- `hasAutoOpenArtifacts = true`
- `availableArtifactTabs = ["plan", "verification", "proposal", "tasks"]`

Then `AgentsArtifactPane` should assemble visible tabs as:

1. Plan
2. Verification
3. Proposals
4. Tasks
5. Publish, if workspace publishing is available

Linear/Jira should be available only through the separate linked-issues overlay opened by the Ticket button in the chat header.

The correct first active ideation tab for this state is Tasks, because execution tasks exist.

## Root Cause

The root cause is an active-tab lifecycle race between Linear and ideation artifacts in the old shared-pane model.

Timeline:

1. The artifact pane opens before ideation `getWithData` has finished.
2. During that loading gap, `availableArtifactTabs` can be empty.
3. Linear settings can already be loaded and valid, so Linear was a visible artifact-pane tab.
4. The active tab can become `linear` from either persisted state or optimistic in-memory state.
5. Once ideation data arrives, Plan/Verification/Proposals/Tasks become available.
6. Persisted stale external tabs were already sanitized, but optimistic `linear` was intentionally preserved to allow manual Linear clicks.
7. That preservation lets a loading-gap Linear selection continue masking the newly available ideation tabs.

The earlier stale-link bug was different:

- the workspace row could be overwritten to a blank continuation session,
- `linked_plan_branch_id` was cleared,
- then the UI truly had no productive session to load.

That data bug is fixed/guarded. The remaining failure is the UI active-tab race.

## Fix Applied

The robust fix is to split issue integrations out of the artifact pane:

- `AgentsArtifactPane.tsx` no longer renders Jira or Linear tabs.
- `AgentsIssuePaneRegion.tsx` owns Jira/Linear as a standalone right-edge overlay.
- `agentIssueTabs.ts` centralizes settings-gated Jira/Linear availability.
- `AgentsChatHeader.tsx` exposes a Ticket button to open/close linked issues.
- Legacy persisted artifact `linear` / `jira` active-tab values are still treated as stale external states and redirected to the preferred ideation tab.

When ideation tabs are available in the artifact pane:

- stale active `linear` / `jira` / `publish` is redirected to the preferred ideation tab,
- preferred ideation tab is `tasks`, then `plan`, then first available ideation tab,
- manual clicks on Linear/Jira now open the linked-issues overlay instead of changing artifact-pane content.

Regression added:

- `does not let Linear mask top-level workspace ideation artifacts`

This covers the exact top-level project conversation case where:

- Linear is configured,
- active tab starts as `linear`,
- workspace has linked ideation session and plan branch,
- session has accepted plan/tasks,
- expected rendered content is Tasks, not Linear.

## Remaining Edge Cases To Watch

- If `availableArtifactTabs` remains empty, the artifact pane will not show Plan/Verification/Proposals/Tasks. That means the next debugging target is session resolution/data fetch, not Linear.
- If the user explicitly opens Linear after ideation tabs are available, Linear should appear in the linked-issues overlay while the artifact pane remains available underneath.
- If the workspace row is again poisoned to a blank session with no plan branch, the UI cannot reliably recover unless recent transcript history still includes the productive session.
- If the productive session is older than the 40-message history window and the workspace link is wrong, frontend discovery may fail.
- If `get_ideation_session_with_data` fails or returns null for the productive session, tab availability stays empty.

## Debug Commands

Use these to separate data-link bugs from tab-state bugs:

```sql
select conversation_id, mode, linked_ideation_session_id, linked_plan_branch_id
from agent_conversation_workspaces
where conversation_id = '0e1ea951-b4c1-423b-973a-a9186ac9cfc0';

select id, status, acceptance_status, plan_artifact_id, inherited_plan_artifact_id, converted_at
from ideation_sessions
where id = 'ae4249ec-43c6-4123-8c55-9b5ddd446889';

select id, session_id, execution_plan_id, status
from plan_branches
where id = '1b1fd88b-69f1-4f8e-a2ec-370936b23005';

select count(*) as proposals,
       sum(case when created_task_id is not null then 1 else 0 end) as with_created_task
from task_proposals
where session_id = 'ae4249ec-43c6-4123-8c55-9b5ddd446889';

select count(*) as tasks
from tasks
where ideation_session_id = 'ae4249ec-43c6-4123-8c55-9b5ddd446889'
   or execution_plan_id = '95b4a47c-f54b-4e28-a4d8-789f2f0dd715';
```

Interpretation:

- If these rows are correct but the artifact pane lacks ideation tabs, debug frontend session resolution/data fetch.
- If these rows are correct and Linear is visible, confirm whether it is the linked-issues overlay rather than artifact pane content.
- If these rows are wrong, debug workspace/session sync.
- If rows are correct but `availableArtifactTabs` is empty, debug `getWithData` and frontend transform/schema.
