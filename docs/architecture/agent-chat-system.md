# Agent Chat System Architecture

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.
>
> **Scope note:** The chat runtime is now provider-neutral at the app boundary (`AppChatService`) and can resolve Claude or Codex per lane. This document still uses Claude-flavored CLI examples where the lower-level transport is Claude-specific; treat those as implementation examples, not the universal runtime contract.

---

## 1. System Overview

```
User message
  → React UI (ChatPanel / IntegratedChatPanel)
    → Tauri IPC: send_agent_message { contextType, contextId, content }
      → ChatService.send_message()
        → resolve_agent(context_type, entity_status) → agent name
        → build_command(cli_path, plugin_dir, conversation, ...) → SpawnableCommand
          → selected harness CLI (`claude ...` | `codex ...`)
            → MCP tools via HTTP :3847 → ralphx-mcp-server → Tauri backend
        → stream_response() → parse JSON-stream events
          → Tauri app_handle.emit("agent:*", payload)
            → EventBus (browser/Tauri abstraction)
              → useAgentEvents / useIntegratedChatEvents / useChatPanelHandlers
                → React Query cache updates → UI re-render
```

### Key Subsystems

| Subsystem | Role | Key File(s) |
|-----------|------|-------------|
| ChatService | Orchestrates send/queue/stop/resume | `src-tauri/src/application/chat_service/mod.rs` |
| Agent Resolution | Maps context_type + status → agent name | `chat_service_helpers.rs` → `resolve_agent()` |
| Command Builder | Builds harness-specific CLI invocation and resume settings | `chat_service_context.rs` → `build_command()` |
| Streaming Parser | Parses JSON-stream output, emits events | `chat_service_streaming.rs` |
| MCP Server | Tool proxy: agent → HTTP :3847 → Tauri backend | `plugins/app/ralphx-mcp-server/src/` |
| Event System | Tauri emit → EventBus → React hooks | `src/lib/events.ts`, `src/hooks/useAgentEvents.ts` |
| Message Queue | Durable pending rows + live drain buffer while agent is running | `chat_service_queue.rs`, `QueuedMessageRepository`, `MessageQueue` service |
| Session Recovery | Retries on stale session detection | `chat_service_recovery.rs` |

### Data Flow: New Conversation

1. `send_agent_message` → `ChatService.send_message()`
2. Find or create `ChatConversation` (keyed by context_type + context_id)
3. Create `ChatMessage` (user role) + persist
4. Resolve agent name via `resolve_agent(context_type, entity_status)`
5. Build `SpawnableCommand` with `--agent ralphx:<name>` + env vars
6. Spawn the selected harness process in a background task
7. Emit `agent:run_started` → return `SendResult { conversation_id, agent_run_id }`
8. Stream stdout → parse events → emit `agent:chunk`, `agent:tool_call`, etc.
9. On completion → persist assistant message → emit `agent:run_completed`
10. If durable queued messages exist → auto-send via harness continuation (`--resume`-style path)

### Data Flow: Resumed Conversation

Steps 1-3 same, but step 5 uses the harness-native continuation path when provider session lineage is preserved. Legacy `claude_session_id` may still appear as a compatibility alias, but canonical provider metadata is `provider_harness` + `provider_session_id`. Fresh review cycles (`reviewer` agent) always start new sessions to avoid stale context.

---

## 2. Complete Agent Inventory (19 live agents)

| # | Agent Name (frontmatter) | Short Const | Category | Model | Trigger | Session Lifecycle |
|---|--------------------------|-------------|----------|-------|---------|-------------------|
| 1 | `ralphx-ideation` | `SHORT_ORCHESTRATOR_IDEATION` | ideation | sonnet | User sends ideation message | per-session |
| 2 | `ralphx-ideation-readonly` | `SHORT_ORCHESTRATOR_IDEATION_READONLY` | ideation | sonnet | Read-only mode (session status = "accepted") | per-session |
| 3 | `ralphx-utility-session-namer` | `SHORT_SESSION_NAMER` | ideation | haiku | First message in ideation session | fire-and-forget |
| 4 | `ralphx-chat-task` | `SHORT_CHAT_TASK` | task | default | User chats in task detail view | per-task |
| 5 | `ralphx-chat-project` | `SHORT_CHAT_PROJECT` | project | default | User chats in project view | per-project |
| 6 | `ralphx-execution-worker` | `SHORT_WORKER` | execution | default | Task enters "executing" / "re_executing"; decomposes graph + delegates parallel coder waves (max 3) | per-task-execution |
| 7 | `ralphx-execution-coder` | `SHORT_CODER` | execution | default | Delegated coding from `ralphx-execution-worker` | per-subtask |
| 8 | `ralphx-execution-reviewer` | `SHORT_REVIEWER` | review | default | Task enters "reviewing" | per-review-cycle (fresh session each cycle) |
| 9 | `ralphx-review-chat` | `SHORT_REVIEW_CHAT` | review | default | Task in "review_passed" (human decision) | per-review |
| 10 | `ralphx-review-history` | `SHORT_REVIEW_HISTORY` | review | default | Task "approved" (read-only history) | per-review |
| 11 | `ralphx-execution-merger` | `SHORT_MERGER` | merge | default | Task enters "merging" | per-merge |
| 12 | `ralphx-execution-orchestrator` | `SHORT_ORCHESTRATOR` | orchestration | opus | Multi-task coordination | -- |
| 14 | `ralphx-qa-prep` | `SHORT_QA_PREP` | qa | sonnet | Task enters "ready" | fire-and-forget |
| 15 | `ralphx-qa-executor` | `SHORT_QA_EXECUTOR` | qa | sonnet | QA execution phase | -- |
| 16 | `ralphx-research-deep-researcher` | `SHORT_DEEP_RESEARCHER` | research | opus | Research tasks | -- |
| 17 | `ralphx-project-analyzer` | `SHORT_PROJECT_ANALYZER` | analysis | haiku | Project analysis (build/validation detection) | fire-and-forget |
| 18 | `ralphx-memory-capture` | `SHORT_MEMORY_CAPTURE` | memory | haiku | Post-session knowledge extraction | fire-and-forget |
| 19 | `ralphx-memory-maintainer` | `SHORT_MEMORY_MAINTAINER` | memory | default | Memory ingestion/dedup/index maintenance | -- |

### Agent MCP Tool Summary

| Agent | MCP Tools (from canonical `mcp_tools`) |
|-------|-------------------------------|
| ralphx-ideation | create/update/delete_task_proposal, list_session_proposals, get_proposal, analyze_session_dependencies, create/update_plan_artifact, link_proposals_to_plan, get_session_plan, ask_user_question, create_child_session, get_parent_session_context, search/get/get_for_paths memories |
| ralphx-ideation-readonly | list_session_proposals, get_proposal, get_session_plan, get_parent_session_context, create_child_session, search/get/get_for_paths memories |
| ralphx-utility-session-namer | update_session_title |
| ralphx-chat-task | update_task, add_task_note, get_task_details, search/get/get_for_paths memories |
| ralphx-chat-project | suggest_task, list_tasks, search/get/get_for_paths memories, get_conversation_transcript |
| ralphx-execution-worker | start/complete/skip/fail/add_step, get_step_progress, get_task_issues, mark_issue_in_progress, mark_issue_addressed, get_project_analysis, get_task_context, get/get_version/get_related/search_project artifacts, get_review_notes, get_task_steps, search/get/get_for_paths memories |
| ralphx-execution-coder | start/complete/skip/fail/add_step, get_step_progress, get_task_issues, mark_issue_in_progress, mark_issue_addressed, get_project_analysis, get_task_context, get/get_version/get_related/search_project artifacts, get_review_notes, get_task_steps, search/get/get_for_paths memories |
| ralphx-execution-reviewer | complete_review, get_task_issues, get_step_progress, get_issue_progress, get_project_analysis, get_task_context, get/get_version/get_related/search_project artifacts, get_review_notes, get_task_steps, search/get/get_for_paths memories |
| ralphx-review-chat | approve_task, request_task_changes, get_review_notes, get_task_context, get/get_version/get_related/search_project artifacts, get_task_steps, search/get/get_for_paths memories |
| ralphx-review-history | get_review_notes, get_task_context, get_task_issues, get_task_steps, get_step_progress, get_issue_progress, get/get_version/get_related/search_project artifacts, search/get/get_for_paths memories |
| ralphx-execution-merger | complete_merge, report_conflict, report_incomplete, get_merge_target, get_project_analysis, get_task_context, search/get/get_for_paths memories |
| ralphx-execution-orchestrator | search/get/get_for_paths memories |
| ralphx-qa-prep | fs_read_file, fs_list_dir, fs_grep, fs_glob |
| ralphx-qa-executor | (no MCP tools) |
| ralphx-research-deep-researcher | search/get/get_for_paths memories |
| ralphx-project-analyzer | save_project_analysis, get_project_analysis |
| ralphx-memory-capture | upsert_memories, search/get/get_for_paths memories, get_conversation_transcript |
| ralphx-memory-maintainer | upsert_memories, mark_memory_obsolete, refresh_memory_rule_index, ingest_rule_file, rebuild_archive_snapshots, search/get/get_for_paths memories, get_conversation_transcript |

---

## 3. ChatContextType Reference

6 context types spanning Rust backend, TypeScript frontend, and store keying.

| ContextType | Rust Enum Variant | TS String Literal | Default Agent | Store Key Pattern | CWD Resolution |
|-------------|-------------------|-------------------|---------------|-------------------|----------------|
| ideation | `ChatContextType::Ideation` | `"ideation"` | ralphx-ideation | `session:{id}` | project.working_directory |
| task | `ChatContextType::Task` | `"task"` | ralphx-chat-task | `task:{id}` | project.working_directory (Local) or task.worktree_path (Worktree) |
| project | `ChatContextType::Project` | `"project"` | ralphx-chat-project | `project:{id}` | project.working_directory |
| task_execution | `ChatContextType::TaskExecution` | `"task_execution"` | ralphx-execution-worker | `task_execution:{id}` | project.working_directory (Local) or task.worktree_path (Worktree) |
| review | `ChatContextType::Review` | `"review"` | ralphx-execution-reviewer | `review:{id}` | project.working_directory (Local) or task.worktree_path (Worktree) |
| merge | `ChatContextType::Merge` | `"merge"` | ralphx-execution-merger | `merge:{id}` | project.working_directory (Local) or merge worktree (Worktree, never task worktree) |

### CWD Resolution Rules (Worktree Mode)

| Context | Worktree Mode CWD |
|---------|-------------------|
| Task, TaskExecution, Review | task.worktree_path if exists, else project.working_directory |
| Merge | merge worktree (`merge-<task_id>`) if exists, else project.working_directory. **Never** uses task worktree. |
| Ideation, Project | project.working_directory (always) |

### Context → Message Role Mapping

| Context Type | Assistant Role |
|-------------|---------------|
| Ideation | `Orchestrator` |
| Task | `Orchestrator` |
| Project | `Orchestrator` |
| TaskExecution | `Worker` |
| Review | `Reviewer` |
| Merge | `Merger` |

---

## 4. Event System Reference

All `agent:*` events emitted by the Rust backend via `app_handle.emit()`.

### Event Constants

Defined in both backend (`chat_service_types.rs::events`) and frontend (`src/lib/events.ts`).

| Event Name | Rust Constant | TS Constant | Payload Struct (Rust) |
|------------|---------------|-------------|----------------------|
| `agent:run_started` | `events::AGENT_RUN_STARTED` | `AGENT_RUN_STARTED` | `AgentRunStartedPayload` |
| `agent:message_created` | `events::AGENT_MESSAGE_CREATED` | `AGENT_MESSAGE_CREATED` | `AgentMessageCreatedPayload` |
| `agent:run_completed` | `events::AGENT_RUN_COMPLETED` | `AGENT_RUN_COMPLETED` | `AgentRunCompletedPayload` |
| `agent:chunk` | `events::AGENT_CHUNK` | `AGENT_CHUNK` | `AgentChunkPayload` |
| `agent:tool_call` | `events::AGENT_TOOL_CALL` | `AGENT_TOOL_CALL` | `AgentToolCallPayload` |
| `agent:error` | `events::AGENT_ERROR` | `AGENT_ERROR` | `AgentErrorPayload` |
| `agent:message_queued` | `events::AGENT_MESSAGE_QUEUED` | -- | `AgentMessageQueuedPayload` |
| `agent:queue_sent` | `events::AGENT_QUEUE_SENT` | `AGENT_QUEUE_SENT` | `AgentQueueSentPayload` |
| `agent:task_started` | `events::AGENT_TASK_STARTED` | `AGENT_TASK_STARTED` | `AgentTaskStartedPayload` |
| `agent:task_completed` | `events::AGENT_TASK_COMPLETED` | `AGENT_TASK_COMPLETED` | `AgentTaskCompletedPayload` |
| `agent:hook` | `events::AGENT_HOOK` | `AGENT_HOOK` | `AgentHookPayload` |
| `agent:stopped` | (inline string) | -- | `{ context_type, context_id, conversation_id, agent_run_id }` |
| `agent:session_recovered` | (inline string) | `AGENT_SESSION_RECOVERED` | `{ conversation_id, message }` |
| `agent:message` | `events::AGENT_MESSAGE` | `AGENT_MESSAGE` | (activity stream message) |

### Event Payload Fields

| Event | Key Payload Fields |
|-------|-------------------|
| `agent:run_started` | run_id, conversation_id, context_type, context_id, run_chain_id?, parent_run_id?, provider_harness?, provider_session_id? |
| `agent:message_created` | message_id, conversation_id, context_type, context_id, role, content |
| `agent:run_completed` | conversation_id, context_type, context_id, provider_harness?, provider_session_id?, claude_session_id? (legacy alias), run_chain_id? |
| `agent:chunk` | text, conversation_id, context_type, context_id |
| `agent:tool_call` | tool_name, tool_id?, arguments, result?, conversation_id, context_type, context_id, diff_context?, parent_tool_use_id? |
| `agent:error` | conversation_id?, context_type, context_id, error, stderr? |
| `agent:message_queued` | message_id, content, conversation_id?, context_type, context_id, created_at, attachment_ids? |
| `agent:queue_sent` | message_id, conversation_id, context_type, context_id |
| `agent:task_started` | tool_use_id, description?, subagent_type?, model?, conversation_id, context_type, context_id |
| `agent:task_completed` | tool_use_id, agent_id?, total_duration_ms?, total_tokens?, total_tool_use_count?, conversation_id, context_type, context_id |
| `agent:hook` | type ("started"\|"completed"\|"block"), hook_name?, hook_event?, hook_id?, output?, outcome?, exit_code?, reason?, conversation_id, context_type, context_id, timestamp |
| `agent:stopped` | context_type, context_id, conversation_id, agent_run_id |
| `agent:session_recovered` | conversation_id, message |

### Frontend Event Consumers

| Event | Consumer Hooks |
|-------|---------------|
| `agent:run_started` | useAgentEvents (set running state, invalidate conversation list, set active conversation) |
| `agent:message_created` | useAgentEvents (optimistic user message append, invalidate for assistant messages) |
| `agent:run_completed` | useAgentEvents (clear running state, invalidate queries), useIntegratedChatEvents (clear streaming state) |
| `agent:chunk` | useIntegratedChatEvents (accumulate streaming text) |
| `agent:tool_call` | useIntegratedChatEvents (accumulate streaming tool calls), useChatPanelHandlers (tool call display) |
| `agent:error` | useAgentEvents (clear running state, invalidate queries), useChatPanelHandlers (error display) |
| `agent:message_queued` | useAgentEvents (optimistic queue insert/update) |
| `agent:queue_sent` | useAgentEvents (remove from frontend optimistic queue) |
| `agent:task_started` | useIntegratedChatEvents (add to streaming tasks map) |
| `agent:task_completed` | useIntegratedChatEvents (update streaming tasks map) |
| `agent:hook` | ChatPanel via ChatMessages (hook status display) |
| `agent:stopped` | useAgentEvents (defensive running state cleanup) |
| `agent:session_recovered` | useAgentEvents (info toast notification) |

---

## 5. MCP Tool Scoping (3-Layer Enforcement)

Each agent's tool access is restricted at three independent layers. All three must agree for a tool to be available.

| Layer | Location | Mechanism | Granularity |
|-------|----------|-----------|-------------|
| 1. Canonical prompt/config | `agents/<agent>/...` + generated Claude frontmatter | `tools:` / `disallowedTools:` plus harness prompt body | Per-agent |
| 2. MCP Server Filter | `ralphx-mcp-server/src/tools.ts` + `canonical-agent-metadata.ts` | canonical `agents/<agent>/agent.yaml` `capabilities.mcp_tools` (with env/CLI override precedence and legacy fallback only) → filters `listTools` response | Per-agent-type at runtime |
| 3. Agent System Prompt | Agent `.md` body instructions | Natural language guidance on which tools to use | Behavioral (soft) |

### Enforcement Flow

```
Claude CLI spawns agent → sets RALPHX_AGENT_TYPE env var
  → MCP server starts → reads RALPHX_AGENT_TYPE
    → listTools request → getAllowedToolNames()
      → env/CLI override | canonical `mcp_tools` | legacy fallback
      → filter ALL_TOOLS by resolved tool names
        → Only matching tools returned to Claude
```

Layer 1 (YAML) controls which built-in Claude tools the agent can use (Read, Write, Edit, Bash, etc.).
Layer 2 (MCP) controls which RalphX-specific MCP tools are exposed.
Layer 3 (Prompt) provides behavioral guidance to reinforce tool boundaries.

### Env Var Propagation

| Env Var | Set By | Purpose |
|---------|--------|---------|
| `RALPHX_AGENT_TYPE` | `build_command()` via `mcp_agent_type()` | MCP tool filtering |
| `RALPHX_TASK_ID` | `build_command()` for Task/TaskExecution/Review/Merge | Task context for MCP handlers |
| `RALPHX_PROJECT_ID` | `build_command()` when project_id resolved | Project context for MCP handlers |

---

## 6. Agent Resolution Rules

`resolve_agent(context_type, entity_status)` in `chat_service_helpers.rs`.

### Status-Specific Overrides (Checked First)

| Context Type | Entity Status | Resolved Agent | Why |
|-------------|---------------|----------------|-----|
| Ideation | `"accepted"` | `ralphx-ideation-readonly` | No mutation tools for accepted plans |
| Review | `"review_passed"` | `ralphx-review-chat` | Human discusses findings, can approve/reject |
| Review | `"approved"` | `ralphx-review-history` | Read-only retrospective discussion |

### Default Resolution (No Status Match)

| Context Type | Default Agent | FQ Name |
|-------------|---------------|---------|
| Ideation | ralphx-ideation | `ralphx:ralphx-ideation` |
| Task | ralphx-chat-task | `ralphx:ralphx-chat-task` |
| Project | ralphx-chat-project | `ralphx:ralphx-chat-project` |
| TaskExecution | ralphx-execution-worker | `ralphx:ralphx-execution-worker` |
| Review | ralphx-execution-reviewer | `ralphx:ralphx-execution-reviewer` |
| Merge | ralphx-execution-merger | `ralphx:ralphx-execution-merger` |

### Delegated Execution Agent

`ralphx-execution-coder` is not selected directly by `resolve_agent()`. It is invoked by `ralphx-execution-worker` for delegated coding scopes during task execution.
`ralphx-execution-worker` builds a dependency graph from task scope, schedules parallel waves, and dispatches up to 3 concurrent coder instances with non-overlapping file ownership.

### Session Resumption Rules

| Condition | Resume? | Reason |
|-----------|---------|--------|
| Has `claude_session_id` + not reviewer + not TaskExecution | Yes (`--resume`) | Conversation continuity |
| Reviewer agent (fresh review cycle) | No (new session) | Avoids stale "review already submitted" context |
| TaskExecution context | No (new session) | Each execution is independent |
| First message (no session ID) | No | Nothing to resume |

### Spawner Agent Mapping

State machine side effects use short names → `spawner_agent_name()` maps to FQ names:

| Spawner Key | FQ Agent Name |
|-------------|---------------|
| `"qa-prep"` | `ralphx:ralphx-qa-prep` |
| `"qa-refiner"` | `ralphx:qa-refiner` |
| `"qa-tester"` | `ralphx:qa-tester` |
| `"worker"` / `"ralphx-execution-worker"` | `ralphx:ralphx-execution-worker` |
| `"reviewer"` / `"ralphx-execution-reviewer"` | `ralphx:ralphx-execution-reviewer` |
| `"merger"` / `"ralphx-execution-merger"` | `ralphx:ralphx-execution-merger` |

---

## 7. Key Files Index

### Backend (Rust)

| File | Purpose |
|------|---------|
| `src-tauri/src/application/chat_service/mod.rs` | ChatService trait + ClaudeChatService implementation |
| `src-tauri/src/application/chat_service/chat_service_types.rs` | Event constants, payload structs, SendResult, errors |
| `src-tauri/src/application/chat_service/chat_service_context.rs` | Agent resolution, CWD resolution, command builder, prompt builder |
| `src-tauri/src/application/chat_service/chat_service_helpers.rs` | `resolve_agent()`, `get_assistant_role()` |
| `src-tauri/src/application/chat_service/chat_service_streaming.rs` | JSON-stream parser, event emission during streaming |
| `src-tauri/src/application/chat_service/chat_service_send_background.rs` | Background task: spawn agent, process stream, handle completion |
| `src-tauri/src/application/chat_service/chat_service_handlers.rs` | Post-stream success/error handling, task transitions, session recovery |
| `src-tauri/src/application/chat_service/chat_service_queue.rs` | Message queue processing (auto-send via --resume) |
| `src-tauri/src/domain/repositories/queued_message_repository.rs` | Durable queued-message repository contract |
| `src-tauri/src/infrastructure/sqlite/sqlite_queued_message_repo.rs` | SQLite queued-message storage |
| `src-tauri/src/infrastructure/memory/memory_queued_message_repo.rs` | In-memory queued-message repository for tests/non-SQLite wiring |
| `src-tauri/src/application/chat_service/chat_service_recovery.rs` | Stale session detection and retry logic |
| `src-tauri/src/application/chat_service/chat_service_replay.rs` | Conversation replay for debugging |
| `src-tauri/src/application/chat_service/chat_service_repository.rs` | Conversation/message persistence helpers |
| `src-tauri/src/application/chat_service/chat_service_merge.rs` | Merge-specific completion logic |
| `src-tauri/src/application/chat_service/chat_service_errors.rs` | Error classification (stale session, auth, etc.) |
| `src-tauri/src/application/chat_service/chat_service_mock.rs` | Mock implementation for testing |
| `src-tauri/src/commands/unified_chat_commands.rs` | Tauri IPC command handlers (thin wrappers) |
| `src-tauri/src/domain/entities/chat_conversation.rs` | ChatConversation entity, ChatContextType enum, ChatConversationId newtype |
| `src-tauri/src/infrastructure/agents/claude/agent_names.rs` | Agent name constants (single source of truth for FQ names) |
| `src-tauri/src/application/chat_resumption.rs` | Chat resumption service |

### Frontend (TypeScript/React)

| File | Purpose |
|------|---------|
| `frontend/src/lib/events.ts` | Event name constants (mirrors backend) |
| `frontend/src/hooks/useAgentEvents.ts` | Agent lifecycle event listener (run state, messages, queue, errors) |
| `frontend/src/hooks/useQueuedMessagesHydration.ts` | Hydrates backend-owned queued rows after panel mount/navigation |
| `frontend/src/hooks/useIntegratedChatEvents.ts` | Streaming event handler (chunks, tool calls, tasks) |
| `frontend/src/hooks/useChatPanelHandlers.ts` | ChatPanel event handlers and queue management |
| `frontend/src/hooks/useChat.ts` | TanStack Query hooks for chat API calls |
| `frontend/src/api/chat.ts` | Tauri invoke wrappers for chat commands |
| `frontend/src/stores/chatStore.ts` | Zustand store for chat UI state (running, queue, active conversation) |
| `frontend/src/components/Chat/ChatPanel.tsx` | Main chat panel component |
| `frontend/src/components/Chat/IntegratedChatPanel.tsx` | Integrated panel with streaming tool call display |
| `frontend/src/providers/EventProvider.tsx` | EventBus provider (browser/Tauri abstraction) |
| `frontend/src/types/chat-conversation.ts` | ContextType type, conversation schemas |

### MCP Server

| File | Purpose |
|------|---------|
| `plugins/app/ralphx-mcp-server/src/tools.ts` | ALL_TOOLS composition + MCP authorization facade |
| `plugins/app/ralphx-mcp-server/src/tool-authorization.ts` | Canonical-vs-legacy MCP authorization resolution |
| `plugins/app/ralphx-mcp-server/src/agentNames.ts` | Agent name constants (TS mirror of Rust agent_names.rs) |
| `plugins/app/ralphx-mcp-server/src/index.ts` | MCP server entry point, CLI arg parsing |

### Agent Definitions

| File | Agent |
|------|-------|
| `agents/ralphx-ideation/` | Ideation orchestrator |
| `agents/ralphx-ideation-readonly/` | Read-only ideation |
| `agents/ralphx-utility-session-namer/` | Session title generator |
| `agents/ralphx-chat-task/` | Task-scoped chat |
| `agents/ralphx-chat-project/` | Project-scoped chat |
| `agents/ralphx-execution-worker/` | Task execution worker |
| `agents/ralphx-execution-coder/` | Delegated coding worker |
| `agents/ralphx-execution-reviewer/` | Code reviewer |
| `agents/ralphx-review-chat/` | Post-review human discussion |
| `agents/ralphx-review-history/` | Read-only review history |
| `agents/ralphx-execution-merger/` | Merge conflict resolver |
| `agents/ralphx-execution-orchestrator/` | Multi-task orchestrator |
| `agents/ralphx-qa-prep/` | QA preparation |
| `agents/ralphx-qa-executor/` | QA execution |
| `agents/ralphx-research-deep-researcher/` | Research agent |
| `agents/ralphx-project-analyzer/` | Build/validation detector |
| `agents/ralphx-memory-capture/` | Post-session knowledge extraction |
| `agents/ralphx-memory-maintainer/` | Memory ingestion/maintenance |

---

## 8. Tauri Commands Reference

All chat-related Tauri IPC commands registered in `unified_chat_commands.rs`.

| Command | Input | Returns | Description |
|---------|-------|---------|-------------|
| `send_agent_message` | `{ contextType, contextId, content }` | `{ conversation_id, agent_run_id, is_new_conversation }` | Send message, returns immediately, processing in background |
| `queue_agent_message` | `{ contextType, contextId, content, clientId? }` | `{ id, content, created_at, is_editing }` | Persist queued message for when current run completes |
| `get_queued_agent_messages` | `contextType, contextId` | `QueuedMessageResponse[]` | List queued messages for context |
| `delete_queued_agent_message` | `contextType, contextId, messageId` | `bool` | Delete a queued message |
| `list_agent_conversations` | `contextType, contextId` | `AgentConversationResponse[]` | List all conversations for context |
| `get_agent_conversation` | `conversationId` | `AgentConversationWithMessagesResponse?` | Get conversation with all messages |
| `get_agent_run_status_unified` | `conversationId` | `AgentRunStatusResponse?` | Get active agent run status |
| `is_chat_service_available` | -- | `bool` | Check if Claude CLI is available |
| `stop_agent` | `contextType, contextId` | `bool` | SIGTERM running agent, emits agent:stopped |
| `is_agent_running` | `contextType, contextId` | `bool` | Check if agent is running for context |
| `create_agent_conversation` | `{ contextType, contextId }` | `AgentConversationResponse` | Create new conversation manually |

---

## 9. Error Handling

### ChatServiceError Variants

| Variant | Meaning |
|---------|---------|
| `AgentNotAvailable` | Claude CLI not found or not in PATH |
| `SpawnFailed` | Failed to spawn Claude CLI process |
| `CommunicationFailed` | IPC/stream communication error |
| `ParseError` | Failed to parse JSON-stream output |
| `ContextNotFound` | Context entity (task/session/project) not found |
| `ConversationNotFound` | Conversation ID not in database |
| `RepositoryError` | Database/persistence error |
| `AgentRunFailed` | Agent run completed with error status |

### Error Classification (StreamError)

Errors from agent stderr are classified for recovery decisions:

| Classification | Action |
|---------------|--------|
| Stale session | Auto-retry with new session (session recovery) |
| Auth error | Surface to user, no retry |
| Rate limit | Surface to user with backoff suggestion |
| Other | Surface to user, mark run as failed |
