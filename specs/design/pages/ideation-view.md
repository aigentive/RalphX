# Ideation in Agents

Ideation is a conversation-scoped artifact inside **Agents**. It is not a root
view and must not receive a navigation item, keyboard shortcut, `AppView`
value, or per-project route entry.

## Live ownership

- `AgentsView` owns the selected conversation and main chat.
- `AgentsArtifactPane` owns Plan, Ideation, Verification, Proposals, and Tasks.
- `useAgentsAttachedIdeation` hydrates the linked ideation session.
- The backend remains authoritative for sessions, proposals, plan artifacts,
  verification, finalization, and task creation.

## Interaction contract

1. Open Agents and select or start a planning conversation.
2. Use the Plan/Ideation artifact tabs beside the conversation.
3. Keep questions, progress, and approval actions in that same workspace.
4. After implementation begins, use the Tasks artifact's Kanban and Graph
   modes; selecting a task opens the Agents-owned detail overlay.

Cold or historical ideation links resolve to their exact Agent workspace.
Missing or deleted ownership leaves Agents visible, preserves the current valid
conversation, and shows non-blocking guidance.

Historical standalone Ideation layouts are retired and are not implementation
guidance.
