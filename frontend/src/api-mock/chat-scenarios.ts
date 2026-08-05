import {
  normalizeConversationProviderMetadata,
  type ChatConversation,
  type ContextType,
} from "@/types/chat-conversation";
import type {
  ChatMessageResponse,
  ChildSessionStatusResponse,
  QueuedMessageResponse,
} from "@/api/chat";

export type MockChatScenarioName =
  | "ideation_db_widget_mix"
  | "ideation_widget_matrix"
  | "execution_db_compact"
  | "review_db_compact"
  | "review_widget_matrix"
  | "merge_db_compact"
  | "merge_widget_matrix";

export type MockChatScenario = {
  conversations: ChatConversation[];
  messages: Record<string, ChatMessageResponse[]>;
  queuedMessages?: Record<string, QueuedMessageResponse[]>;
  childSessionStatuses?: Record<string, ChildSessionStatusResponse>;
};

function createConversation(
  conversation: Omit<
    ChatConversation,
    "claudeSessionId" | "providerSessionId" | "providerHarness" | "coordinationMode"
  > &
    Partial<Pick<ChatConversation, "coordinationMode">> &
    Parameters<typeof normalizeConversationProviderMetadata>[0]
): ChatConversation {
  return {
    id: conversation.id,
    contextType: conversation.contextType,
    contextId: conversation.contextId,
    ...normalizeConversationProviderMetadata({
      claudeSessionId: conversation.claudeSessionId,
      providerSessionId: conversation.providerSessionId,
      providerHarness: conversation.providerHarness,
    }),
    coordinationMode: conversation.coordinationMode ?? "solo",
    title: conversation.title,
    messageCount: conversation.messageCount,
    lastMessageAt: conversation.lastMessageAt,
    createdAt: conversation.createdAt,
    updatedAt: conversation.updatedAt,
  };
}

export function cloneMockChatMessage(
  message: ChatMessageResponse
): ChatMessageResponse {
  return {
    ...message,
    toolCalls: message.toolCalls ? [...message.toolCalls] : null,
    contentBlocks: message.contentBlocks ? [...message.contentBlocks] : null,
  };
}

export function listMockChatScenarios(): MockChatScenarioName[] {
  return Object.keys(chatScenarioFixtures) as MockChatScenarioName[];
}

export function getMockChatScenario(name: MockChatScenarioName): MockChatScenario {
  return chatScenarioFixtures[name];
}

export const IDEATION_REPLAY_CONTEXTS: Record<
  Extract<MockChatScenarioName, "ideation_db_widget_mix" | "ideation_widget_matrix">,
  {
    contextType: Extract<ContextType, "ideation">;
    contextId: string;
    conversationId: string;
  }
> = {
  ideation_db_widget_mix: {
    contextType: "ideation",
    contextId: "session-mock-1",
    conversationId: "conv-ideation-db-widget-mix",
  },
  ideation_widget_matrix: {
    contextType: "ideation",
    contextId: "session-widget-matrix",
    conversationId: "conv-ideation-widget-matrix",
  },
};

export const IDEATION_REPLAY_CONTEXT = IDEATION_REPLAY_CONTEXTS.ideation_db_widget_mix;

export const TASK_REPLAY_CONTEXTS: Record<
  Exclude<MockChatScenarioName, "ideation_db_widget_mix" | "ideation_widget_matrix">,
  {
    contextType: Extract<ContextType, "task_execution" | "review" | "merge">;
    contextId: string;
    conversationId: string;
  }
> = {
  execution_db_compact: {
    contextType: "task_execution",
    contextId: "task-mock-4",
    conversationId: "conv-execution-db-compact",
  },
  review_db_compact: {
    contextType: "review",
    contextId: "task-mock-5",
    conversationId: "conv-review-db-compact",
  },
  review_widget_matrix: {
    contextType: "review",
    contextId: "task-mock-5",
    conversationId: "conv-review-widget-matrix",
  },
  merge_db_compact: {
    contextType: "merge",
    contextId: "task-mock-merge-incomplete",
    conversationId: "conv-merge-db-compact",
  },
  merge_widget_matrix: {
    contextType: "merge",
    contextId: "task-mock-merge-incomplete",
    conversationId: "conv-merge-widget-matrix",
  },
};

const ideationReplayConversationId = IDEATION_REPLAY_CONTEXT.conversationId;
const ideationWidgetMatrixConversationId =
  IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.conversationId;
const ideationWidgetDelegatedConversationId = "conv-delegated-1";
const executionReplayConversationId =
  TASK_REPLAY_CONTEXTS.execution_db_compact.conversationId;
const reviewReplayConversationId =
  TASK_REPLAY_CONTEXTS.review_db_compact.conversationId;
const reviewWidgetMatrixConversationId =
  TASK_REPLAY_CONTEXTS.review_widget_matrix.conversationId;
const mergeReplayConversationId =
  TASK_REPLAY_CONTEXTS.merge_db_compact.conversationId;
const mergeWidgetMatrixConversationId =
  TASK_REPLAY_CONTEXTS.merge_widget_matrix.conversationId;

const chatScenarioFixtures: Record<MockChatScenarioName, MockChatScenario> = {
  ideation_db_widget_mix: {
    conversations: [
      createConversation({
        id: ideationReplayConversationId,
        contextType: "ideation",
        contextId: IDEATION_REPLAY_CONTEXT.contextId,
        claudeSessionId: "db-ideation-claude-session-1",
        title: "GitHub task-pipeline integration",
        messageCount: 6,
        lastMessageAt: "2026-03-11T21:58:43.308888Z",
        createdAt: "2026-03-11T21:51:34.604754Z",
        updatedAt: "2026-03-11T21:58:43.308888Z",
      }),
    ],
    messages: {
      [ideationReplayConversationId]: [
        {
          id: "msg-ideation-user-1",
          sessionId: "session-mock-1",
          projectId: "project-mock-1",
          taskId: null,
          role: "user",
          content:
            "we’ll be working on a github integration with the task pipeline. will need a new configurable option to create a PR when merge succeeds.",
          metadata: null,
          parentMessageId: null,
          conversationId: ideationReplayConversationId,
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: "2026-03-11T21:51:34.589194Z",
        },
        {
          id: "msg-ideation-orchestrator-1",
          sessionId: "session-mock-1",
          projectId: "project-mock-1",
          taskId: null,
          role: "assistant",
          content:
            "Let me start by checking the session state and pulling in specialists. The real DB sample behind this replay had a compact user prompt and a large orchestrator response dominated by native delegation, TaskCreate, and session-context MCP calls.",
          metadata: null,
          parentMessageId: "msg-ideation-user-1",
          conversationId: ideationReplayConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "text",
              text: "Fresh session. I’m gathering context and selecting native delegation lenses.",
            },
            {
              type: "tool_use",
              id: "tool-parent-context-1",
              name: "mcp__ralphx__get_parent_session_context",
              arguments: { session_id: "session-mock-1" },
              result: [
                {
                  type: "text",
                  text: "{\"ok\":true,\"title\":\"Demo Ideation Session\"}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "tool-task-create-1",
              name: "TaskCreate",
              arguments: {
                subject: "Support optional PR creation on merge",
                description: "Add a configurable merge-time GitHub PR path.",
              },
              result: { task_id: "task-github-pr" },
            },
            {
              type: "tool_use",
              id: "tool-agent-1",
              name: "Agent",
              arguments: {
                description: "Inspect merge pipeline hooks",
                subagent_type: "research",
              },
              result: [
                {
                  type: "tool_use",
                  id: "tool-agent-1-read-1",
                  name: "Read",
                  input: {
                    file_path: "src-tauri/src/application/chat_service/mod.rs",
                  },
                },
                {
                  type: "tool_result",
                  tool_use_id: "tool-agent-1-read-1",
                  content: "inspected merge pipeline",
                },
              ],
            },
          ],
          sender: "lead",
          attributionSource: "backfilled",
          providerHarness: "claude",
          providerSessionId: "db-ideation-claude-session-1",
          upstreamProvider: "anthropic",
          providerProfile: null,
          logicalModel: "claude-sonnet-4-6",
          effectiveModelId: "claude-sonnet-4-6",
          logicalEffort: "high",
          effectiveEffort: "high",
          inputTokens: 4821,
          outputTokens: 713,
          cacheCreationTokens: 0,
          cacheReadTokens: 3204,
          estimatedUsd: 0.08,
          createdAt: "2026-03-11T21:51:34.604754Z",
        },
        {
          id: "msg-ideation-orchestrator-2",
          sessionId: "session-mock-1",
          projectId: "project-mock-1",
          taskId: null,
          role: "assistant",
          content:
            "Good, first findings are in. The state machine can stay the same while the merge implementation picks up an optional GitHub PR action at the end.",
          metadata: null,
          parentMessageId: "msg-ideation-orchestrator-1",
          conversationId: ideationReplayConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "text",
              text: "I’m updating the task and pulling the current verification state.",
            },
            {
              type: "tool_use",
              id: "tool-task-update-1",
              name: "TaskUpdate",
              arguments: {
                taskId: "task-github-pr",
                status: "in_progress",
                owner: "codex",
                subject: "Replay chat UI from DB samples",
              },
              result: { success: true },
            },
            {
              type: "tool_use",
              id: "tool-plan-verification-1",
              name: "mcp__ralphx__get_plan_verification",
              arguments: { session_id: "session-mock-1" },
              result: [{ type: "text", text: "{\"round\":2,\"gapScore\":3}" }],
            },
            {
              type: "tool_use",
              id: "tool-create-artifact-1",
              name: "mcp__ralphx__create_plan_artifact",
              arguments: {
                title: "GitHub PR merge workflow",
                artifact_type: "design_doc",
              },
              result: [
                {
                  type: "text",
                  text: "{\"title\":\"GitHub PR merge workflow\",\"artifact_type\":\"design_doc\",\"version\":3}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "tool-team-artifacts-1",
              name: "mcp__ralphx__get_team_artifacts",
              arguments: { session_id: "session-mock-1" },
              result: [
                {
                  type: "text",
                  text: "{\"artifacts\":[{\"title\":\"Pipeline audit\"},{\"title\":\"Settings audit\"}]}",
                },
              ],
            },
          ],
          sender: "lead",
          createdAt: "2026-03-11T21:54:55.491580Z",
        },
        {
          id: "msg-ideation-orchestrator-3",
          sessionId: "session-mock-1",
          projectId: "project-mock-1",
          taskId: null,
          role: "assistant",
          content:
            "Critics are reporting back. I’m cross-checking repository usage with direct file reads and searches.",
          metadata: null,
          parentMessageId: "msg-ideation-orchestrator-2",
          conversationId: ideationReplayConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "text",
              text: "This bundle mirrors the real DB pattern where one orchestrator message mixes SendMessage, file reads, and verification updates.",
            },
            {
              type: "tool_use",
              id: "tool-send-message-1",
              name: "SendMessage",
              arguments: {
                recipient: "layer1-critic",
                content: "Nudged for findings.",
              },
              result: { success: true },
            },
            {
              type: "tool_use",
              id: "tool-read-1",
              name: "Read",
              arguments: {
                file_path: "src-tauri/src/application/chat_service/mod.rs",
              },
              result: "merge pipeline helper logic",
            },
            {
              type: "tool_use",
              id: "tool-grep-1",
              name: "Grep",
              arguments: { pattern: "create_pr", path: "src-tauri" },
              result: "found in merge workflow dialog",
            },
            {
              type: "tool_use",
              id: "tool-glob-1",
              name: "Glob",
              arguments: { pattern: "**/*merge*.rs", path: "src-tauri" },
              result: [
                "src-tauri/src/domain/state_machine/transition_handler/on_enter_states/merge.rs",
              ],
            },
            {
              type: "tool_use",
              id: "tool-ask-user-1",
              name: "mcp__ralphx__ask_user_question",
              arguments: {
                question: "Preferred default for automatic PR creation?",
                options: ["Always", "Configurable", "Never"],
              },
              result: { request_id: "question-1", status: "sent" },
            },
          ],
          sender: "lead",
          createdAt: "2026-03-11T21:58:43.308888Z",
        },
      ],
    },
  },
  ideation_widget_matrix: {
    conversations: [
      createConversation({
        id: ideationWidgetMatrixConversationId,
        contextType: "ideation",
        contextId: IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.contextId,
        claudeSessionId: "db-ideation-widget-matrix-session-1",
        title: "Widget matrix coverage",
        messageCount: 2,
        lastMessageAt: "2026-04-10T08:15:00.000000Z",
        createdAt: "2026-04-10T08:10:00.000000Z",
        updatedAt: "2026-04-10T08:15:00.000000Z",
      }),
      createConversation({
        id: ideationWidgetDelegatedConversationId,
        contextType: "project",
        contextId: "project-mock-1",
        providerHarness: "codex",
        providerSessionId: "thread-delegated-1",
        title: "Delegated reviewer",
        messageCount: 2,
        lastMessageAt: "2026-04-10T08:12:18.000000Z",
        createdAt: "2026-04-10T08:12:00.000000Z",
        updatedAt: "2026-04-10T08:12:18.000000Z",
      }),
    ],
    messages: {
      [ideationWidgetMatrixConversationId]: [
        {
          id: "msg-ideation-widget-user-1",
          sessionId: IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.contextId,
          projectId: "project-mock-1",
          taskId: null,
          role: "user",
          content: "Build a deterministic visual matrix for the chat widgets.",
          metadata: null,
          parentMessageId: null,
          conversationId: ideationWidgetMatrixConversationId,
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: "2026-04-10T08:10:00.000000Z",
        },
        {
          id: "msg-ideation-widget-assistant-1",
          sessionId: IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.contextId,
          projectId: "project-mock-1",
          taskId: null,
          role: "assistant",
          content:
            "This replay isolates the highest-variance widget families so visual coverage can target stable roots instead of the whole panel.",
          metadata: null,
          parentMessageId: "msg-ideation-widget-user-1",
          conversationId: ideationWidgetMatrixConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "tool_use",
              id: "proposal-create-1",
              name: "mcp__ralphx__create_task_proposal",
              arguments: {
                title: "Add execution widget snapshots",
                category: "testing",
              },
              result: [
                {
                  type: "text",
                  text: "{\"title\":\"Add execution widget snapshots\",\"category\":\"testing\"}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "proposal-update-1",
              name: "mcp__ralphx__update_task_proposal",
              arguments: {
                title: "Add execution widget snapshots",
                category: "quality",
                description: "Broaden the visual matrix",
              },
              result: [
                {
                  type: "text",
                  text: "{\"title\":\"Add execution widget snapshots\",\"category\":\"quality\"}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "proposal-delete-1",
              name: "mcp__ralphx__archive_task_proposal",
              arguments: { proposal_id: "proposal-delete-1" },
              result: [
                {
                  type: "text",
                  text: "{\"title\":\"Delete stale snapshot proposal\"}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "verification-complete-1",
              name: "mcp__ralphx__complete_plan_verification",
              arguments: {},
              result: [
                {
                  type: "text",
                  text: "{\"status\":\"verified\",\"verified_plan_artifact_id\":\"artifact-1\"}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "verification-get-1",
              name: "mcp__ralphx__get_plan_verification",
              arguments: {},
              result: [
                {
                  type: "text",
                  text: "{\"status\":\"verified\",\"plan_artifact_id\":\"artifact-1\",\"verified_plan_artifact_id\":\"artifact-1\",\"in_progress\":false}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "send-message-broadcast-1",
              name: "SendMessage",
              arguments: {
                type: "broadcast",
                recipient: "all",
                summary: "Broadcasted widget snapshot plan",
                content: "Broadcasted widget snapshot plan to all specialists so they can align on the visual matrix before merge.",
              },
              result: "Broadcast acknowledged",
            },
            {
              type: "tool_use",
              id: "ask-question-1",
              name: "mcp__ralphx__ask_user_question",
              arguments: {
                header: "Widget snapshot scope",
                question: "Should we snapshot every widget or only the layout-heavy ones?",
              },
              result: { request_id: "question-widget-scope", status: "sent" },
            },
            {
              type: "tool_use",
              id: "plan-create-1",
              name: "mcp__ralphx__create_plan_artifact",
              arguments: {
                title: "Chat widget matrix",
                artifact_type: "design_doc",
              },
              result: [
                {
                  type: "text",
                  text: "{\"name\":\"Chat widget matrix\",\"version\":1}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "plan-update-1",
              name: "mcp__ralphx__update_plan_artifact",
              arguments: {},
              result: [
                {
                  type: "text",
                  text: "{\"name\":\"Chat widget matrix\",\"version\":2}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "child-session-active-1",
              name: "mcp__ralphx__create_child_session",
              arguments: {
                title: "Verification follow-up",
                purpose: "verification",
              },
              result: [
                {
                  type: "text",
                  text: "{\"session_id\":\"child-session-active-1\",\"title\":\"Verification follow-up\",\"purpose\":\"verification\",\"orchestration_triggered\":true}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "child-session-pending-1",
              name: "mcp__ralphx__create_child_session",
              arguments: {
                title: "Queued specialist session",
                purpose: "general",
              },
              result: [
                {
                  type: "text",
                  text: "{\"session_id\":\"child-session-pending-1\",\"title\":\"Queued specialist session\",\"purpose\":\"general\",\"orchestration_triggered\":true}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "child-session-loading-1",
              name: "mcp__ralphx__create_child_session",
              arguments: {
                title: "Loading child session",
                purpose: "verification",
              },
              result: [
                {
                  type: "text",
                  text: "{\"session_id\":\"child-session-loading-1\",\"title\":\"Loading child session\",\"purpose\":\"verification\",\"orchestration_triggered\":true}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "child-session-error-1",
              name: "mcp__ralphx__create_child_session",
              arguments: {
                title: "Errored child session",
                purpose: "verification",
              },
              result: [
                {
                  type: "text",
                  text: "{\"session_id\":\"child-session-error-1\",\"title\":\"Errored child session\",\"purpose\":\"verification\",\"orchestration_triggered\":true}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "delegate-start-1",
              name: "delegate_start",
              arguments: {
                parent_session_id: IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.contextId,
                agent_name: "ralphx-execution-reviewer",
                prompt: "Review the delegated patch and report blockers with exact file references.",
                harness: "codex",
                model: "gpt-5.4",
                logical_effort: "high",
                sandbox_mode: "danger-full-access",
              },
              result: [
                {
                  type: "text",
                  text: "{\"job_id\":\"delegate-job-1\",\"status\":\"running\",\"agent_name\":\"ralphx-execution-reviewer\",\"harness\":\"codex\",\"delegated_session_id\":\"delegated-session-1\"}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "delegate-start-failed-1",
              name: "delegate_start",
              arguments: {
                parent_session_id: IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.contextId,
                agent_name: "ralphx-execution-fixer",
                prompt: "Patch the failing validation path and explain the root cause.",
                harness: "claude",
                model: "claude-sonnet-4",
                logical_effort: "medium",
                sandbox_mode: "danger-full-access",
              },
              result: [
                {
                  type: "text",
                  text: "{\"job_id\":\"delegate-job-failed\",\"status\":\"failed\",\"content\":\"Delegated fixer failed after the validation command returned a non-zero exit code.\",\"delegated_status\":{\"session\":{\"id\":\"delegated-session-failed-1\",\"title\":\"Delegated fixer\",\"status\":\"failed\",\"agent_name\":\"ralphx-execution-fixer\",\"harness\":\"claude\",\"provider_session_id\":\"claude-thread-failed-1\"},\"latest_run\":{\"agent_run_id\":\"delegated-run-failed-1\",\"status\":\"failed\",\"started_at\":\"2026-04-10T08:13:10.000000Z\",\"completed_at\":\"2026-04-10T08:13:22.000000Z\",\"harness\":\"claude\",\"provider_session_id\":\"claude-thread-failed-1\",\"logical_model\":\"claude-sonnet-4\",\"effective_model_id\":\"claude-sonnet-4\",\"logical_effort\":\"medium\",\"effective_effort\":\"medium\",\"approval_policy\":\"never\",\"sandbox_mode\":\"danger-full-access\",\"input_tokens\":940,\"output_tokens\":201,\"estimated_usd\":0.29}}}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "delegate-start-cancelled-1",
              name: "delegate_start",
              arguments: {
                parent_session_id: IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.contextId,
                agent_name: "ralphx-merge-auditor",
                prompt: "Check whether the merge preconditions are still valid.",
                harness: "codex",
                model: "gpt-5.4-mini",
                logical_effort: "low",
                sandbox_mode: "danger-full-access",
              },
              result: [
                {
                  type: "text",
                  text: "{\"job_id\":\"delegate-job-cancelled\",\"status\":\"cancelled\",\"content\":\"Delegated merge audit was cancelled before it produced a final recommendation.\",\"delegated_status\":{\"session\":{\"id\":\"delegated-session-cancelled-1\",\"title\":\"Delegated merge audit\",\"status\":\"cancelled\",\"agent_name\":\"ralphx-merge-auditor\",\"harness\":\"codex\",\"provider_session_id\":\"thread-cancelled-1\"},\"latest_run\":{\"agent_run_id\":\"delegated-run-cancelled-1\",\"status\":\"cancelled\",\"started_at\":\"2026-04-10T08:13:40.000000Z\",\"completed_at\":\"2026-04-10T08:13:46.000000Z\",\"harness\":\"codex\",\"provider_session_id\":\"thread-cancelled-1\",\"logical_model\":\"gpt-5.4-mini\",\"effective_model_id\":\"gpt-5.4-mini\",\"logical_effort\":\"low\",\"effective_effort\":\"low\",\"approval_policy\":\"never\",\"sandbox_mode\":\"danger-full-access\",\"input_tokens\":210,\"output_tokens\":0,\"estimated_usd\":0.02}}}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "delegate-wait-1",
              name: "delegate_wait",
              arguments: {
                job_id: "delegate-job-1",
                include_delegated_status: true,
              },
              result: [
                {
                  type: "text",
                  text: "{\"job_id\":\"delegate-job-1\",\"status\":\"completed\",\"content\":\"Delegated reviewer found one merge-blocking issue in src-tauri/src/http_server/handlers/coordination/mod.rs: the parent tool result still lacks durable parent-linked transcript metadata.\",\"delegated_status\":{\"conversation_id\":\"conv-delegated-1\",\"session\":{\"id\":\"delegated-session-1\",\"title\":\"Delegated reviewer\",\"status\":\"completed\",\"parent_context_type\":\"ideation\",\"parent_context_id\":\"session-widget-matrix\",\"agent_name\":\"ralphx-execution-reviewer\",\"harness\":\"codex\",\"provider_session_id\":\"thread-delegated-1\",\"created_at\":\"2026-04-10T08:12:00.000000Z\",\"updated_at\":\"2026-04-10T08:15:00.000000Z\",\"completed_at\":\"2026-04-10T08:15:00.000000Z\"},\"latest_run\":{\"agent_run_id\":\"delegated-run-1\",\"status\":\"completed\",\"started_at\":\"2026-04-10T08:12:10.000000Z\",\"completed_at\":\"2026-04-10T08:12:18.000000Z\",\"harness\":\"codex\",\"provider_session_id\":\"thread-delegated-1\",\"logical_model\":\"gpt-5.4\",\"effective_model_id\":\"gpt-5.4\",\"logical_effort\":\"high\",\"effective_effort\":\"high\",\"approval_policy\":\"never\",\"sandbox_mode\":\"danger-full-access\",\"input_tokens\":1240,\"output_tokens\":318,\"cache_read_tokens\":412,\"estimated_usd\":0.43},\"recent_messages\":[{\"role\":\"assistant\",\"content\":\"Delegated reviewer found one merge-blocking issue in src-tauri/src/http_server/handlers/coordination/mod.rs: the parent tool result still lacks durable parent-linked transcript metadata.\",\"created_at\":\"2026-04-10T08:12:18.000000Z\"}]}}",
                },
              ],
            },
            {
              type: "tool_use",
              id: "agent-card-1",
              name: "Agent",
              arguments: {
                name: "frontend-researcher",
                description: "Inspect the current chat chrome and identify repeated badge logic.",
                subagent_type: "Explore",
                model: "sonnet",
                prompt: "Audit the shared chat card styling and call out duplication.",
              },
              result: "Shared badge styles repeat across persisted and streaming task cards.\nagentId: agent123abc\n<usage>total_tokens: 1532\ntool_uses: 3\nduration_ms: 6200</usage>",
            },
            {
              type: "tool_use",
              id: "task-card-1",
              name: "Task",
              arguments: {
                description: "Run repository smoke checks",
                subagent_type: "general-purpose",
                model: "opus",
                prompt: "Run the smoke checks and summarize the result.",
              },
              result: [
                {
                  type: "text",
                  text: "Smoke checks completed with one follow-up note.\nagentId: task123abc\n<usage>total_tokens: 2048\ntool_uses: 2\nduration_ms: 8200</usage>",
                },
                {
                  type: "tool_use",
                  id: "task-card-1-bash",
                  name: "bash",
                  arguments: { command: "npm --prefix frontend run typecheck" },
                  result: "Typecheck completed cleanly.",
                },
              ],
            },
          ],
          sender: "lead",
          createdAt: "2026-04-10T08:15:00.000000Z",
        },
      ],
      [ideationWidgetDelegatedConversationId]: [
        {
          id: "msg-delegated-user-1",
          sessionId: "delegated-session-1",
          projectId: "project-mock-1",
          taskId: null,
          role: "user",
          content: "Review the delegated patch and report blockers with exact file references.",
          metadata: null,
          parentMessageId: null,
          conversationId: ideationWidgetDelegatedConversationId,
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: "2026-04-10T08:12:00.000000Z",
        },
        {
          id: "msg-delegated-assistant-1",
          sessionId: "delegated-session-1",
          projectId: "project-mock-1",
          taskId: null,
          role: "assistant",
          content: "Delegated reviewer found one merge-blocking issue in src-tauri/src/http_server/handlers/coordination/mod.rs: the parent tool result still lacks durable parent-linked transcript metadata.",
          metadata: null,
          parentMessageId: "msg-delegated-user-1",
          conversationId: ideationWidgetDelegatedConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "text",
              text: "I inspected the coordination handler first and then verified the parent linkage path.",
            },
            {
              type: "tool_use",
              id: "delegated-bash-1",
              name: "bash",
              arguments: { command: "rg -n \"parent_tool_use_id\" src-tauri/src/http_server/handlers/coordination/mod.rs" },
              result: "src-tauri/src/http_server/handlers/coordination/mod.rs:208:        tool_use_id: payload.tool_use_id.clone()",
            },
            {
              type: "text",
              text: "Blocking issue: the parent tool result still lacked durable parent-linked transcript metadata in the stored path.",
            },
          ],
          sender: null,
          createdAt: "2026-04-10T08:12:18.000000Z",
        },
      ],
    },
    childSessionStatuses: {
      "child-session-active-1": {
        session_id: "child-session-active-1",
        title: "Verification follow-up",
        agent_state: { estimated_status: "likely_generating" },
        recent_messages: [
          {
            role: "assistant",
            content: "I am checking the latest verification gaps before reporting back.",
            created_at: "2026-04-10T08:14:00.000000Z",
          },
          {
            role: "user",
            content: "Keep the replay deterministic and snapshot the expanded widget state.",
            created_at: "2026-04-10T08:14:30.000000Z",
          },
        ],
        lastEffectiveModel: "gpt-5.4-mini",
      },
      "child-session-pending-1": {
        session_id: "child-session-pending-1",
        title: "Queued specialist session",
        agent_state: { estimated_status: "idle" },
        recent_messages: [],
        pending_initial_prompt: "Waiting for pipeline capacity before spawning the specialist.",
        lastEffectiveModel: null,
      },
      "child-session-loading-1": {
        session_id: "child-session-loading-1",
        title: "Loading child session",
        agent_state: { estimated_status: "likely_waiting" },
        recent_messages: [
          {
            role: "assistant",
            content: "This response should never render in the loading screenshot.",
            created_at: "2026-04-10T08:15:30.000000Z",
          },
        ],
        lastEffectiveModel: "claude-sonnet-4",
      },
      "child-session-error-1": {
        session_id: "child-session-error-1",
        title: "Errored child session",
        agent_state: { estimated_status: "likely_generating" },
        recent_messages: [
          {
            role: "assistant",
            content: "This response should be replaced by the visual error override.",
            created_at: "2026-04-10T08:15:45.000000Z",
          },
        ],
        lastEffectiveModel: "gpt-5.4",
      },
    },
  },
  execution_db_compact: {
    conversations: [
      createConversation({
        id: executionReplayConversationId,
        contextType: "task_execution",
        contextId: TASK_REPLAY_CONTEXTS.execution_db_compact.contextId,
        claudeSessionId: "db-execution-claude-session-1",
        title: "Execute task-mock-4",
        messageCount: 2,
        lastMessageAt: "2026-04-07T01:13:52.882129Z",
        createdAt: "2026-04-07T01:13:52.876318Z",
        updatedAt: "2026-04-07T01:13:52.882129Z",
      }),
    ],
    messages: {
      [executionReplayConversationId]: [
        {
          id: "msg-exec-user-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: "task-mock-4",
          role: "user",
          content: "Task runtime bootstrap",
          metadata: JSON.stringify({
            hidden_from_ui: true,
            source: "task_runtime_bootstrap",
          }),
          parentMessageId: null,
          conversationId: executionReplayConversationId,
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: "2026-04-07T01:13:52.876318Z",
        },
        {
          id: "msg-exec-worker-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: "task-mock-4",
          role: "assistant",
          content:
            "The blocker task is merged. I’m checking the current state of the modified files and validating the pipeline before making changes.",
          metadata: null,
          parentMessageId: "msg-exec-user-1",
          conversationId: executionReplayConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "text",
              text: "Execution replay sampled from a compact two-message worker conversation.",
            },
            {
              type: "tool_use",
              id: "exec-read-1",
              name: "Read",
              arguments: {
                file_path: "frontend/src/components/Chat/MessageItem.tsx",
              },
              result: "provider metadata rendering",
            },
            {
              type: "tool_use",
              id: "exec-grep-1",
              name: "Grep",
              arguments: {
                pattern: "providerHarness",
                path: "frontend/src/components/Chat",
              },
              result: "multiple matches",
            },
            {
              type: "tool_use",
              id: "exec-bash-1",
              name: "bash",
              arguments: {
                command:
                  "npm --prefix frontend run test:run -- src/components/Chat/MessageItem.test.tsx",
              },
              result: "tests passed",
            },
          ],
          sender: "worker",
          attributionSource: "native",
          providerHarness: "claude",
          providerSessionId: "db-execution-claude-session-1",
          upstreamProvider: "anthropic",
          providerProfile: null,
          logicalModel: "claude-sonnet-4-6",
          effectiveModelId: "claude-sonnet-4-6",
          logicalEffort: "medium",
          effectiveEffort: "medium",
          inputTokens: 980,
          outputTokens: 164,
          cacheCreationTokens: 0,
          cacheReadTokens: 512,
          estimatedUsd: 0.02,
          createdAt: "2026-04-07T01:13:52.882129Z",
        },
      ],
    },
  },
  review_db_compact: {
    conversations: [
      createConversation({
        id: reviewReplayConversationId,
        contextType: "review",
        contextId: TASK_REPLAY_CONTEXTS.review_db_compact.contextId,
        claudeSessionId: "db-review-claude-session-1",
        title: "Review task-mock-5",
        messageCount: 2,
        lastMessageAt: "2026-04-07T01:16:57.845453Z",
        createdAt: "2026-04-07T01:16:57.831419Z",
        updatedAt: "2026-04-07T01:16:57.845453Z",
      }),
    ],
    messages: {
      [reviewReplayConversationId]: [
        {
          id: "msg-review-user-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: "task-mock-5",
          role: "user",
          content: "Review runtime bootstrap",
          metadata: JSON.stringify({
            hidden_from_ui: true,
            source: "task_runtime_bootstrap",
          }),
          parentMessageId: null,
          conversationId: reviewReplayConversationId,
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: "2026-04-07T01:16:57.831419Z",
        },
        {
          id: "msg-reviewer-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: "task-mock-5",
          role: "assistant",
          content:
            "This is a first review. I’m examining the diff and changed files before deciding whether to request changes or approve.",
          metadata: null,
          parentMessageId: "msg-review-user-1",
          conversationId: reviewReplayConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "text",
              text: "Reviewer replay sampled from a compact two-message real conversation.",
            },
            {
              type: "tool_use",
              id: "review-complete-1",
              name: "complete_review",
              arguments: {
                decision: "changes_requested",
                feedback: "Need stronger widget replay coverage.",
                issues: [
                  {
                    severity: "major",
                    description: "Missing DB-derived chat replay fixtures",
                  },
                ],
              },
              result: {
                success: true,
                new_status: "reviewing",
                followup_session_id: "followup-1",
              },
            },
          ],
          sender: "reviewer",
          attributionSource: "native",
          providerHarness: "claude",
          providerSessionId: "db-review-claude-session-1",
          upstreamProvider: "anthropic",
          providerProfile: null,
          logicalModel: "claude-sonnet-4-6",
          effectiveModelId: "claude-sonnet-4-6",
          logicalEffort: "medium",
          effectiveEffort: "medium",
          inputTokens: 1506,
          outputTokens: 203,
          cacheCreationTokens: 0,
          cacheReadTokens: 644,
          estimatedUsd: 0.03,
          createdAt: "2026-04-07T01:16:57.845453Z",
        },
      ],
    },
  },
  review_widget_matrix: {
    conversations: [
      createConversation({
        id: reviewWidgetMatrixConversationId,
        contextType: "review",
        contextId: TASK_REPLAY_CONTEXTS.review_widget_matrix.contextId,
        claudeSessionId: "db-review-widget-matrix-session-1",
        title: "Review widget matrix",
        messageCount: 2,
        lastMessageAt: "2026-04-10T08:22:00.000000Z",
        createdAt: "2026-04-10T08:20:00.000000Z",
        updatedAt: "2026-04-10T08:22:00.000000Z",
      }),
    ],
    messages: {
      [reviewWidgetMatrixConversationId]: [
        {
          id: "msg-review-widget-user-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: TASK_REPLAY_CONTEXTS.review_widget_matrix.contextId,
          role: "user",
          content: "Review the widget matrix task.",
          metadata: null,
          parentMessageId: null,
          conversationId: reviewWidgetMatrixConversationId,
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: "2026-04-10T08:20:00.000000Z",
        },
        {
          id: "msg-review-widget-assistant-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: TASK_REPLAY_CONTEXTS.review_widget_matrix.contextId,
          role: "assistant",
          content: "Review widget matrix states.",
          metadata: null,
          parentMessageId: "msg-review-widget-user-1",
          conversationId: reviewWidgetMatrixConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "tool_use",
              id: "review-approved-1",
              name: "complete_review",
              arguments: {
                decision: "approved",
                feedback: "Snapshot gates look stable after explicit loaded-content waits.",
                issues: [],
              },
              result: {
                success: true,
                new_status: "approved",
              },
            },
            {
              type: "tool_use",
              id: "review-notes-1",
              name: "get_review_notes",
              arguments: {},
              result: [
                {
                  type: "text",
                  text: "{\"reviews\":[{\"id\":\"note-1\",\"reviewer\":\"qa-bot\",\"outcome\":\"changes_requested\",\"summary\":\"Snapshot mismatch\",\"notes\":\"Loading-state baseline was stale.\",\"issues\":[{\"severity\":\"major\",\"description\":\"Replace stale baseline\"}],\"created_at\":\"2026-04-10T08:21:00.000000Z\"},{\"id\":\"note-2\",\"reviewer\":\"qa-bot\",\"outcome\":\"approved\",\"summary\":\"Snapshot verified\",\"notes\":\"Loaded-content baseline now matches the real panel.\",\"issues\":[],\"created_at\":\"2026-04-10T08:22:00.000000Z\"}],\"revision_count\":1,\"max_revisions\":3}",
                },
              ],
            },
          ],
          sender: "reviewer",
          createdAt: "2026-04-10T08:22:00.000000Z",
        },
      ],
    },
  },
  merge_db_compact: {
    conversations: [
      createConversation({
        id: mergeReplayConversationId,
        contextType: "merge",
        contextId: TASK_REPLAY_CONTEXTS.merge_db_compact.contextId,
        claudeSessionId: null,
        providerHarness: "codex",
        providerSessionId: "thread-merge-codex-1",
        title: "Merge task-mock-merge-incomplete",
        messageCount: 2,
        lastMessageAt: "2026-04-07T01:14:03.573602Z",
        createdAt: "2026-04-07T01:14:03.561142Z",
        updatedAt: "2026-04-07T01:14:03.573602Z",
      }),
    ],
    messages: {
      [mergeReplayConversationId]: [
        {
          id: "msg-merge-user-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: "task-mock-merge-incomplete",
          role: "user",
          content:
            "Resolve the merge conflict for task-mock-merge-incomplete",
          metadata: null,
          parentMessageId: null,
          conversationId: mergeReplayConversationId,
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: "2026-04-07T01:14:03.561142Z",
        },
        {
          id: "msg-merge-merger-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: "task-mock-merge-incomplete",
          role: "assistant",
          content:
            "On the plan branch. Conflict file is src/commands/gateway.ts. I’m resolving and validating before finalizing the merge.",
          metadata: null,
          parentMessageId: "msg-merge-user-1",
          conversationId: mergeReplayConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "text",
              text: "Merge replay sampled from a compact two-message merger conversation.",
            },
            {
              type: "tool_use",
              id: "merge-target-1",
              name: "get_merge_target",
              arguments: {},
              result: { source_branch: "task/mock-merge", target_branch: "main" },
            },
            {
              type: "tool_use",
              id: "merge-complete-1",
              name: "mcp__ralphx__complete_merge",
              arguments: { commit_sha: "abcdef1234567" },
              result: {
                success: true,
                message: "Merged cleanly",
                new_status: "merged",
              },
            },
          ],
          sender: "merger",
          attributionSource: "native",
          providerHarness: "codex",
          providerSessionId: "thread-merge-codex-1",
          upstreamProvider: "openai",
          providerProfile: null,
          logicalModel: "gpt-5.4",
          effectiveModelId: "gpt-5.4",
          logicalEffort: "high",
          effectiveEffort: "high",
          inputTokens: 1244,
          outputTokens: 188,
          cacheCreationTokens: 0,
          cacheReadTokens: 608,
          estimatedUsd: 0.03,
          createdAt: "2026-04-07T01:14:03.573602Z",
        },
      ],
    },
  },
  merge_widget_matrix: {
    conversations: [
      createConversation({
        id: mergeWidgetMatrixConversationId,
        contextType: "merge",
        contextId: TASK_REPLAY_CONTEXTS.merge_widget_matrix.contextId,
        providerHarness: "codex",
        providerSessionId: "thread-merge-widget-matrix-1",
        title: "Merge widget matrix",
        messageCount: 2,
        lastMessageAt: "2026-04-10T08:26:00.000000Z",
        createdAt: "2026-04-10T08:24:00.000000Z",
        updatedAt: "2026-04-10T08:26:00.000000Z",
      }),
    ],
    messages: {
      [mergeWidgetMatrixConversationId]: [
        {
          id: "msg-merge-widget-user-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: TASK_REPLAY_CONTEXTS.merge_widget_matrix.contextId,
          role: "user",
          content: "Render merge widget states for the snapshot matrix.",
          metadata: null,
          parentMessageId: null,
          conversationId: mergeWidgetMatrixConversationId,
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: "2026-04-10T08:24:00.000000Z",
        },
        {
          id: "msg-merge-widget-assistant-1",
          sessionId: null,
          projectId: "project-mock-1",
          taskId: TASK_REPLAY_CONTEXTS.merge_widget_matrix.contextId,
          role: "assistant",
          content: "Merge widget matrix states.",
          metadata: null,
          parentMessageId: "msg-merge-widget-user-1",
          conversationId: mergeWidgetMatrixConversationId,
          toolCalls: null,
          contentBlocks: [
            {
              type: "tool_use",
              id: "merge-target-1",
              name: "mcp__ralphx__get_merge_target",
              arguments: {},
              result: { source_branch: "task/widget-matrix", target_branch: "main" },
            },
            {
              type: "tool_use",
              id: "merge-conflict-1",
              name: "mcp__ralphx__report_conflict",
              arguments: {
                reason: "Conflict in snapshot helper",
                conflict_files: [
                  "frontend/tests/fixtures/chat.fixtures.ts",
                  "frontend/src/api-mock/chat-scenarios.ts",
                ],
              },
              result: { success: true },
            },
            {
              type: "tool_use",
              id: "merge-incomplete-1",
              name: "mcp__ralphx__report_incomplete",
              arguments: {
                reason: "Waiting for snapshot approval",
                diagnostic_info: "Approval required before finalizing the visual matrix.",
              },
              result: { success: true },
            },
            {
              type: "tool_use",
              id: "merge-complete-1",
              name: "mcp__ralphx__complete_merge",
              arguments: { commit_sha: "abcdef1234567" },
              result: {
                success: true,
                message: "Merged widget coverage updates.",
                new_status: "merged",
              },
            },
          ],
          sender: "merger",
          createdAt: "2026-04-10T08:26:00.000000Z",
        },
      ],
    },
  },
};
