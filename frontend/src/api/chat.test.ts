import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  parseContentBlocks,
  parseToolCalls,
  listConversations,
  listConversationsPage,
  getConversation,
  getConversationSummary,
  getConversationMessagesPage,
  getConversationTimelinePage,
  getAgentMessageToolCallDetail,
  getAgentTimelineItemToolCallDetail,
  getConversationStats,
  getAgentWorkspacePrReviewContext,
  setAgentWorkspacePrReviewAutoApprove,
  setAgentWorkspacePrReviewMonitoring,
  getAgentWorkspaceReviewContext,
  getAgentWorkspaceReviewStartPreview,
  startAgentWorkspaceReview,
  startAgentWorkspaceReviewFixer,
  approveAgentWorkspaceReviewAnyway,
  listAgentConversationIssues,
  updateAgentConversationIssueStatus,
  convertAgentConversationIssueFollowup,
  submitAgentWorkspacePrReviewAction,
  skipAgentWorkspacePrReviewAction,
  createConversation,
  updateConversationTitle,
  spawnConversationSessionNamer,
  archiveConversation,
  restoreConversation,
  setAgentConversationMuted,
  getAgentRunStatus,
  getAgentConversationWorkspaceFreshness,
  openAgentConversationWorkspace,
  openAgentConversationWorkspacePath,
  listAgentConversationWorkspacePublicationEvents,
  listAgentConversationWorkspacesByProject,
  listAgentSidebarConversations,
  updateAgentConversationWorkspaceFromBase,
  commitAgentConversationWorkspaceLocally,
  precomputeAgentConversationWorkspacePrDescription,
  setAgentConversationWorkspaceAutoPublish,
  setAgentConversationWorkspacePrSupervision,
  startAgentConversation,
  startAgentConversationInvokeInput,
  transformStartAgentConversationResponse,
  StartAgentConversationResponseSchema,
  forkAgentConversation,
  switchAgentConversationMode,
  updateAgentConversationCoordinationMode,
  copyAgentConversationPlan,
  importAgentConversationPlan,
  sendAgentMessage,
  getQueuedAgentMessages,
  deleteQueuedAgentMessage,
  sendQueuedAgentMessageNow,
  isChatServiceAvailable,
  stopAgent,
  isAgentRunning,
  getAgentRunningStates,
  getAgentConversationRuntimeIndex,
  getAgentConversationRuntimeStatuses,
  chatApi,
  getConversationActiveState,
  getChildSessionStatus,
  AgentConversationWorkspaceResponseSchema,
} from "./chat";
import type { ConversationActiveStateResponse } from "./chat";
import { backendApiUrl } from "./backend";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

function planSeedConversationResponse() {
  return {
    id: "conversation-plan",
    context_type: "project",
    context_id: "project-1",
    claude_session_id: null,
    provider_session_id: null,
    provider_harness: null,
    agent_mode: "plan",
    title: "Plan chat",
    message_count: 2,
    last_message_at: null,
    created_at: "2026-01-24T10:00:00Z",
    updated_at: "2026-01-24T10:05:00Z",
    archived_at: null,
  };
}

function planSeedWorkspaceResponse() {
  return {
    conversation_id: "conversation-plan",
    project_id: "project-1",
    mode: "plan",
    base_ref_kind: "project_default",
    base_ref: "main",
    base_display_name: "Project default (main)",
    base_commit: null,
    branch_name: "ralphx/demo/agent-conversation-plan",
    worktree_path: "/tmp/ralphx/conversation-plan",
    linked_ideation_session_id: "session-plan",
    linked_plan_branch_id: null,
    publication_pr_number: null,
    publication_pr_url: null,
    publication_pr_status: null,
    publication_push_status: null,
    status: "active",
    created_at: "2026-01-24T10:00:00Z",
    updated_at: "2026-01-24T10:05:00Z",
  };
}

function planSeedArtifactResponse() {
  return {
    id: "artifact-plan",
    name: "Imported plan",
    artifact_type: "specification",
    content_type: "inline",
    content: "# Imported plan",
    created_at: "2026-01-24T10:05:00Z",
    created_by: "user",
    version: 1,
    bucket_id: "prd-library",
    task_id: null,
    process_id: null,
    derived_from: ["source-plan:v2"],
    plan_approval_status: "draft",
    plan_approved_artifact_id: null,
    plan_approved_version: null,
    plan_approved_at: null,
  };
}

describe("chat api", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    delete window.__TAURI_INTERNALS__;
    delete window.__mockChatApi;
  });

  it("parses tool calls", () => {
    const parsed = parseToolCalls(
      '[{"id":"t1","name":"bash","arguments":{"command":"ls"}}]',
    );
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toMatchObject({ id: "t1", name: "bash" });
  });

  it("preserves parent tool linkage on parsed tool calls", () => {
    const parsed = parseToolCalls(
      '[{"id":"t1","name":"bash","arguments":{"command":"ls"},"parent_tool_use_id":"delegate-1"}]',
    );
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toMatchObject({
      id: "t1",
      name: "bash",
      parentToolUseId: "delegate-1",
    });
  });

  it("preserves preview metadata and detail refs on parsed tool calls", () => {
    const parsed = parseToolCalls(
      JSON.stringify([
        {
          id: "t1",
          name: "bash",
          arguments: { command: "cat big.log" },
          result: "line 1\nline 2",
          result_preview_truncated: true,
          result_preview_line_count: 40,
          result_preview_omitted_lines: 30,
          result_preview_original_bytes: 12000,
          result_preview_paths: ["$.task.details"],
          detail_ref: {
            conversation_id: "conv-1",
            message_id: "msg-1",
            tool_call_id: "t1",
          },
        },
      ]),
    );

    expect(parsed[0]).toMatchObject({
      id: "t1",
      resultPreviewTruncated: true,
      resultPreviewLineCount: 40,
      resultPreviewOmittedLines: 30,
      resultPreviewOriginalBytes: 12000,
      resultPreviewPaths: ["$.task.details"],
      detailRef: {
        conversationId: "conv-1",
        messageId: "msg-1",
        toolCallId: "t1",
      },
    });
  });

  it("preserves argument preview metadata and diff previews on parsed tool calls", () => {
    const parsed = parseToolCalls([
      {
        id: "tool-edit",
        name: "edit",
        arguments: { file_path: "src/example.ts" },
        arguments_preview_truncated: true,
        arguments_preview_original_bytes: 2400,
        arguments_preview_line_count: 120,
        arguments_preview_omitted_lines: 114,
        diff_preview: {
          file_path: "src/example.ts",
          language: "typescript",
          hunks: [
            {
              old_start: 1,
              old_lines: 2,
              new_start: 1,
              new_lines: 2,
              header: "@@ -1,2 +1,2 @@",
              lines: [
                {
                  kind: "context",
                  content: "line 1",
                  old_line_num: 1,
                  new_line_num: 1,
                },
                {
                  kind: "addition",
                  content: "line 2 changed",
                  old_line_num: null,
                  new_line_num: 2,
                },
              ],
            },
          ],
          old_total_lines: 60,
          new_total_lines: 60,
          is_binary: false,
        },
      },
    ]);

    expect(parsed[0]).toMatchObject({
      id: "tool-edit",
      argumentsPreviewTruncated: true,
      argumentsPreviewOriginalBytes: 2400,
      argumentsPreviewLineCount: 120,
      argumentsPreviewOmittedLines: 114,
      diffPreview: {
        filePath: "src/example.ts",
        language: "typescript",
        oldTotalLines: 60,
        newTotalLines: 60,
        hunks: [
          {
            oldStart: 1,
            newStart: 1,
            lines: [
              {
                content: "line 1",
                oldLineNum: 1,
                newLineNum: 1,
              },
              {
                content: "line 2 changed",
                oldLineNum: null,
                newLineNum: 2,
              },
            ],
          },
        ],
      },
    });
  });

  it("preserves camelCase preview metadata and ignores invalid detail refs on parsed tool calls", () => {
    const parsed = parseToolCalls([
      {
        id: "t1",
        name: "bash",
        arguments: { command: "cat big.log" },
        result: "line 1",
        resultPreviewTruncated: true,
        resultPreviewLineCount: 12,
        resultPreviewOmittedLines: 2,
        resultPreviewOriginalBytes: 1200,
        resultPreviewPaths: ["$.output"],
        detailRef: {
          conversationId: "conv-1",
        },
      },
    ]);

    expect(parsed[0]).toMatchObject({
      id: "t1",
      resultPreviewTruncated: true,
      resultPreviewLineCount: 12,
      resultPreviewOmittedLines: 2,
      resultPreviewOriginalBytes: 1200,
      resultPreviewPaths: ["$.output"],
    });
    expect(parsed[0]?.detailRef).toBeUndefined();
  });

  it("preserves tool call errors and snake/camel diff context variants", () => {
    const parsed = parseToolCalls([
      {
        id: "t1",
        name: "edit",
        arguments: {},
        error: "edit failed",
        diff_context: {
          file_path: "src/main.rs",
          old_content: "old",
          old_file_exists: true,
        },
      },
      {
        id: "t2",
        name: "write",
        arguments: {},
        diffContext: {
          filePath: "src/lib.rs",
          oldContent: "before",
          oldFileExists: false,
        },
      },
    ]);

    expect(parsed[0]).toMatchObject({
      error: "edit failed",
      diffContext: {
        filePath: "src/main.rs",
        oldContent: "old",
        oldFileExists: true,
      },
    });
    expect(parsed[1]).toMatchObject({
      diffContext: {
        filePath: "src/lib.rs",
        oldContent: "before",
        oldFileExists: false,
      },
    });
  });

  it("parses content blocks", () => {
    const parsed = parseContentBlocks('[{"type":"text","text":"hello"}]');
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toMatchObject({ type: "text", text: "hello" });
  });

  it("preserves parent tool linkage on parsed content blocks", () => {
    const parsed = parseContentBlocks(
      '[{"type":"tool_use","id":"tool-1","name":"bash","arguments":{"command":"ls"},"parent_tool_use_id":"delegate-1"}]',
    );
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toMatchObject({
      type: "tool_use",
      id: "tool-1",
      name: "bash",
      parentToolUseId: "delegate-1",
    });
  });

  it("preserves preview metadata and detail refs on parsed content blocks", () => {
    const parsed = parseContentBlocks(
      JSON.stringify([
        {
          type: "tool_use",
          id: "tool-1",
          name: "read",
          input: { file_path: "big.txt" },
          result: "first lines",
          result_preview_truncated: true,
          result_preview_line_count: 20,
          result_preview_paths: ["$.content[1].text"],
          detail_ref: {
            conversation_id: "conv-1",
            message_id: "msg-1",
            tool_call_id: "tool-1",
            content_block_index: 2,
          },
        },
      ]),
    );

    expect(parsed[0]).toMatchObject({
      type: "tool_use",
      id: "tool-1",
      arguments: { file_path: "big.txt" },
      resultPreviewTruncated: true,
      resultPreviewLineCount: 20,
      resultPreviewPaths: ["$.content[1].text"],
      detailRef: {
        conversationId: "conv-1",
        messageId: "msg-1",
        toolCallId: "tool-1",
        contentBlockIndex: 2,
      },
    });
  });

  it("preserves argument preview metadata and diff previews on parsed content blocks", () => {
    const parsed = parseContentBlocks([
      {
        type: "tool_use",
        id: "tool-edit",
        name: "edit",
        arguments: { file_path: "src/example.ts" },
        arguments_preview_truncated: true,
        diff_preview: {
          file_path: "src/example.ts",
          language: "typescript",
          hunks: [
            {
              old_start: 1,
              old_lines: 1,
              new_start: 1,
              new_lines: 1,
              header: "@@ -1,1 +1,1 @@",
              lines: [
                {
                  kind: "addition",
                  content: "new line",
                  old_line_num: null,
                  new_line_num: 1,
                },
              ],
            },
          ],
          old_total_lines: 1,
          new_total_lines: 1,
          is_binary: false,
        },
      },
    ]);

    expect(parsed[0]).toMatchObject({
      type: "tool_use",
      argumentsPreviewTruncated: true,
      diffPreview: {
        filePath: "src/example.ts",
        hunks: [
          {
            oldStart: 1,
            lines: [
              {
                content: "new line",
                oldLineNum: null,
                newLineNum: 1,
              },
            ],
          },
        ],
      },
    });
  });

  it("preserves diff context on parsed content block tool uses", () => {
    const parsed = parseContentBlocks([
      {
        type: "tool_use",
        id: "tool-1",
        name: "edit",
        input: { file_path: "src/main.rs" },
        diff_context: {
          file_path: "src/main.rs",
          old_content: "old",
          old_file_exists: false,
        },
      },
    ]);

    expect(parsed[0]).toMatchObject({
      type: "tool_use",
      arguments: { file_path: "src/main.rs" },
      diffContext: {
        filePath: "src/main.rs",
        oldContent: "old",
        oldFileExists: false,
      },
    });
  });

  it("lists conversations", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "c1",
        context_type: "project",
        context_id: "p1",
        claude_session_id: null,
        provider_session_id: "thread-1",
        provider_harness: "codex",
        logical_model: "gpt-5.4",
        effective_model_id: "gpt-5.4-2026-04-01",
        logical_effort: "high",
        effective_effort: "high",
        automation_id: "automation-1",
        automation_run_id: "run-1",
        title: "Title",
        message_count: 2,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
      },
    ]);

    const result = await listConversations("project", "p1");

    expect(mockInvoke).toHaveBeenCalledWith("list_agent_conversations", {
      contextType: "project",
      contextId: "p1",
      includeArchived: false,
    });
    expect(result[0]).toMatchObject({
      contextType: "project",
      contextId: "p1",
      providerSessionId: "thread-1",
      providerHarness: "codex",
      upstreamProvider: null,
      providerProfile: null,
      logicalModel: "gpt-5.4",
      effectiveModelId: "gpt-5.4-2026-04-01",
      logicalEffort: "high",
      effectiveEffort: "high",
      automationId: "automation-1",
      automationRunId: "run-1",
      claudeSessionId: null,
    });
  });

  it("accepts persona_builder mode and transforms its persona bindings", async () => {
    mockInvoke.mockResolvedValue([
      {
        ...planSeedConversationResponse(),
        agent_mode: "persona_builder",
        persona_id: null,
        builder_draft_id: "draft-1",
        builder_result_persona_id: "persona-1",
      },
    ]);

    const result = await listConversations("project", "p1");

    expect(result[0]).toMatchObject({
      agentMode: "persona_builder",
      personaId: null,
      builderDraftId: "draft-1",
      builderResultPersonaId: "persona-1",
    });
  });

  it("threads a set snake_case persona_id to camelCase personaId", async () => {
    mockInvoke.mockResolvedValue([
      { ...planSeedConversationResponse(), persona_id: "persona-1" },
    ]);

    const result = await listConversations("project", "p1");

    expect(result[0]?.personaId).toBe("persona-1");
  });

  it("transforms body-free persona attribution from the latest conversation run", async () => {
    mockInvoke.mockResolvedValue([
      {
        ...planSeedConversationResponse(),
        persona_id: "persona-1",
        last_run_persona_run_id: "run-persona-1",
        last_run_persona_id: "persona-1",
        last_run_persona_slug: "design-voice",
        last_run_persona_version: 2,
        last_run_persona_content_hash: "persona-hash",
        last_run_persona_injected: false,
        last_run_persona_skipped_reason: "native_agent_flag",
        persona_runs: [
          {
            run_id: "run-persona-1",
            persona_id: "persona-1",
            persona_slug: "design-voice",
            persona_version: 2,
            persona_content_hash: "persona-hash",
            injected: false,
            skipped_reason: "native_agent_flag",
          },
        ],
      },
    ]);

    const result = await listConversations("project", "p1");

    expect(result[0]).toMatchObject({
      lastRunPersonaRunId: "run-persona-1",
      lastRunPersonaId: "persona-1",
      lastRunPersonaSlug: "design-voice",
      lastRunPersonaVersion: 2,
      lastRunPersonaContentHash: "persona-hash",
      lastRunPersonaInjected: false,
      lastRunPersonaSkippedReason: "native_agent_flag",
      personaRuns: [
        {
          id: "run-persona-1",
          personaId: "persona-1",
          personaSlug: "design-voice",
          personaVersion: 2,
          personaContentHash: "persona-hash",
          personaInjected: false,
          personaSkippedReason: "native_agent_flag",
        },
      ],
    });
    expect(JSON.stringify(result[0])).not.toContain(
      "SECRET_PERSONA_BODY_SENTINEL",
    );
  });

  it("lists paginated conversations with server-side search", async () => {
    mockInvoke.mockResolvedValue({
      conversations: [
        {
          id: "c-page-1",
          context_type: "project",
          context_id: "p-page",
          claude_session_id: null,
          provider_session_id: "thread-page",
          provider_harness: "codex",
          title: "Fix sidebar pagination",
          message_count: 2,
          last_message_at: null,
          created_at: "2026-01-24T10:00:00Z",
          updated_at: "2026-01-24T10:00:00Z",
        },
      ],
      limit: 6,
      offset: 6,
      total: 11,
      has_more: true,
    });

    const result = await listConversationsPage(
      "project",
      "p-page",
      6,
      6,
      false,
      "sidebar",
    );

    expect(mockInvoke).toHaveBeenCalledWith("list_agent_conversations_page", {
      contextType: "project",
      contextId: "p-page",
      includeArchived: false,
      limit: 6,
      offset: 6,
      search: "sidebar",
    });
    expect(result).toMatchObject({
      limit: 6,
      offset: 6,
      total: 11,
      hasMore: true,
    });
    expect(result.conversations[0]).toMatchObject({
      id: "c-page-1",
      providerHarness: "codex",
    });
  });

  it("passes archivedOnly when requesting archived-only pages", async () => {
    mockInvoke.mockResolvedValue({
      conversations: [],
      limit: 1,
      offset: 0,
      total: 3,
      has_more: true,
    });

    await listConversationsPage(
      "project",
      "p-page",
      1,
      0,
      true,
      undefined,
      true,
    );

    expect(mockInvoke).toHaveBeenCalledWith("list_agent_conversations_page", {
      contextType: "project",
      contextId: "p-page",
      includeArchived: true,
      archivedOnly: true,
      limit: 1,
      offset: 0,
    });
  });

  it("preserves unknown provider harness values", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "c-unknown",
        context_type: "project",
        context_id: "p-unknown",
        claude_session_id: null,
        provider_session_id: "thread-unknown",
        provider_harness: "openai",
        title: "Unknown provider row",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
      },
    ]);

    const result = await listConversations("project", "p-unknown");

    expect(result[0]).toMatchObject({
      providerSessionId: "thread-unknown",
      providerHarness: "openai",
      claudeSessionId: null,
    });
  });

  it("spawns the session namer for an agent conversation", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await spawnConversationSessionNamer(
      "conversation-42",
      "fix the agents landing flow",
    );

    expect(mockInvoke).toHaveBeenCalledWith("spawn_session_namer", {
      conversationId: "conversation-42",
      firstMessage: "fix the agents landing flow",
    });
  });

  it("passes selected provider when spawning the session namer", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await spawnConversationSessionNamer(
      "conversation-42",
      "fix the agents landing flow",
      "codex",
    );

    expect(mockInvoke).toHaveBeenCalledWith("spawn_session_namer", {
      conversationId: "conversation-42",
      firstMessage: "fix the agents landing flow",
      providerHarness: "codex",
    });
  });

  it("does not infer claude harness from provider session id alone", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "c2",
        context_type: "project",
        context_id: "p2",
        claude_session_id: null,
        provider_session_id: "thread-legacy",
        provider_harness: null,
        title: "Legacy provider row",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
      },
    ]);

    const result = await listConversations("project", "p2");

    expect(result[0]).toMatchObject({
      providerSessionId: "thread-legacy",
      providerHarness: null,
      claudeSessionId: null,
    });
  });

  it("infers claude harness only from the legacy claude session id", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "c3",
        context_type: "project",
        context_id: "p3",
        claude_session_id: "claude-thread-1",
        provider_session_id: null,
        provider_harness: null,
        title: "Legacy claude row",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
      },
    ]);

    const result = await listConversations("project", "p3");

    expect(result[0]).toMatchObject({
      providerSessionId: "claude-thread-1",
      providerHarness: "claude",
      claudeSessionId: "claude-thread-1",
    });
  });

  it("gets conversation with transformed messages", async () => {
    mockInvoke.mockResolvedValue({
      conversation: {
        id: "c1",
        context_type: "project",
        context_id: "p1",
        claude_session_id: null,
        provider_session_id: "thread-2",
        provider_harness: "codex",
        title: null,
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
      },
      messages: [
        {
          id: "m1",
          conversation_id: "c1",
          role: "user",
          content: "Hello",
          metadata: '{"verification_result":true}',
          tool_calls: null,
          content_blocks: null,
          attribution_source: "native",
          provider_harness: "codex",
          provider_session_id: "thread-2",
          upstream_provider: "openai",
          provider_profile: null,
          logical_model: "gpt-5.4",
          effective_model_id: "gpt-5.4",
          logical_effort: "high",
          effective_effort: "high",
          input_tokens: 120,
          output_tokens: 40,
          cache_creation_tokens: 5,
          cache_read_tokens: 8,
          estimated_usd: 0.42,
          created_at: "2026-01-24T10:00:00Z",
        },
      ],
    });

    const result = await getConversation("c1");

    expect(mockInvoke).toHaveBeenCalledWith("get_agent_conversation", {
      conversationId: "c1",
    });
    expect(result.messages[0]).toMatchObject({
      id: "m1",
      conversationId: "c1",
      createdAt: "2026-01-24T10:00:00Z",
      metadata: '{"verification_result":true}',
      attributionSource: "native",
      providerHarness: "codex",
      providerSessionId: "thread-2",
      upstreamProvider: "openai",
      logicalModel: "gpt-5.4",
      effectiveEffort: "high",
      inputTokens: 120,
      estimatedUsd: 0.42,
    });
  });

  it("gets a paginated conversation message window", async () => {
    mockInvoke.mockResolvedValue({
      conversation: {
        id: "c1",
        context_type: "project",
        context_id: "p1",
        claude_session_id: null,
        provider_session_id: "thread-2",
        provider_harness: "codex",
        title: null,
        message_count: 3,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
      },
      messages: [
        {
          id: "m2",
          conversation_id: "c1",
          role: "user",
          content: "Latest tail message",
          metadata: null,
          tool_calls: null,
          content_blocks: null,
          attribution_source: "native",
          provider_harness: "codex",
          provider_session_id: "thread-2",
          upstream_provider: "openai",
          provider_profile: null,
          logical_model: "gpt-5.4",
          effective_model_id: "gpt-5.4",
          logical_effort: "high",
          effective_effort: "high",
          input_tokens: 12,
          output_tokens: 4,
          cache_creation_tokens: 0,
          cache_read_tokens: 0,
          estimated_usd: 0.02,
          created_at: "2026-01-24T10:00:01Z",
        },
      ],
      limit: 40,
      offset: 0,
      total_message_count: 3,
      has_older: true,
    });

    const result = await getConversationMessagesPage("c1", 40, 0);

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_messages_page",
      {
        conversationId: "c1",
        limit: 40,
        offset: 0,
      },
    );
    expect(result).toMatchObject({
      limit: 40,
      offset: 0,
      totalMessageCount: 3,
      hasOlder: true,
    });
    expect(result.messages[0]).toMatchObject({
      id: "m2",
      conversationId: "c1",
      providerHarness: "codex",
      providerSessionId: "thread-2",
      effectiveModelId: "gpt-5.4",
    });
  });

  it("falls back to the page conversation id when legacy messages omit conversation_id", async () => {
    mockInvoke.mockResolvedValue({
      conversation: {
        id: "c-legacy",
        context_type: "project",
        context_id: "p1",
        claude_session_id: null,
        provider_session_id: null,
        provider_harness: null,
        title: null,
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
      },
      messages: [
        {
          id: "m-legacy",
          role: "assistant",
          content: "Legacy row",
          metadata: null,
          tool_calls: null,
          content_blocks: null,
          attribution_source: null,
          provider_harness: null,
          provider_session_id: null,
          upstream_provider: null,
          provider_profile: null,
          logical_model: null,
          effective_model_id: null,
          logical_effort: null,
          effective_effort: null,
          created_at: "2026-01-24T10:00:01Z",
        },
      ],
      limit: 40,
      offset: 0,
      total_message_count: 1,
      has_older: false,
    });

    const result = await getConversationMessagesPage("c-legacy", 40, 0);

    expect(result.messages[0]?.conversationId).toBe("c-legacy");
  });

  it("gets a lightweight conversation summary", async () => {
    mockInvoke.mockResolvedValue({
      id: "c-summary",
      context_type: "project",
      context_id: "p1",
      claude_session_id: null,
      provider_session_id: "thread-summary",
      provider_harness: "codex",
      title: "Breadcrumb title",
      message_count: 9,
      last_message_at: "2026-01-24T10:05:00Z",
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:05:00Z",
      archived_at: null,
    });

    const result = await getConversationSummary("c-summary");

    expect(mockInvoke).toHaveBeenCalledWith("get_agent_conversation_summary", {
      conversationId: "c-summary",
    });
    expect(result).toMatchObject({
      id: "c-summary",
      contextType: "project",
      contextId: "p1",
      title: "Breadcrumb title",
      providerHarness: "codex",
    });
  });

  it("transforms normalized conversation timeline pages into renderable messages", async () => {
    mockInvoke.mockResolvedValue({
      conversation: {
        id: "c-timeline",
        context_type: "project",
        context_id: "p1",
        claude_session_id: null,
        provider_session_id: "thread-1",
        provider_harness: "codex",
        title: "Timeline",
        message_count: 1,
        last_message_at: "2026-01-24T10:00:01Z",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:01Z",
      },
      items: [
        {
          id: "block:msg-1:0",
          conversation_id: "c-timeline",
          message_id: "msg-1",
          run_id: "run-1",
          sequence: 4,
          block_index: 0,
          role: "orchestrator",
          kind: "text",
          status: "streaming",
          content: "Working",
          content_blocks: [{ type: "text", text: "Working" }],
          tool_call: null,
          metadata: null,
          provider_harness: "codex",
          provider_session_id: "thread-1",
          input_tokens: 10,
          output_tokens: 3,
          cache_creation_tokens: 1,
          cache_read_tokens: 2,
          estimated_usd: 0.01,
          created_at: "2026-01-24T10:00:01Z",
          updated_at: "2026-01-24T10:00:02Z",
          finalized_at: null,
        },
        {
          id: "block:msg-1:1",
          conversation_id: "c-timeline",
          message_id: "msg-1",
          run_id: "run-1",
          sequence: 5,
          block_index: 1,
          role: "orchestrator",
          kind: "tool_use",
          status: "finalized",
          content: "",
          content_blocks: [
            {
              type: "tool_use",
              id: "tool-1",
              name: "bash",
              arguments: { command: "cargo test" },
              detail_ref: {
                conversation_id: "c-timeline",
                message_id: "msg-1",
                tool_call_id: "tool-1",
                content_block_index: 1,
                timeline_item_id: "block:msg-1:1",
              },
            },
          ],
          tool_call: {
            id: "tool-1",
            name: "bash",
            arguments: { command: "cargo test" },
            result: "ok",
            detail_ref: {
              conversation_id: "c-timeline",
              message_id: "msg-1",
              tool_call_id: "tool-1",
              content_block_index: 1,
              timeline_item_id: "block:msg-1:1",
            },
          },
          metadata: '{"kind":"tool"}',
          provider_harness: "codex",
          provider_session_id: "thread-1",
          created_at: "2026-01-24T10:00:03Z",
          updated_at: "2026-01-24T10:00:04Z",
          finalized_at: "2026-01-24T10:00:04Z",
        },
      ],
      limit: 40,
      before_sequence: 6,
      total_item_count: 8,
      has_older: true,
      oldest_loaded_sequence: 4,
      newest_loaded_sequence: 5,
    });

    const result = await getConversationTimelinePage("c-timeline", 40, 6);

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_timeline_page",
      {
        conversationId: "c-timeline",
        limit: 40,
        beforeSequence: 6,
      },
    );
    expect(result).toMatchObject({
      limit: 40,
      beforeSequence: 6,
      totalItemCount: 8,
      hasOlder: true,
      oldestLoadedSequence: 4,
      newestLoadedSequence: 5,
    });
    expect(result.messages.map((message) => message.id)).toEqual([
      "block:msg-1:0",
      "block:msg-1:1",
    ]);
    expect(result.messages[0]).toMatchObject({
      parentMessageId: "msg-1",
      timelineStatus: "streaming",
      timelineKind: "text",
      timelineSequence: 4,
      timelineBlockIndex: 0,
      inputTokens: 10,
      providerHarness: "codex",
    });
    expect(result.items[1].toolCall?.detailRef).toMatchObject({
      conversationId: "c-timeline",
      messageId: "msg-1",
      toolCallId: "tool-1",
      contentBlockIndex: 1,
      timelineItemId: "block:msg-1:1",
    });
  });

  it("loads a full tool call detail by preview detail ref", async () => {
    mockInvoke.mockResolvedValue({
      tool_call: {
        id: "tool-1",
        name: "bash",
        arguments: { command: "cat big.log" },
        result: "full output",
      },
    });

    const result = await getAgentMessageToolCallDetail({
      conversationId: "conv-1",
      messageId: "msg-1",
      toolCallId: "tool-1",
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_message_tool_call_detail",
      {
        conversationId: "conv-1",
        messageId: "msg-1",
        toolCallId: "tool-1",
        contentBlockIndex: null,
      },
    );
    expect(result?.toolCall).toMatchObject({
      id: "tool-1",
      name: "bash",
      result: "full output",
    });
  });

  it("loads a full tool call detail by timeline item detail ref", async () => {
    mockInvoke.mockResolvedValue({
      tool_call: {
        id: "tool-1",
        name: "bash",
        arguments: { command: "cargo test" },
        result: "full output",
      },
    });

    const result = await getAgentMessageToolCallDetail({
      conversationId: "conv-1",
      messageId: "msg-1",
      timelineItemId: "block:msg-1:1",
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_timeline_item_tool_call_detail",
      {
        conversationId: "conv-1",
        timelineItemId: "block:msg-1:1",
      },
    );
    expect(result?.toolCall).toMatchObject({
      id: "tool-1",
      name: "bash",
      result: "full output",
    });
  });

  it("returns null when a timeline item detail payload is unavailable", async () => {
    mockInvoke.mockResolvedValue(null);

    const result = await getAgentTimelineItemToolCallDetail(
      "conv-1",
      "missing-timeline-item",
    );

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_timeline_item_tool_call_detail",
      {
        conversationId: "conv-1",
        timelineItemId: "missing-timeline-item",
      },
    );
    expect(result).toBeNull();
  });

  it("returns null when a preview detail ref no longer has a full result", async () => {
    mockInvoke.mockResolvedValue(null);

    const result = await getAgentMessageToolCallDetail({
      conversationId: "conv-1",
      messageId: "msg-1",
      contentBlockIndex: 1,
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_message_tool_call_detail",
      {
        conversationId: "conv-1",
        messageId: "msg-1",
        toolCallId: null,
        contentBlockIndex: 1,
      },
    );
    expect(result).toBeNull();
  });

  it("gets conversation stats with camelCase totals and buckets", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "c1",
      context_type: "project",
      context_id: "p1",
      provider_harness: "codex",
      upstream_provider: "openai",
      provider_profile: null,
      message_usage_totals: {
        input_tokens: 120,
        output_tokens: 40,
        cache_creation_tokens: 5,
        cache_read_tokens: 8,
        processed_tokens: 160,
        estimated_usd: 0.42,
      },
      run_usage_totals: {
        input_tokens: 999,
        output_tokens: 111,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        processed_tokens: 1110,
        estimated_usd: 1.25,
      },
      effective_usage_totals: {
        input_tokens: 120,
        output_tokens: 40,
        cache_creation_tokens: 5,
        cache_read_tokens: 8,
        processed_tokens: 160,
        estimated_usd: 0.42,
      },
      usage_coverage: {
        provider_message_count: 1,
        provider_messages_with_usage: 1,
        run_count: 1,
        runs_with_usage: 1,
        effective_run_conversation_count: 0,
        effective_message_conversation_count: 1,
        legacy_estimated_sample_count: 0,
        fallback_estimated_sample_count: 0,
        uncounted_sample_count: 0,
        effective_totals_source: "messages",
      },
      attribution_coverage: {
        provider_message_count: 1,
        provider_messages_with_attribution: 1,
        run_count: 1,
        runs_with_attribution: 1,
      },
      by_harness: [
        {
          key: "codex",
          count: 1,
          usage: {
            input_tokens: 120,
            output_tokens: 40,
            cache_creation_tokens: 5,
            cache_read_tokens: 8,
            processed_tokens: 160,
            estimated_usd: 0.42,
          },
        },
      ],
      by_upstream_provider: [],
      by_model: [],
      by_effort: [],
    });

    const result = await getConversationStats("c1");

    expect(mockInvoke).toHaveBeenCalledWith("get_agent_conversation_stats", {
      conversationId: "c1",
    });
    expect(result).toMatchObject({
      conversationId: "c1",
      providerHarness: "codex",
      upstreamProvider: "openai",
      usageCoverage: {
        effectiveTotalsSource: "messages",
      },
      effectiveUsageTotals: {
        inputTokens: 120,
        processedTokens: 160,
        estimatedUsd: 0.42,
      },
      byHarness: [
        {
          key: "codex",
          usage: {
            inputTokens: 120,
          },
        },
      ],
    });
  });

  it("creates conversation", async () => {
    mockInvoke.mockResolvedValue({
      id: "c1",
      context_type: "task",
      context_id: "t1",
      claude_session_id: null,
      provider_session_id: null,
      provider_harness: null,
      title: null,
      message_count: 0,
      last_message_at: null,
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:00:00Z",
    });

    await createConversation("task", "t1");

    expect(mockInvoke).toHaveBeenCalledWith("create_agent_conversation", {
      input: { contextType: "task", contextId: "t1" },
    });
  });

  it("creates titled conversation", async () => {
    mockInvoke.mockResolvedValue({
      id: "c-title",
      context_type: "project",
      context_id: "p1",
      claude_session_id: null,
      provider_session_id: null,
      provider_harness: null,
      title: "Build agent",
      message_count: 0,
      last_message_at: null,
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:00:00Z",
    });

    await createConversation("project", "p1", " Build agent ");

    expect(mockInvoke).toHaveBeenCalledWith("create_agent_conversation", {
      input: { contextType: "project", contextId: "p1", title: "Build agent" },
    });
  });

  it("creates a self-keyed standalone conversation without sending contextId", async () => {
    mockInvoke.mockResolvedValue({
      id: "standalone-1",
      context_type: "standalone",
      context_id: "standalone-1",
      claude_session_id: null,
      provider_session_id: null,
      provider_harness: null,
      title: null,
      message_count: 0,
      last_message_at: null,
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:00:00Z",
    });

    await createConversation("standalone");

    expect(mockInvoke).toHaveBeenCalledWith("create_agent_conversation", {
      input: { contextType: "standalone" },
    });
  });

  it("creates a persona builder with its mode persisted before setup", async () => {
    mockInvoke.mockResolvedValue({
      id: "standalone-builder-1",
      context_type: "standalone",
      context_id: "standalone-builder-1",
      claude_session_id: null,
      provider_session_id: null,
      provider_harness: null,
      agent_mode: "persona_builder",
      title: null,
      message_count: 0,
      last_message_at: null,
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:00:00Z",
    });

    await createConversation("standalone", null, undefined, "persona_builder");

    expect(mockInvoke).toHaveBeenCalledWith("create_agent_conversation", {
      input: { contextType: "standalone", mode: "persona_builder" },
    });
  });

  it("updates conversation title", async () => {
    mockInvoke.mockResolvedValue({
      id: "c-title",
      context_type: "project",
      context_id: "p1",
      claude_session_id: null,
      provider_session_id: null,
      provider_harness: null,
      title: "Review agent title",
      message_count: 2,
      last_message_at: null,
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:01:00Z",
    });

    const result = await updateConversationTitle(
      "c-title",
      " Review agent title ",
    );

    expect(mockInvoke).toHaveBeenCalledWith("update_agent_conversation_title", {
      input: {
        conversationId: "c-title",
        title: "Review agent title",
      },
    });
    expect(result.title).toBe("Review agent title");
  });

  it("archives conversation", async () => {
    mockInvoke.mockResolvedValue({
      conversation: {
        id: "c-archive",
        context_type: "project",
        context_id: "p1",
        claude_session_id: null,
        provider_session_id: null,
        provider_harness: null,
        title: "Old agent",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:01:00Z",
        archived_at: "2026-01-24T10:01:00Z",
      },
      cleanup: {
        runtime_shutdown_succeeded: true,
        cleanup_claim: "claimed",
        local_cleanup: "cleaned",
        message: null,
      },
    });

    const result = await archiveConversation("c-archive", { closePullRequest: false });

    expect(mockInvoke).toHaveBeenCalledWith("archive_agent_conversation", {
      conversationId: "c-archive",
      closePullRequest: false,
    });
    expect(result.conversation.archivedAt).toBe("2026-01-24T10:01:00Z");
    expect(result.cleanup.localCleanup).toBe("cleaned");
  });

  it("passes explicit PR closure intent when archiving", async () => {
    mockInvoke.mockResolvedValue({
      conversation: {
        id: "c-archive-close-pr",
        context_type: "project",
        context_id: "p1",
        claude_session_id: null,
        provider_session_id: null,
        provider_harness: null,
        title: "Close PR",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:01:00Z",
        archived_at: "2026-01-24T10:01:00Z",
      },
      cleanup: {
        runtime_shutdown_succeeded: true,
        cleanup_claim: "claimed",
        local_cleanup: "cleaned",
        message: null,
      },
    });

    await archiveConversation("c-archive-close-pr", { closePullRequest: true });

    expect(mockInvoke).toHaveBeenCalledWith("archive_agent_conversation", {
      conversationId: "c-archive-close-pr",
      closePullRequest: true,
    });
  });

  it("restores conversation", async () => {
    mockInvoke.mockResolvedValue({
      id: "c-restore",
      context_type: "project",
      context_id: "p1",
      claude_session_id: null,
      provider_session_id: null,
      provider_harness: null,
      title: "Old agent",
      message_count: 1,
      last_message_at: null,
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:02:00Z",
      archived_at: null,
    });

    const result = await restoreConversation("c-restore");

    expect(mockInvoke).toHaveBeenCalledWith("restore_agent_conversation", {
      conversationId: "c-restore",
    });
    expect(result.archivedAt).toBeNull();
  });

  it("gets nullable agent run status", async () => {
    mockInvoke.mockResolvedValue(null);
    const result = await getAgentRunStatus("c1");
    expect(result).toBeNull();
  });

  it("transforms body-free persona attribution on an agent run", async () => {
    mockInvoke.mockResolvedValue({
      id: "run-1",
      conversation_id: "c1",
      status: "running",
      started_at: "2026-07-13T06:19:00Z",
      completed_at: null,
      error_message: null,
      model_id: "gpt-5.5",
      model_label: "GPT-5.5",
      persona_id: "persona-design-voice",
      persona_slug: "design-voice",
      persona_version: 2,
      persona_content_hash: "persona-hash",
      persona_injected: false,
      persona_skipped_reason: "native_agent_flag",
    });

    await expect(getAgentRunStatus("c1")).resolves.toMatchObject({
      personaId: "persona-design-voice",
      personaSlug: "design-voice",
      personaVersion: 2,
      personaContentHash: "persona-hash",
      personaInjected: false,
      personaSkippedReason: "native_agent_flag",
    });
  });

  it("lists agent conversation workspaces for a project", async () => {
    mockInvoke.mockResolvedValue([
      {
        conversation_id: "conversation-1",
        project_id: "project-1",
        mode: "edit",
        base_ref_kind: "project_default",
        base_ref: "main",
        base_display_name: "Project default (main)",
        base_commit: null,
        branch_name: "ralphx/demo/agent-conversation-1",
        worktree_path: "/tmp/ralphx/conversation-1",
        linked_ideation_session_id: null,
        linked_plan_branch_id: null,
        publication_pr_number: null,
        publication_pr_url: null,
        publication_pr_status: null,
        publication_push_status: null,
        maintenance_operation: {
          operation_id: "maintenance-1",
          generation: 2,
          source: "base_update",
          stage: "repairing",
          status: "active",
          summary: "Resolving the base conflict",
          blocker: null,
          automatic_continuation: true,
          started_at: "2026-01-24T10:00:00Z",
          updated_at: "2026-01-24T10:01:00Z",
        },
        publication_metadata_attempt_id: "attempt-plan-1",
        publication_metadata_phase: "reconciling",
        publication_metadata_state: "unknown",
        pr_autofix_fingerprint_spend: {
          generations: 3,
          minutes: 92,
          budget_minutes: 45,
          is_exhausted: true,
        },
        auto_publish_enabled: true,
        auto_publish_paused_pr_autofix_enabled: null,
        auto_publish_paused_pr_auto_merge_desired: null,
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:01:00Z",
      },
    ]);

    const result = await listAgentConversationWorkspacesByProject("project-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "list_agent_conversation_workspaces_by_project",
      { projectId: "project-1" },
    );
    expect(result[0]).toMatchObject({
      conversationId: "conversation-1",
      projectId: "project-1",
      branchName: "ralphx/demo/agent-conversation-1",
      autoPublishInitialPrEnabled: false,
      maintenanceOperation: {
        operationId: "maintenance-1",
        generation: 2,
        stage: "repairing",
        status: "active",
        automaticContinuation: true,
      },
      prAutofixFingerprintSpend: {
        generations: 3,
        minutes: 92,
        budgetMinutes: 45,
        isExhausted: true,
      },
    });
  });

  it("keeps a missing maintenance operation compatible with older backends", () => {
    expect(
      AgentConversationWorkspaceResponseSchema.parse(planSeedWorkspaceResponse())
        .maintenance_operation,
    ).toBeNull();
  });

  it("rejects an unknown maintenance operation stage", () => {
    expect(() =>
      AgentConversationWorkspaceResponseSchema.parse({
        ...planSeedWorkspaceResponse(),
        maintenance_operation: {
          operation_id: "maintenance-1",
          generation: 1,
          source: "base_update",
          stage: "not_a_stage",
          status: "active",
          summary: null,
          blocker: null,
          automatic_continuation: true,
          started_at: "2026-01-24T10:00:00Z",
          updated_at: "2026-01-24T10:01:00Z",
        },
      }),
    ).toThrow();
  });

  it("opens an agent conversation workspace when Tauri returns null for Rust unit", async () => {
    mockInvoke.mockResolvedValue(null);

    await expect(
      openAgentConversationWorkspace("conversation-1", "cursor"),
    ).resolves.toBeUndefined();

    expect(mockInvoke).toHaveBeenCalledWith(
      "open_agent_conversation_workspace",
      { conversationId: "conversation-1", targetId: "cursor" },
    );
  });

  it("opens an agent conversation workspace path when Tauri returns null for Rust unit", async () => {
    mockInvoke.mockResolvedValue(null);

    await expect(
      openAgentConversationWorkspacePath(
        "conversation-1",
        "cursor",
        "/tmp/worktree/src/lib.rs",
      ),
    ).resolves.toBeUndefined();

    expect(mockInvoke).toHaveBeenCalledWith(
      "open_agent_conversation_workspace_path",
      {
        conversationId: "conversation-1",
        targetId: "cursor",
        path: "/tmp/worktree/src/lib.rs",
      },
    );
  });

  it("lists grouped agent sidebar conversations", async () => {
    mockInvoke.mockResolvedValue({
      groups: [
        {
          key: "merged",
          label: "Merged",
          total: 1,
          offset: 0,
          limit: 20,
          has_more: false,
          rows: [
            {
              conversation: {
                id: "conversation-1",
                context_type: "project",
                context_id: "project-1",
                claude_session_id: null,
                provider_session_id: "thread-1",
                provider_harness: "codex",
                title: "Merged sidebar work",
                message_count: 2,
                last_message_at: null,
                created_at: "2026-01-24T10:00:00Z",
                updated_at: "2026-01-24T10:00:00Z",
                archived_at: null,
              },
              workspace: {
                conversation_id: "conversation-1",
                project_id: "project-1",
                mode: "edit",
                base_ref_kind: "project_default",
                base_ref: "main",
                base_display_name: "Project default (main)",
                base_commit: null,
                branch_name: "ralphx/demo/agent-conversation-1",
                worktree_path: "/tmp/ralphx/conversation-1",
                linked_ideation_session_id: null,
                linked_plan_branch_id: null,
                publication_pr_number: 123,
                publication_pr_url: "https://github.com/acme/demo/pull/123",
                publication_pr_status: "merged",
                publication_push_status: "published",
                status: "active",
                created_at: "2026-01-24T10:00:00Z",
                updated_at: "2026-01-24T10:01:00Z",
              },
              ref_kind: "pull_request",
              ref_label: "PR #123",
              publication_state: "merged",
              publication_label: "merged",
              attention_lane: "done",
              parked_delegate_count: 2,
              action_verb: "Merged",
              is_muted: false,
            },
          ],
        },
      ],
    });

    const result = await listAgentSidebarConversations({
      projectIds: ["project-1"],
      groupBy: "publication",
      publicationStates: ["merged", "closed"],
      limitPerGroup: 20,
      offsets: { merged: 0 },
      pinnedConversationIds: ["conversation-pinned"],
      priorityConversationIds: ["conversation-selected"],
      search: " merged ",
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "list_agent_sidebar_conversations",
      {
        input: {
          projectIds: ["project-1"],
          includeArchived: false,
          archivedOnly: false,
          search: "merged",
          publicationStates: ["merged", "closed"],
          groupBy: "publication",
          limitPerGroup: 20,
          offsets: { merged: 0 },
          pinnedConversationIds: ["conversation-pinned"],
          priorityConversationIds: ["conversation-selected"],
        },
      },
    );
    expect(result.groups[0]).toMatchObject({
      key: "merged",
      hasMore: false,
      rows: [
        {
          refKind: "pull-request",
          refLabel: "PR #123",
          publicationState: "merged",
          publicationLabel: "merged",
          attentionLane: "done",
          parkedDelegateCount: 2,
          actionVerb: "Merged",
          isMuted: false,
        },
      ],
    });
  });

  it("sets an agent conversation muted state", async () => {
    mockInvoke.mockResolvedValue(null);

    await expect(
      setAgentConversationMuted("conversation-1", true),
    ).resolves.toBeUndefined();

    expect(mockInvoke).toHaveBeenCalledWith("set_agent_conversation_muted", {
      input: {
        conversationId: "conversation-1",
        muted: true,
      },
    });
  });

  it("passes automation sidebar grouping through to the backend input", async () => {
    mockInvoke.mockResolvedValue({
      groups: [],
    });

    const result = await listAgentSidebarConversations({
      projectIds: ["project-1", "project-2"],
      includeArchived: true,
      archivedOnly: true,
      groupBy: "automation",
      publicationStates: ["active", "merged"],
      limitPerGroup: 8,
      offsets: { "automation-1": 16 },
      pinnedConversationIds: ["conversation-pinned"],
      priorityConversationIds: ["conversation-selected"],
      search: " release ",
      sort: "za",
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "list_agent_sidebar_conversations",
      {
        input: {
          projectIds: ["project-1", "project-2"],
          includeArchived: true,
          archivedOnly: true,
          search: "release",
          publicationStates: ["active", "merged"],
          groupBy: "automation",
          sort: "za",
          limitPerGroup: 8,
          offsets: { "automation-1": 16 },
          pinnedConversationIds: ["conversation-pinned"],
          priorityConversationIds: ["conversation-selected"],
        },
      },
    );
    expect(result.groups).toEqual([]);
  });

  it("lists agent conversation workspace publication events", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "event-1",
        conversation_id: "conversation-1",
        step: "refreshing",
        status: "started",
        summary: "Refreshing branch from base",
        classification: null,
        created_at: "2026-04-26T09:01:00Z",
      },
    ]);

    const result =
      await listAgentConversationWorkspacePublicationEvents("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "list_agent_conversation_workspace_publication_events",
      { conversationId: "conversation-1" },
    );
    expect(result[0]).toMatchObject({
      conversationId: "conversation-1",
      step: "refreshing",
      summary: "Refreshing branch from base",
    });
  });

  it("gets agent conversation workspace freshness", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "conversation-1",
      freshness_scope: "full",
      base_ref: "feature/agent-screen",
      base_display_name: "Current branch (feature/agent-screen)",
      target_ref: "origin/feature/agent-screen",
      captured_base_commit: "old-base",
      target_base_commit: "new-base",
      is_base_ahead: true,
      has_uncommitted_changes: true,
      unpublished_commit_count: 2,
      remote_refreshed: true,
      worktree_status_checked: true,
      base_status: "retargeted",
      effective_base_ref: "main",
      effective_base_display_name: "Project default (main)",
      base_block_reason: null,
      recommended_actions: ["update_from_base", "base_pr_merged"],
    });

    const result =
      await getAgentConversationWorkspaceFreshness("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_workspace_freshness",
      { conversationId: "conversation-1" },
    );
    expect(result).toMatchObject({
      conversationId: "conversation-1",
      freshnessScope: "full",
      baseRef: "feature/agent-screen",
      targetRef: "origin/feature/agent-screen",
      baseStatus: "retargeted",
      effectiveBaseRef: "main",
      effectiveBaseDisplayName: "Project default (main)",
      isBaseAhead: true,
      hasUncommittedChanges: true,
      unpublishedCommitCount: 2,
      remoteRefreshed: true,
      worktreeStatusChecked: true,
      recommendedActions: ["update_from_base", "base_pr_merged"],
    });
  });

  it("defaults absent workspace freshness recommended actions", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "conversation-1",
      freshness_scope: "full",
      base_ref: "main",
      base_display_name: "Project default (main)",
      target_ref: "origin/main",
      captured_base_commit: "base",
      target_base_commit: "base",
      is_base_ahead: false,
      has_uncommitted_changes: false,
      unpublished_commit_count: null,
      remote_refreshed: true,
      worktree_status_checked: true,
    });

    await expect(
      getAgentConversationWorkspaceFreshness("conversation-1"),
    ).resolves.toMatchObject({ recommendedActions: [] });
  });

  it("requests scoped agent conversation workspace freshness", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "conversation-1",
      freshness_scope: "local",
      base_ref: "main",
      base_display_name: "Project default (main)",
      target_ref: "ralphx/test/agent-workspace",
      captured_base_commit: "base",
      target_base_commit: "base",
      is_base_ahead: false,
      has_uncommitted_changes: false,
      unpublished_commit_count: null,
      remote_refreshed: false,
      worktree_status_checked: false,
    });

    const result = await getAgentConversationWorkspaceFreshness(
      "conversation-1",
      {
        scope: "local",
      },
    );

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_workspace_freshness",
      { conversationId: "conversation-1", freshnessScope: "local" },
    );
    expect(result).toMatchObject({
      conversationId: "conversation-1",
      freshnessScope: "local",
      remoteRefreshed: false,
      worktreeStatusChecked: false,
    });
  });

  it("precomputes an agent conversation workspace PR description", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "conversation-1",
      status: "ready",
      cache_status: "miss",
      reason: null,
    });

    const result =
      await precomputeAgentConversationWorkspacePrDescription("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "precompute_agent_conversation_workspace_pr_description",
      { conversationId: "conversation-1" },
    );
    expect(result).toEqual({
      conversationId: "conversation-1",
      status: "ready",
      cacheStatus: "miss",
      reason: null,
    });
  });

  it("commits a workspace locally with the exact review receipt and transforms its result", async () => {
    mockInvoke.mockResolvedValue({
      workspace: {
        conversation_id: "conversation-1",
        project_id: "project-1",
        mode: "edit",
        base_ref_kind: "project_default",
        base_ref: "main",
        base_display_name: "Project default (main)",
        base_commit: "base",
        branch_name: "ralphx/demo/agent-conversation-1",
        worktree_path: "/tmp/ralphx/conversation-1",
        linked_ideation_session_id: null,
        linked_plan_branch_id: null,
        publication_pr_number: null,
        publication_pr_url: null,
        publication_pr_status: null,
        publication_push_status: null,
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:01:00Z",
      },
      outcome: "committed_local",
      branch_name: "ralphx/demo/agent-conversation-1",
      previous_head_sha: "abcdef0",
      commit_sha: "1234567890abcdef",
      had_changes: true,
      attempt_token: "attempt-7",
    });

    const result = await commitAgentConversationWorkspaceLocally("conversation-1", {
      expectedHeadSha: "abcdef0",
      reviewArtifactId: "artifact-1",
      reviewArtifactVersion: 3,
      reviewedHeadSha: "abcdef0",
      reviewedDiffFingerprint: "fingerprint-1",
      attemptToken: "attempt-7",
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "commit_agent_conversation_workspace_locally",
      {
        input: {
          conversationId: "conversation-1",
          expectedHeadSha: "abcdef0",
          reviewArtifactId: "artifact-1",
          reviewArtifactVersion: 3,
          reviewedHeadSha: "abcdef0",
          reviewedDiffFingerprint: "fingerprint-1",
          attemptToken: "attempt-7",
        },
      },
    );
    expect(result).toMatchObject({
      outcome: "committed_local",
      commitSha: "1234567890abcdef",
      attemptToken: "attempt-7",
      workspace: { conversationId: "conversation-1" },
    });
  });

  it("updates an agent conversation workspace from its base branch", async () => {
    mockInvoke.mockResolvedValue({
      workspace: {
        conversation_id: "conversation-1",
        project_id: "project-1",
        mode: "edit",
        base_ref_kind: "local_branch",
        base_ref: "feature/agent-screen",
        base_display_name: "PR #42: Add PR picker",
        base_commit: "new-base",
        branch_name: "ralphx/demo/agent-conversation-1",
        worktree_path: "/tmp/ralphx/conversation-1",
        linked_ideation_session_id: null,
        linked_plan_branch_id: null,
        publication_pr_number: 78,
        publication_pr_url: "https://github.com/mock/project/pull/78",
        publication_pr_status: "open",
        publication_push_status: "refreshed",
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:01:00Z",
      },
      updated: true,
      repair_started: true,
      target_ref: "origin/feature/agent-screen",
      base_commit: "new-base",
    });

    const result =
      await updateAgentConversationWorkspaceFromBase("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "update_agent_conversation_workspace_from_base",
      { conversationId: "conversation-1" },
    );
    expect(result).toMatchObject({
      updated: true,
      repairStarted: true,
      targetRef: "origin/feature/agent-screen",
      baseCommit: "new-base",
      workspace: {
        conversationId: "conversation-1",
        baseCommit: "new-base",
        publicationPushStatus: "refreshed",
      },
    });
  });

  it("updates an agent conversation workspace from a PR-backed base branch", async () => {
    mockInvoke.mockResolvedValue({
      workspace: {
        conversation_id: "conversation-1",
        project_id: "project-1",
        mode: "edit",
        base_ref_kind: "local_branch",
        base_ref: "feature/pr-base",
        base_display_name: "PR #42: Add PR base",
        base_commit: "new-base",
        branch_name: "ralphx/demo/agent-conversation-1",
        worktree_path: "/tmp/ralphx/conversation-1",
        linked_ideation_session_id: null,
        linked_plan_branch_id: null,
        source_pull_request: {
          number: 42,
          url: "https://github.com/mock/project/pull/42",
          title: "Add PR base",
          head_ref_name: "feature/pr-base",
          base_ref_name: "main",
          head_ref_oid: "pr-head-sha",
        },
        publication_pr_number: 78,
        publication_pr_url: "https://github.com/mock/project/pull/78",
        publication_pr_status: "open",
        publication_push_status: "refreshed",
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:01:00Z",
      },
      updated: true,
      target_ref: "origin/feature/pr-base",
      base_commit: "new-base",
    });

    const result = await updateAgentConversationWorkspaceFromBase(
      "conversation-1",
      {
        kind: "local_branch",
        ref: "feature/pr-base",
        displayName: "PR #42: Add PR base",
        sourcePullRequest: {
          number: 42,
          url: "https://github.com/mock/project/pull/42",
          title: "Add PR base",
          headRefName: "feature/pr-base",
          baseRefName: "main",
          headRefOid: "pr-head-sha",
        },
      },
    );

    expect(mockInvoke).toHaveBeenCalledWith(
      "update_agent_conversation_workspace_from_base",
      {
        conversationId: "conversation-1",
        baseRefKind: "local_branch",
        baseRef: "feature/pr-base",
        baseDisplayName: "PR #42: Add PR base",
        baseSourcePullRequest: {
          number: 42,
          url: "https://github.com/mock/project/pull/42",
          title: "Add PR base",
          headRefName: "feature/pr-base",
          baseRefName: "main",
          headRefOid: "pr-head-sha",
        },
      },
    );
    expect(result.workspace.sourcePullRequest).toMatchObject({
      number: 42,
      headRefName: "feature/pr-base",
    });
  });

  it("updates PR supervision preferences for an agent conversation workspace", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "conversation-1",
      project_id: "project-1",
      mode: "edit",
      base_ref_kind: "project_default",
      base_ref: "main",
      base_display_name: "Project default (main)",
      base_commit: "base-sha",
      branch_name: "ralphx/demo/agent-conversation-1",
      worktree_path: "/tmp/ralphx/conversation-1",
      linked_ideation_session_id: null,
      linked_plan_branch_id: null,
      publication_pr_number: 78,
      publication_pr_url: "https://github.com/mock/project/pull/78",
      publication_pr_status: "open",
      publication_push_status: "pushed",
      auto_publish_enabled: true,
      auto_publish_paused_pr_autofix_enabled: null,
      auto_publish_paused_pr_auto_merge_desired: null,
      pr_autofix_enabled: true,
      pr_auto_merge_desired: true,
      pr_auto_merge_method: "squash",
      pr_auto_merge_current: null,
      pr_supervision_status: "monitoring",
      pr_supervision_summary: "RalphX PR supervision is enabled.",
      pr_supervision_updated_at: "2026-05-17T10:00:00Z",
      status: "active",
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:01:00Z",
    });

    const result = await setAgentConversationWorkspacePrSupervision(
      "conversation-1",
      {
        autoFixEnabled: true,
        autoMergeDesired: true,
      },
    );

    expect(mockInvoke).toHaveBeenCalledWith(
      "set_agent_conversation_workspace_pr_supervision",
      {
        conversationId: "conversation-1",
        input: {
          autoFixEnabled: true,
          autoMergeDesired: true,
        },
      },
    );
    expect(result).toMatchObject({
      conversationId: "conversation-1",
      prAutofixEnabled: true,
      prAutoMergeDesired: true,
      prAutoMergeMethod: "squash",
      prSupervisionStatus: "monitoring",
    });
  });

  it("sets agent conversation workspace auto publish", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "conversation-1",
      project_id: "project-1",
      mode: "edit",
      base_ref_kind: "project_default",
      base_ref: "main",
      base_display_name: "Project default (main)",
      base_commit: null,
      branch_name: "ralphx/demo/agent-conversation-1",
      worktree_path: "/tmp/ralphx/conversation-1",
      linked_ideation_session_id: null,
      linked_plan_branch_id: null,
      publication_pr_number: 42,
      publication_pr_url: "https://github.com/mock/project/pull/42",
      publication_pr_status: "open",
      publication_push_status: "pushed",
      auto_publish_enabled: false,
      auto_publish_paused_pr_autofix_enabled: true,
      auto_publish_paused_pr_auto_merge_desired: false,
      pr_autofix_enabled: false,
      pr_auto_merge_desired: false,
      pr_auto_merge_method: "squash",
      pr_auto_merge_current: null,
      pr_supervision_status: "paused",
      pr_supervision_summary: "Auto Publish is paused.",
      pr_supervision_updated_at: "2026-05-17T10:00:00Z",
      status: "active",
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:01:00Z",
    });

    const result = await setAgentConversationWorkspaceAutoPublish(
      "conversation-1",
      { autoPublishEnabled: false },
    );

    expect(mockInvoke).toHaveBeenCalledWith(
      "set_agent_conversation_workspace_auto_publish",
      {
        conversationId: "conversation-1",
        input: {
          autoPublishEnabled: false,
        },
      },
    );
    expect(result).toMatchObject({
      conversationId: "conversation-1",
      autoPublishEnabled: false,
      autoPublishPausedPrAutofixEnabled: true,
      prAutofixEnabled: false,
      prSupervisionStatus: "paused",
    });
  });

  it("accepts the camelCase conversation stats payload returned by Tauri", async () => {
    mockInvoke.mockResolvedValue({
      conversationId: "c1",
      contextType: "project",
      contextId: "p1",
      providerHarness: "codex",
      upstreamProvider: "openai",
      providerProfile: null,
      messageUsageTotals: {
        inputTokens: 2535967,
        outputTokens: 13593,
        cacheCreationTokens: 0,
        cacheReadTokens: 2434048,
        processedTokens: 2549560,
        estimatedUsd: null,
      },
      runUsageTotals: {
        inputTokens: 2535967,
        outputTokens: 13593,
        cacheCreationTokens: 0,
        cacheReadTokens: 2434048,
        processedTokens: 2549560,
        estimatedUsd: null,
      },
      effectiveUsageTotals: {
        inputTokens: 2535967,
        outputTokens: 13593,
        cacheCreationTokens: 0,
        cacheReadTokens: 2434048,
        processedTokens: 2549560,
        estimatedUsd: null,
      },
      usageCoverage: {
        providerMessageCount: 1,
        providerMessagesWithUsage: 1,
        runCount: 1,
        runsWithUsage: 1,
        effectiveRunConversationCount: 0,
        effectiveMessageConversationCount: 1,
        legacyEstimatedSampleCount: 0,
        fallbackEstimatedSampleCount: 0,
        uncountedSampleCount: 0,
        effectiveTotalsSource: "messages",
      },
      attributionCoverage: {
        providerMessageCount: 1,
        providerMessagesWithAttribution: 1,
        runCount: 1,
        runsWithAttribution: 1,
      },
      byHarness: [
        {
          key: "codex",
          count: 1,
          usage: {
            inputTokens: 2535967,
            outputTokens: 13593,
            cacheCreationTokens: 0,
            cacheReadTokens: 2434048,
            processedTokens: 2549560,
            estimatedUsd: null,
          },
        },
      ],
      byUpstreamProvider: [],
      byModel: [
        {
          key: "gpt-5.4",
          count: 1,
          usage: {
            inputTokens: 2535967,
            outputTokens: 13593,
            cacheCreationTokens: 0,
            cacheReadTokens: 2434048,
            processedTokens: 2549560,
            estimatedUsd: null,
          },
        },
      ],
      byEffort: [
        {
          key: "medium",
          count: 1,
          usage: {
            inputTokens: 2535967,
            outputTokens: 13593,
            cacheCreationTokens: 0,
            cacheReadTokens: 2434048,
            processedTokens: 2549560,
            estimatedUsd: null,
          },
        },
      ],
    });

    const result = await getConversationStats("c1");

    expect(result).toMatchObject({
      conversationId: "c1",
      usageCoverage: {
        effectiveTotalsSource: "messages",
        providerMessagesWithUsage: 1,
      },
      effectiveUsageTotals: {
        inputTokens: 2535967,
        outputTokens: 13593,
        cacheReadTokens: 2434048,
        processedTokens: 2549560,
      },
      byModel: [{ key: "gpt-5.4" }],
      byEffort: [{ key: "medium" }],
    });
  });

  it("starts chat-mode agent conversations with a selected workspace base", async () => {
    mockInvoke.mockResolvedValue({
      conversation: {
        id: "conversation-chat",
        context_type: "project",
        context_id: "project-1",
        claude_session_id: null,
        provider_session_id: null,
        provider_harness: null,
        agent_mode: "chat",
        title: "Chat",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
        archived_at: null,
      },
      workspace: {
        conversation_id: "conversation-chat",
        project_id: "project-1",
        mode: "chat",
        base_ref_kind: "local_branch",
        base_ref: "feature/agent-screen",
        base_display_name: "PR #42: Add PR picker",
        base_commit: null,
        branch_name: "ralphx/demo/agent-conversation-chat",
        worktree_path: "/tmp/ralphx/conversation-chat",
        linked_ideation_session_id: null,
        linked_plan_branch_id: null,
        source_pull_request: {
          number: 42,
          url: "https://github.com/owner/repo/pull/42",
          title: "Add PR picker",
          head_ref_name: "feature/agent-screen",
          base_ref_name: "main",
          head_ref_oid: "abc123",
        },
        publication_pr_number: null,
        publication_pr_url: null,
        publication_pr_status: null,
        publication_push_status: null,
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
      },
      send_result: {
        conversation_id: "conversation-chat",
        agent_run_id: "run-chat",
        is_new_conversation: true,
      },
    });

    const result = await startAgentConversation({
      projectId: "project-1",
      content: "What changed?",
      providerHarness: "codex",
      modelId: "gpt-5.5",
      logicalEffort: "xhigh",
      codexFastMode: true,
      mode: "chat",
      base: {
        kind: "local_branch",
        ref: "feature/agent-screen",
        displayName: "PR #42: Add PR picker",
        sourcePullRequest: {
          number: 42,
          url: "https://github.com/owner/repo/pull/42",
          title: "Add PR picker",
          headRefName: "feature/agent-screen",
          baseRefName: "main",
          headRefOid: "abc123",
        },
      },
    });

    expect(mockInvoke).toHaveBeenCalledWith("start_agent_conversation", {
      input: {
        projectId: "project-1",
        content: "What changed?",
        providerHarness: "codex",
        modelOverride: "gpt-5.5",
        logicalEffort: "xhigh",
        codexFastMode: true,
        mode: "chat",
        baseRefKind: "local_branch",
        baseRef: "feature/agent-screen",
        baseDisplayName: "PR #42: Add PR picker",
        baseSourcePullRequest: {
          number: 42,
          url: "https://github.com/owner/repo/pull/42",
          title: "Add PR picker",
          headRefName: "feature/agent-screen",
          baseRefName: "main",
          headRefOid: "abc123",
        },
      },
    });
    expect(result.conversation.agentMode).toBe("chat");
    expect(result.workspace).toMatchObject({
      mode: "chat",
      baseRefKind: "local_branch",
      baseRef: "feature/agent-screen",
      sourcePullRequest: expect.objectContaining({
        number: 42,
        headRefName: "feature/agent-screen",
        baseRefName: "main",
        headRefOid: "abc123",
      }),
    });
  });

  it("switches an existing agent conversation into plan mode", async () => {
    mockInvoke.mockResolvedValue({
      conversation: {
        id: "conversation-chat",
        context_type: "project",
        context_id: "project-1",
        claude_session_id: null,
        provider_session_id: null,
        provider_harness: null,
        agent_mode: "plan",
        title: "Chat",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:02:00Z",
        archived_at: null,
      },
      workspace: {
        conversation_id: "conversation-chat",
        project_id: "project-1",
        mode: "plan",
        base_ref_kind: "project_default",
        base_ref: "main",
        base_display_name: "Project default (main)",
        base_commit: null,
        branch_name: "ralphx/demo/agent-conversation-chat",
        worktree_path: "/tmp/ralphx/conversation-chat",
        linked_ideation_session_id: null,
        linked_plan_branch_id: null,
        publication_pr_number: null,
        publication_pr_url: null,
        publication_pr_status: null,
        publication_push_status: null,
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:02:00Z",
      },
    });

    const result = await switchAgentConversationMode({
      conversationId: "conversation-chat",
      mode: "plan",
    });

    expect(mockInvoke).toHaveBeenCalledWith("switch_agent_conversation_mode", {
      input: {
        conversationId: "conversation-chat",
        mode: "plan",
      },
    });
    expect(result.conversation.agentMode).toBe("plan");
    expect(result.workspace?.mode).toBe("plan");
  });

  it("updates an existing agent conversation coordination mode", async () => {
    mockInvoke.mockResolvedValue({
      id: "conversation-chat",
      context_type: "project",
      context_id: "project-1",
      claude_session_id: null,
      provider_session_id: null,
      provider_harness: "codex",
      agent_mode: "edit",
      coordination_mode: "rx_native_team",
      title: "Chat",
      message_count: 1,
      last_message_at: null,
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:02:00Z",
      archived_at: null,
    });

    const result = await updateAgentConversationCoordinationMode({
      conversationId: "conversation-chat",
      coordinationMode: "rx_native_team",
      modelOverride: "gpt-5.6-sol",
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "update_agent_conversation_coordination_mode",
      {
        input: {
          conversationId: "conversation-chat",
          coordinationMode: "rx_native_team",
          modelOverride: "gpt-5.6-sol",
        },
      },
    );
    expect(result.coordinationMode).toBe("rx_native_team");
  });

  it("sends source pull request metadata when switching mode with a PR base", async () => {
    mockInvoke.mockResolvedValue({
      conversation: {
        id: "conversation-chat",
        context_type: "project",
        context_id: "project-1",
        claude_session_id: null,
        provider_session_id: null,
        provider_harness: null,
        agent_mode: "edit",
        title: "Chat",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:02:00Z",
        archived_at: null,
      },
      workspace: {
        conversation_id: "conversation-chat",
        project_id: "project-1",
        mode: "edit",
        base_ref_kind: "local_branch",
        base_ref: "feature/source-pr",
        base_display_name: "PR #42: Source PR",
        base_commit: null,
        branch_name: "ralphx/demo/agent-conversation-chat",
        worktree_path: "/tmp/ralphx/conversation-chat",
        linked_ideation_session_id: null,
        linked_plan_branch_id: null,
        source_pull_request: {
          number: 42,
          url: "https://github.com/owner/repo/pull/42",
          title: "Source PR",
          head_ref_name: "feature/source-pr",
          base_ref_name: "main",
          head_ref_oid: "abc123",
        },
        publication_pr_number: null,
        publication_pr_url: null,
        publication_pr_status: null,
        publication_push_status: null,
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:02:00Z",
      },
    });

    const result = await switchAgentConversationMode({
      conversationId: "conversation-chat",
      mode: "edit",
      base: {
        kind: "local_branch",
        ref: "feature/source-pr",
        displayName: "PR #42: Source PR",
        sourcePullRequest: {
          number: 42,
          url: "https://github.com/owner/repo/pull/42",
          title: "Source PR",
          headRefName: "feature/source-pr",
          baseRefName: "main",
          headRefOid: "abc123",
        },
      },
    });

    expect(mockInvoke).toHaveBeenCalledWith("switch_agent_conversation_mode", {
      input: {
        conversationId: "conversation-chat",
        mode: "edit",
        baseRefKind: "local_branch",
        baseRef: "feature/source-pr",
        baseDisplayName: "PR #42: Source PR",
        baseSourcePullRequest: {
          number: 42,
          url: "https://github.com/owner/repo/pull/42",
          title: "Source PR",
          headRefName: "feature/source-pr",
          baseRefName: "main",
          headRefOid: "abc123",
        },
      },
    });
    expect(result.workspace?.sourcePullRequest?.number).toBe(42);
  });

  it("copies an existing plan into an Agent conversation and transforms the seed result", async () => {
    mockInvoke.mockResolvedValue({
      conversation: planSeedConversationResponse(),
      workspace: planSeedWorkspaceResponse(),
      session_id: "session-plan",
      artifact: planSeedArtifactResponse(),
      blueprint_artifact: {
        ...planSeedArtifactResponse(),
        id: "artifact-blueprint",
        name: "Implementation Blueprint",
        content: "# Blueprint",
        derived_from: ["source-blueprint"],
      },
    });

    const result = await copyAgentConversationPlan({
      conversationId: "conversation-plan",
      sourceSessionId: "source-session",
      sourceArtifactId: "source-artifact",
      sourceVersion: 2,
    });

    expect(mockInvoke).toHaveBeenCalledWith("copy_agent_conversation_plan", {
      input: {
        conversationId: "conversation-plan",
        sourceSessionId: "source-session",
        sourceArtifactId: "source-artifact",
        sourceVersion: 2,
      },
    });
    expect(result.conversation.agentMode).toBe("plan");
    expect(result.workspace.linkedIdeationSessionId).toBe("session-plan");
    expect(result.sessionId).toBe("session-plan");
    expect(result.artifact).toMatchObject({
      id: "artifact-plan",
      name: "Imported plan",
      content: { type: "inline", text: "# Imported plan" },
      planApproval: { status: "draft" },
    });
    expect(result.blueprintArtifact).toMatchObject({
      id: "artifact-blueprint",
      name: "Implementation Blueprint",
      content: { type: "inline", text: "# Blueprint" },
      derivedFrom: ["source-blueprint"],
    });
  });

  it("imports markdown into an Agent conversation plan and transforms the seed result", async () => {
    mockInvoke.mockResolvedValue({
      conversation: planSeedConversationResponse(),
      workspace: planSeedWorkspaceResponse(),
      session_id: "session-plan",
      artifact: {
        ...planSeedArtifactResponse(),
        id: "artifact-imported",
        name: "Dropped plan",
        content: "# Dropped plan",
        derived_from: [],
      },
    });

    const result = await importAgentConversationPlan({
      conversationId: "conversation-plan",
      title: "Dropped plan",
      content: "# Dropped plan",
    });

    expect(mockInvoke).toHaveBeenCalledWith("import_agent_conversation_plan", {
      input: {
        conversationId: "conversation-plan",
        title: "Dropped plan",
        content: "# Dropped plan",
      },
    });
    expect(result.sessionId).toBe("session-plan");
    expect(result.artifact).toMatchObject({
      id: "artifact-imported",
      content: { type: "inline", text: "# Dropped plan" },
      derivedFrom: [],
    });
    expect(result.blueprintArtifact).toBeNull();
  });

  it("forks an agent conversation and transforms child workspace metadata", async () => {
    const conversation = {
      id: "conversation-child",
      context_type: "project",
      context_id: "project-1",
      claude_session_id: null,
      provider_session_id: "thread-child",
      provider_harness: "codex",
      logical_model: "gpt-5.5",
      effective_model_id: "gpt-5.5",
      logical_effort: "high",
      effective_effort: "high",
      agent_mode: "edit",
      parent_conversation_id: "conversation-parent",
      title: "[Fork] Parent chat",
      message_count: 2,
      last_message_at: null,
      created_at: "2026-01-24T10:00:00Z",
      updated_at: "2026-01-24T10:02:00Z",
      archived_at: null,
    };
    mockInvoke.mockResolvedValue({
      parent_conversation: {
        ...conversation,
        id: "conversation-parent",
        provider_session_id: "thread-parent",
        parent_conversation_id: null,
        title: "Parent chat",
      },
      conversation,
      workspace: {
        conversation_id: "conversation-child",
        project_id: "project-1",
        mode: "edit",
        base_ref_kind: "project_default",
        base_ref: "main",
        base_display_name: "Project default (main)",
        base_commit: "base-sha",
        branch_name: "ralphx/demo/conversation-child",
        worktree_path: "/tmp/ralphx/conversation-child",
        linked_ideation_session_id: null,
        linked_plan_branch_id: null,
        publication_pr_number: null,
        publication_pr_url: null,
        publication_pr_status: null,
        publication_push_status: null,
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:02:00Z",
      },
      provider_session_forked: true,
      copied_message_count: 2,
      copied_timeline_item_count: 3,
    });

    const result = await forkAgentConversation("conversation-parent");

    expect(mockInvoke).toHaveBeenCalledWith("fork_agent_conversation", {
      input: {
        conversationId: "conversation-parent",
      },
    });
    expect(result.parentConversation.id).toBe("conversation-parent");
    expect(result.conversation).toMatchObject({
      id: "conversation-child",
      providerHarness: "codex",
      providerSessionId: "thread-child",
      logicalModel: "gpt-5.5",
      logicalEffort: "high",
      parentConversationId: "conversation-parent",
    });
    expect(result.workspace).toMatchObject({
      conversationId: "conversation-child",
      baseCommit: "base-sha",
      branchName: "ralphx/demo/conversation-child",
    });
    expect(result.providerSessionForked).toBe(true);
    expect(result.copiedTimelineItemCount).toBe(3);
  });

  it("uses the web-mode chat mock for child session status when available", async () => {
    window.__mockChatApi = {
      reset: vi.fn(),
      seedScenario: vi.fn(),
      seedConversation: vi.fn(),
      replaceMessages: vi.fn(),
      listScenarios: vi.fn().mockReturnValue([]),
      listConversations: vi.fn(),
      listConversationsPage: vi.fn(),
      listAgentSidebarConversations: vi.fn(),
      getConversation: vi.fn(),
      getConversationSummary: vi.fn(),
      getConversationTimelinePage: vi.fn(),
      getConversationStats: vi.fn(),
      seedAgentConversationWorkspace: vi.fn(),
      getChildSessionStatus: vi.fn().mockResolvedValue({
        session_id: "child-1",
        title: "Mock child session",
        agent_state: { estimated_status: "likely_generating" },
        recent_messages: [],
        lastEffectiveModel: "gpt-5.4-mini",
      }),
      setChildSessionStatusOverride: vi.fn(),
      clearChildSessionStatusOverrides: vi.fn(),
    };

    const result = await getChildSessionStatus("child-1");

    expect(window.__mockChatApi.getChildSessionStatus).toHaveBeenCalledWith(
      "child-1",
    );
    expect(result).toMatchObject({
      session_id: "child-1",
      title: "Mock child session",
      lastEffectiveModel: "gpt-5.4-mini",
    });
  });

  it("sends unified agent message", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "c1",
      agent_run_id: "r1",
      is_new_conversation: true,
      queued_as_pending: true,
    });

    const result = await sendAgentMessage("project", "p1", "Hello");

    expect(mockInvoke).toHaveBeenCalledWith("send_agent_message", {
      input: { contextType: "project", contextId: "p1", content: "Hello" },
    });
    expect(result).toEqual({
      conversationId: "c1",
      agentRunId: "r1",
      isNewConversation: true,
      wasQueued: false,
      queuedAsPending: true,
      queuedMessageId: undefined,
    });
  });

  it("sends unified agent message with provider and model overrides", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "c1",
      agent_run_id: "r1",
      is_new_conversation: true,
    });

    await sendAgentMessage("project", "p1", "Hello", undefined, {
      conversationId: "c1",
      providerHarness: "codex",
      modelId: "gpt-5.4",
      logicalEffort: "high",
      codexFastMode: true,
    });

    expect(mockInvoke).toHaveBeenCalledWith("send_agent_message", {
      input: {
        contextType: "project",
        contextId: "p1",
        content: "Hello",
        conversationId: "c1",
        providerHarness: "codex",
        modelOverride: "gpt-5.4",
        logicalEffort: "high",
        codexFastMode: true,
      },
    });
  });

  it("sends unified agent message with a normalized Team member target", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "c1",
      agent_run_id: "r1",
      is_new_conversation: false,
    });

    await sendAgentMessage(
      "project",
      "p1",
      "Update member",
      undefined,
      {
        conversationId: "c1",
        teamIntent: { coordinationMode: "rx_native_team" },
        teamMessageTarget: {
          kind: "member",
          memberName: "worker one",
        },
      },
    );

    expect(mockInvoke).toHaveBeenCalledWith("send_agent_message", {
      input: {
        contextType: "project",
        contextId: "p1",
        content: "Update member",
        conversationId: "c1",
        teamIntent: { coordinationMode: "rx_native_team" },
        teamMessageTarget: {
          kind: "member",
          memberName: "worker one",
        },
      },
    });
  });

  it("sends the provider-neutral workflow capability intent", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "c1",
      agent_run_id: "r1",
      is_new_conversation: false,
    });

    await sendAgentMessage("project", "p1", "Build a workflow", undefined, {
      conversationId: "c1",
      capabilityIntent: { coordinationMode: "rx_native_workflow" },
    });

    expect(mockInvoke).toHaveBeenCalledWith("send_agent_message", {
      input: {
        contextType: "project",
        contextId: "p1",
        content: "Build a workflow",
        conversationId: "c1",
        capabilityIntent: { coordinationMode: "rx_native_workflow" },
      },
    });
  });

  it("sends unified agent message with hidden user-message handoff", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "c1",
      agent_run_id: "r1",
      is_new_conversation: false,
    });

    await sendAgentMessage(
      "project",
      "p1",
      "Run internally",
      undefined,
      {
        conversationId: "c1",
        suppressUserMessage: true,
      },
    );

    expect(mockInvoke).toHaveBeenCalledWith("send_agent_message", {
      input: {
        contextType: "project",
        contextId: "p1",
        content: "Run internally",
        conversationId: "c1",
        suppressUserMessage: true,
      },
    });
  });

  it("sends unified agent message with structured composer references", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "c1",
      agent_run_id: "r1",
      is_new_conversation: false,
    });

    await sendAgentMessage(
      "project",
      "p1",
      "Read @src/main.ts",
      undefined,
      {
        composerProjectReferences: [{ path: "src/main.ts", kind: "file" }],
        composerIntegrationReferences: [
          { provider: "atlassian", kind: "jira", id: "RX-42", key: "RX-42" },
        ],
        composerArtifactReferences: [
          { kind: "plan", artifactId: "artifact-1", sessionId: "session-1" },
        ],
        composerSelectionSnapshot: {
          sourceType: "artifact",
          sourceKind: "plan",
          sourceId: "artifact-version-2",
          sourceTitle: "Implementation Plan",
          artifactVersion: 2,
          startLine: 10,
          endLine: 11,
          content: "first\nsecond",
        },
      },
    );

    expect(mockInvoke).toHaveBeenCalledWith("send_agent_message", {
      input: {
        contextType: "project",
        contextId: "p1",
        content: "Read @src/main.ts",
        composerProjectReferences: [{ path: "src/main.ts", kind: "file" }],
        composerIntegrationReferences: [
          { provider: "atlassian", kind: "jira", id: "RX-42", key: "RX-42" },
        ],
        composerArtifactReferences: [
          { kind: "plan", artifactId: "artifact-1", sessionId: "session-1" },
        ],
        composerSelectionSnapshot: {
          sourceType: "artifact",
          sourceKind: "plan",
          sourceId: "artifact-version-2",
          sourceTitle: "Implementation Plan",
          artifactVersion: 2,
          startLine: 10,
          endLine: 11,
          content: "first\nsecond",
        },
      },
    });
  });

  it("sends an approved linked-plan policy without composer artifact references", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "c1",
      agent_run_id: "r1",
      is_new_conversation: false,
    });

    await sendAgentMessage("project", "p1", "Implement the plan", undefined, {
      conversationId: "c1",
      requireApprovedLinkedPlan: true,
      expectedLinkedPlanFingerprint: "activation-fingerprint-1",
      suppressUserMessage: true,
    });

    expect(mockInvoke).toHaveBeenCalledWith("send_agent_message", {
      input: {
        contextType: "project",
        contextId: "p1",
        content: "Implement the plan",
        conversationId: "c1",
        requireApprovedLinkedPlan: true,
        expectedLinkedPlanFingerprint: "activation-fingerprint-1",
        suppressUserMessage: true,
      },
    });
    expect(mockInvoke.mock.calls[0]?.[1]).not.toHaveProperty(
      "input.composerArtifactReferences",
    );
  });

  it("lists queued messages", async () => {
    mockInvoke.mockResolvedValueOnce([
      {
        id: "q1",
        content: "queued",
        created_at: "2026-01-24T10:00:00Z",
        is_editing: false,
        composer_selection_snapshot: {
          sourceType: "ticket",
          sourceKind: "clickup",
          sourceId: "task-1",
          sourceKey: "CU-1",
          provider: "clickup",
          startLine: 3,
          endLine: 3,
          content: "selected line",
        },
        attachment_ids: ["att-1"],
      },
    ]);

    const list = await getQueuedAgentMessages("project", "p1");

    expect(list).toHaveLength(1);
    expect(list[0].attachmentIds).toEqual(["att-1"]);
    expect(list[0].composerSelectionSnapshot?.sourceKey).toBe("CU-1");
  });

  it("deletes queued message", async () => {
    mockInvoke.mockResolvedValue(true);
    const result = await deleteQueuedAgentMessage("project", "p1", "q1");
    expect(result).toBe(true);
  });

  it("sends queued message immediately", async () => {
    mockInvoke.mockResolvedValue({
      conversation_id: "conv-1",
      agent_run_id: "run-1",
      is_new_conversation: false,
      was_queued: false,
      queued_as_pending: false,
      queued_message_id: null,
    });

    const result = await sendQueuedAgentMessageNow("project", "conv-1", "q1");

    expect(result).toMatchObject({
      conversationId: "conv-1",
      agentRunId: "run-1",
      wasQueued: false,
    });
    expect(mockInvoke).toHaveBeenCalledWith("send_queued_agent_message_now", {
      contextType: "project",
      contextId: "conv-1",
      messageId: "q1",
    });
  });

  it("checks service and running state and stops agent", async () => {
    mockInvoke
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(false);

    expect(await isChatServiceAvailable()).toBe(true);
    expect(await isAgentRunning("project", "p1")).toBe(true);
    expect(await stopAgent("project", "p1")).toBe(false);
  });

  it("bulk-checks running states", async () => {
    mockInvoke.mockResolvedValueOnce({
      c1: { is_running: true, agent_status: "generating" },
      c2: { is_running: false, agent_status: "idle" },
    });

    await expect(
      getAgentRunningStates("project", ["c1", "c2"]),
    ).resolves.toEqual({
      c1: { isRunning: true, agentStatus: "generating" },
      c2: { isRunning: false, agentStatus: "idle" },
    });
    expect(mockInvoke).toHaveBeenCalledWith("get_agent_running_states", {
      contextType: "project",
      contextIds: ["c1", "c2"],
    });
  });

  it("normalizes legacy boolean bulk running states", async () => {
    mockInvoke.mockResolvedValueOnce({
      c1: true,
      c2: false,
    });

    await expect(
      getAgentRunningStates("project", ["c1", "c2"]),
    ).resolves.toEqual({
      c1: { isRunning: true, agentStatus: "generating" },
      c2: { isRunning: false, agentStatus: "idle" },
    });
  });

  it("loads conversation runtime statuses", async () => {
    mockInvoke.mockResolvedValueOnce({
      c1: {
        conversationId: "c1",
        isRunning: true,
        agentStatus: "generating",
        primarySource: "task_execution",
        summaryLabel: "Executing",
        items: [
          {
            source: "task_execution",
            contextType: "task_execution",
            contextId: "task-1",
            label: "Executing",
            title: "Runtime task",
            agentStatus: "generating",
            taskId: "task-1",
            internalStatus: "executing",
            runningProcess: {
              task_id: "task-1",
              title: "Runtime task",
              internal_status: "executing",
              step_progress: null,
              elapsed_seconds: 12,
              trigger_origin: null,
              task_branch: "ralphx/project/task-1",
            },
            ideationSession: null,
            parentSessionId: null,
            childSessionId: null,
            conversationId: null,
          },
        ],
      },
    });

    await expect(getAgentConversationRuntimeStatuses(["c1"])).resolves.toEqual({
      c1: {
        conversationId: "c1",
        isRunning: true,
        agentStatus: "generating",
        primarySource: "task_execution",
        summaryLabel: "Executing",
        items: [
          {
            source: "task_execution",
            contextType: "task_execution",
            contextId: "task-1",
            label: "Executing",
            title: "Runtime task",
            agentStatus: "generating",
            taskId: "task-1",
            internalStatus: "executing",
            runningProcess: {
              taskId: "task-1",
              title: "Runtime task",
              internalStatus: "executing",
              stepProgress: null,
              elapsedSeconds: 12,
              triggerOrigin: null,
              taskBranch: "ralphx/project/task-1",
            },
            ideationSession: null,
            parentSessionId: null,
            childSessionId: null,
            conversationId: null,
          },
        ],
      },
    });
    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_runtime_statuses",
      { conversationIds: ["c1"] },
    );
  });

  it("loads the durable conversation runtime index", async () => {
    mockInvoke.mockResolvedValueOnce({
      conversationId: "c1",
      rows: [
        {
          id: "workspace:c1",
          group: "main",
          kind: "workspace",
          lifecycle: "running",
          statusLabel: "Running",
          title: "Workspace chat",
          mode: "agent",
          orderIndex: 0,
          orderStartedAt: "2026-07-06T10:00:00Z",
          completedAt: null,
          conversationId: "c1",
          contextType: "project",
          contextId: "c1",
          taskId: null,
          agentRunId: "run-1",
          parentSessionId: null,
          childSessionId: null,
          providerHarness: "codex",
          providerSessionId: "provider-session-1",
          errorMessage: null,
        },
      ],
    });

    await expect(getAgentConversationRuntimeIndex("c1")).resolves.toEqual({
      conversationId: "c1",
      rows: [
        {
          id: "workspace:c1",
          group: "main",
          kind: "workspace",
          lifecycle: "running",
          statusLabel: "Running",
          title: "Workspace chat",
          mode: "agent",
          orderIndex: 0,
          orderStartedAt: "2026-07-06T10:00:00Z",
          completedAt: null,
          conversationId: "c1",
          contextType: "project",
          contextId: "c1",
          taskId: null,
          agentRunId: "run-1",
          parentSessionId: null,
          childSessionId: null,
          providerHarness: "codex",
          providerSessionId: "provider-session-1",
          errorMessage: null,
        },
      ],
    });
    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_runtime_index",
      { conversationId: "c1" },
    );
  });

  it("exports chatApi namespace", () => {
    expect(chatApi.sendAgentMessage).toBe(sendAgentMessage);
    expect(chatApi.listConversations).toBe(listConversations);
    expect(chatApi.listAgentConversationWorkspacesByProject).toBe(
      listAgentConversationWorkspacesByProject,
    );
    expect(chatApi.listAgentSidebarConversations).toBe(
      listAgentSidebarConversations,
    );
    expect(chatApi.listAgentConversationWorkspacePublicationEvents).toBe(
      listAgentConversationWorkspacePublicationEvents,
    );
    expect(chatApi.getAgentConversationRuntimeStatuses).toBe(
      getAgentConversationRuntimeStatuses,
    );
    expect(chatApi.getAgentConversationRuntimeIndex).toBe(
      getAgentConversationRuntimeIndex,
    );
    expect(chatApi.precomputeAgentConversationWorkspacePrDescription).toBe(
      precomputeAgentConversationWorkspacePrDescription,
    );
    expect(chatApi.setAgentConversationWorkspacePrSupervision).toBe(
      setAgentConversationWorkspacePrSupervision,
    );
    expect(chatApi.setAgentConversationWorkspaceAutoPublish).toBe(
      setAgentConversationWorkspaceAutoPublish,
    );
    expect(chatApi.getAgentWorkspacePrReviewContext).toBe(
      getAgentWorkspacePrReviewContext,
    );
    expect(chatApi.getAgentWorkspaceReviewContext).toBe(
      getAgentWorkspaceReviewContext,
    );
    expect(chatApi.getAgentWorkspaceReviewStartPreview).toBe(
      getAgentWorkspaceReviewStartPreview,
    );
    expect(chatApi.startAgentWorkspaceReview).toBe(startAgentWorkspaceReview);
    expect(chatApi.startAgentWorkspaceReviewFixer).toBe(
      startAgentWorkspaceReviewFixer,
    );
    expect(chatApi.submitAgentWorkspacePrReviewAction).toBe(
      submitAgentWorkspacePrReviewAction,
    );
    expect(chatApi.skipAgentWorkspacePrReviewAction).toBe(
      skipAgentWorkspacePrReviewAction,
    );
    expect(chatApi.setAgentWorkspacePrReviewAutoApprove).toBe(
      setAgentWorkspacePrReviewAutoApprove,
    );
    expect(chatApi.setAgentWorkspacePrReviewMonitoring).toBe(
      setAgentWorkspacePrReviewMonitoring,
    );
    expect(chatApi.switchAgentConversationMode).toBe(
      switchAgentConversationMode,
    );
    expect(chatApi.updateAgentConversationCoordinationMode).toBe(
      updateAgentConversationCoordinationMode,
    );
    expect(chatApi.forkAgentConversation).toBe(forkAgentConversation);
    expect(chatApi.archiveConversation).toBe(archiveConversation);
    expect(chatApi.restoreConversation).toBe(restoreConversation);
    expect(chatApi.getConversationActiveState).toBe(getConversationActiveState);
    expect(chatApi.getAgentRunningStates).toBe(getAgentRunningStates);
  });
});

describe("getConversationActiveState", () => {
  let mockFetch: ReturnType<typeof vi.fn>;
  const rawWorkspace = () => ({
    conversation_id: "conversation-1",
    project_id: "project-1",
    mode: "edit",
    base_ref_kind: "project_default",
    base_ref: "main",
    base_display_name: "Project default (main)",
    base_commit: "base-sha",
    branch_name: "ralphx/ralphx/agent-conversation-1",
    worktree_path: "/tmp/ralphx/conversation-1",
    linked_ideation_session_id: null,
    linked_plan_branch_id: null,
    source_pull_request: null,
    publication_pr_number: 411,
    publication_pr_url: "https://github.com/aigentive/ralphx.app/pull/411",
    publication_pr_status: "open",
    publication_push_status: "pushed",
    auto_publish_enabled: true,
    auto_publish_initial_pr_enabled: false,
    auto_publish_paused_pr_autofix_enabled: null,
    auto_publish_paused_pr_auto_merge_desired: null,
    pr_autofix_enabled: true,
    pr_auto_merge_desired: true,
    pr_auto_merge_method: "squash",
    pr_auto_merge_current: true,
    pr_supervision_status: "monitoring",
    pr_supervision_summary: "Watching PR",
    pr_supervision_updated_at: "2026-06-18T12:00:00Z",
    status: "active",
    created_at: "2026-06-18T12:00:00Z",
    updated_at: "2026-06-18T12:00:00Z",
  });
  const rawMonitor = (overrides: Record<string, unknown> = {}) => ({
    conversation_id: "conversation-1",
    project_id: "project-1",
    pr_number: 411,
    status: "awaiting_user",
    monitor_enabled: true,
    auto_approve_enabled: true,
    first_review_completed: false,
    first_action_resolved: false,
    last_seen_head_sha: "abcdef1234567890",
    last_reviewed_head_sha: null,
    last_review_run_id: "run-1",
    last_review_outcome: null,
    last_submitted_review_id: null,
    review_artifact_id: "review-artifact-1",
    review_artifact_head_sha: "abcdef1234567890",
    review_artifact_version: 1,
    review_artifact_updated_at: "2026-06-18T12:00:00Z",
    last_error: null,
    created_at: "2026-06-18T12:00:00Z",
    updated_at: "2026-06-18T12:00:00Z",
    ...overrides,
  });
  const rawAction = (overrides: Record<string, unknown> = {}) => ({
    id: "action-1",
    conversation_id: "conversation-1",
    pr_number: 411,
    head_sha: "abcdef1234567890",
    proposed_action: "request_changes",
    summary: "Found a regression",
    review_body: "Please fix the regression before merge.",
    findings_json: '[{"path":"src/lib.rs"}]',
    status: "pending",
    submitted_review_id: null,
    created_by_run_id: "run-1",
    created_at: "2026-06-18T12:00:00Z",
    updated_at: "2026-06-18T12:00:00Z",
    resolved_at: null,
    ...overrides,
  });
  const rawWorkspaceReviewMonitor = (
    overrides: Record<string, unknown> = {},
  ) => ({
    conversation_id: "conversation-1",
    project_id: "project-1",
    status: "ready",
    current_target_scope: "workspace_delta",
    reviewed_target_scope: "workspace_delta",
    review_conversation_id: "review-conversation-1",
    review_artifact_id: "review-artifact-1",
    review_artifact_version: 2,
    review_artifact_updated_at: "2026-06-18T12:05:00Z",
    review_requested_changes_artifact_id: "requested-changes-artifact-1",
    review_requested_changes_artifact_version: 2,
    review_requested_changes_artifact_updated_at: "2026-06-18T12:05:00Z",
    reviewed_head_sha: "head-sha",
    reviewed_diff_fingerprint: "fingerprint-1",
    selected_source_base_ref: null,
    selected_source_base_sha: null,
    selected_source_head_ref: null,
    selected_source_head_sha: null,
    selected_source_pull_request_number: null,
    workspace_base_ref: "main",
    workspace_base_sha: "base-sha",
    workspace_head_ref: "HEAD",
    workspace_head_sha: "head-sha",
    current_diff_fingerprint: "fingerprint-1",
    previous_version_id: "review-artifact-0",
    review_requested_changes_previous_version_id:
      "requested-changes-artifact-0",
    last_run_id: "run-1",
    last_error: null,
    created_at: "2026-06-18T12:00:00Z",
    updated_at: "2026-06-18T12:05:00Z",
    ...overrides,
  });
  const rawWorkspaceReviewTarget = (
    overrides: Record<string, unknown> = {},
  ) => ({
    scope: "workspace_delta",
    base_ref: "main",
    base_sha: "base-sha",
    head_ref: "HEAD",
    head_sha: "head-sha",
    diff_fingerprint: "fingerprint-1",
    source_pull_request_number: null,
    ...overrides,
  });

  beforeEach(() => {
    mockFetch = vi.fn();
    global.fetch = mockFetch;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("fetches conversation active state with stats fields", async () => {
    const mockResponse = {
      is_active: true,
      run_id: "run-parent-123",
      tool_calls: [],
      streaming_tasks: [
        {
          tool_use_id: "toolu_abc123",
          description: "Running tests",
          subagent_type: "ralphx:coder",
          model: "sonnet",
          status: "completed",
          total_tokens: 5000,
          total_tool_uses: 12,
          duration_ms: 45000,
          delegated_job_id: "job-123",
          delegated_session_id: "delegated-session-123",
          delegated_conversation_id: "conv-child-123",
          delegated_agent_run_id: "run-child-123",
          provider_harness: "codex",
          provider_session_id: "provider-session-123",
          upstream_provider: "openai",
          provider_profile: "prod",
          logical_model: "gpt-5.4",
          effective_model_id: "gpt-5.4-2026-04-01",
          logical_effort: "high",
          effective_effort: "high",
          approval_policy: "never",
          sandbox_mode: "danger-full-access",
          input_tokens: 1100,
          output_tokens: 2200,
          cache_creation_tokens: 330,
          cache_read_tokens: 440,
          estimated_usd: 1.23,
          text_output: "done",
        },
      ],
      partial_text: "",
      partial_thinking_segments: ["Reasoning through the response"],
    };

    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockResponse),
    });

    const result = await getConversationActiveState("conv-123");

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl("conversations/conv-123/active-state"),
    );
    expect(result.is_active).toBe(true);
    expect(result.runId).toBe("run-parent-123");
    expect(result.streaming_tasks).toHaveLength(1);
    const task = result.streaming_tasks[0];
    expect(task.tool_use_id).toBe("toolu_abc123");
    expect(task.total_tokens).toBe(5000);
    expect(task.total_tool_uses).toBe(12);
    expect(task.duration_ms).toBe(45000);
    expect(task.delegated_job_id).toBe("job-123");
    expect(task.provider_harness).toBe("codex");
    expect(task.logical_model).toBe("gpt-5.4");
    expect(task.input_tokens).toBe(1100);
    expect(task.estimated_usd).toBe(1.23);
    expect(task.text_output).toBe("done");
    expect(result.partial_thinking_segments).toEqual(["Reasoning through the response"]);
  });

  it("handles response with no stats fields (old format)", async () => {
    const mockResponse: ConversationActiveStateResponse = {
      is_active: true,
      tool_calls: [],
      streaming_tasks: [
        {
          tool_use_id: "toolu_xyz",
          status: "running",
        },
      ],
      partial_text: "Working...",
    };

    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockResponse),
    });

    const result = await getConversationActiveState("conv-456");

    expect(result.streaming_tasks[0].total_tokens).toBeUndefined();
    expect(result.streaming_tasks[0].total_tool_uses).toBeUndefined();
    expect(result.streaming_tasks[0].duration_ms).toBeUndefined();
  });

  it("throws on non-ok response", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 404,
    });

    await expect(getConversationActiveState("conv-missing")).rejects.toThrow(
      "Failed to get conversation active state: 404",
    );
  });

  it("fetches and transforms agent workspace PR review context", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          success: true,
          workspace: rawWorkspace(),
          events: [
            {
              id: "event-1",
              conversation_id: "conversation-1",
              step: "published",
              status: "succeeded",
              summary: "Draft pull request is ready",
              classification: null,
              created_at: "2026-06-18T12:01:00Z",
            },
          ],
          pr_number: 411,
          pr_url: "https://github.com/aigentive/ralphx.app/pull/411",
          current_head_sha: "abcdef1234567890",
          pending_action_head_status: "current",
          health: { merge_state_status: "Blocked" },
          review_feedback: null,
          monitor: rawMonitor(),
          pending_action: rawAction(),
          recent_actions: [
            rawAction({
              id: "action-0",
              status: "skipped",
              resolved_at: "2026-06-18T12:02:00Z",
            }),
          ],
          issue_comment_evidence: [{ comment_id: "comment-1" }],
        }),
    });

    const result = await getAgentWorkspacePrReviewContext("conversation-1");

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl("agent-workspaces/conversation-1/pr-review-context"),
      undefined,
    );
    expect(result.workspace.conversationId).toBe("conversation-1");
    expect(result.events[0]?.conversationId).toBe("conversation-1");
    expect(result.monitor?.lastReviewRunId).toBe("run-1");
    expect(result.monitor?.reviewArtifactHeadSha).toBe("abcdef1234567890");
    expect(result.pendingAction?.proposedAction).toBe("request_changes");
    expect(result.pendingActionHeadStatus).toBe("current");
    expect(result.recentActions[0]?.status).toBe("skipped");
    expect(result.issueCommentEvidence).toEqual([{ comment_id: "comment-1" }]);
  });

  it("fetches and transforms general workspace review context", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          success: true,
          workspace: rawWorkspace(),
          events: [],
          target: rawWorkspaceReviewTarget(),
          monitor: rawWorkspaceReviewMonitor(),
          review_artifact_is_current: true,
          review_artifact_is_outdated: false,
          can_mutate_review_state: false,
          review_runtime_state: "missing_runtime_identity",
          is_current: true,
          is_outdated: false,
          should_show_tab: true,
        }),
    });

    const result = await getAgentWorkspaceReviewContext("conversation-1");

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl("agent-workspaces/conversation-1/workspace-review-context"),
      undefined,
    );
    expect(result.workspace.conversationId).toBe("conversation-1");
    expect(result.target?.scope).toBe("workspace_delta");
    expect(result.target?.diffFingerprint).toBe("fingerprint-1");
    expect(result.monitor.reviewArtifactVersion).toBe(2);
    expect(result.monitor.reviewRequestedChangesArtifactId).toBe(
      "requested-changes-artifact-1",
    );
    expect(result.monitor.reviewConversationId).toBe("review-conversation-1");
    expect(result.monitor.previousVersionId).toBe("review-artifact-0");
    expect(result.isCurrent).toBe(true);
    expect(result.reviewArtifactIsCurrent).toBe(true);
    expect(result.reviewArtifactIsOutdated).toBe(false);
    expect(result.canMutateReviewState).toBe(false);
    expect(result.reviewRuntimeState).toBe("missing_runtime_identity");
  });

  it("forwards cancellation and full-target refresh options for workspace review context", async () => {
    const controller = new AbortController();
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          success: true,
          workspace: rawWorkspace(),
          events: [],
          target: rawWorkspaceReviewTarget(),
          monitor: rawWorkspaceReviewMonitor(),
          review_artifact_is_current: false,
          review_artifact_is_outdated: false,
          can_mutate_review_state: false,
          review_runtime_state: "missing_runtime_identity",
          is_current: false,
          is_outdated: false,
          should_show_tab: true,
        }),
    });

    await getAgentWorkspaceReviewContext("conversation/1", {
      signal: controller.signal,
      refreshTarget: true,
    });

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl(
        "agent-workspaces/conversation%2F1/workspace-review-context?refresh_target=true",
      ),
      { signal: controller.signal },
    );
  });

  it("starts a general workspace review run through the encoded REST endpoint", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          success: true,
          target: rawWorkspaceReviewTarget({
            scope: "selected_source",
            source_pull_request_number: 42,
          }),
          monitor: rawWorkspaceReviewMonitor({
            status: "reviewing",
            current_target_scope: "selected_source",
          }),
          is_current: false,
          is_outdated: true,
          should_show_tab: true,
          started: true,
          skipped_reason: null,
          was_queued: false,
        }),
    });

    const result = await startAgentWorkspaceReview("conversation/1", {
      force: true,
      runtimeOverride: {
        provider: "codex",
        model: "gpt-5.5",
        effort: "high",
        serviceTier: "standard",
        coordinationMode: "solo",
        personaId: null,
      },
    });

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl("agent-workspaces/conversation%2F1/workspace-review-runs"),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          force: true,
          runtime_override: {
            provider: "codex",
            model: "gpt-5.5",
            effort: "high",
            service_tier: "standard",
            coordination_mode: "solo",
            persona_id: null,
          },
        }),
      },
    );
    expect(result.started).toBe(true);
    expect(result.target?.scope).toBe("selected_source");
    expect(result.target?.sourcePullRequestNumber).toBe(42);
    expect(result.monitor.status).toBe("reviewing");
  });

  it("preserves workspace review HTTP conflicts for a receipt refresh", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 409,
      statusText: "Conflict",
      json: () =>
        Promise.resolve({
          error: "workspace Review target or GitHub auto-merge state changed",
        }),
    });

    await expect(startAgentWorkspaceReview("conversation-1")).rejects.toEqual(
      expect.objectContaining({
        name: "AgentWorkspaceHttpError",
        status: 409,
        detail: "workspace Review target or GitHub auto-merge state changed",
      }),
    );
  });

  it("fetches the target-bound workspace review start preview", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          success: true,
          target: rawWorkspaceReviewTarget(),
          will_disable_auto_merge: true,
          pr_number: 42,
          merge_method: "squash",
          restore_after_publish: true,
          confirmation: {
            target_scope: "workspace_delta",
            diff_fingerprint: "fingerprint-1",
            head_sha: "head-sha-1",
            pr_number: 42,
            will_disable_auto_merge: true,
            merge_method: "squash",
            restore_after_publish: true,
          },
        }),
    });

    const result = await getAgentWorkspaceReviewStartPreview("conversation/1");

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl(
        "agent-workspaces/conversation%2F1/workspace-review-start-preview",
      ),
      undefined,
    );
    expect(result.confirmation).toEqual({
      targetScope: "workspace_delta",
      diffFingerprint: "fingerprint-1",
      headSha: "head-sha-1",
      prNumber: 42,
      willDisableAutoMerge: true,
      mergeMethod: "squash",
      restoreAfterPublish: true,
    });
    expect(result.restoreAfterPublish).toBe(true);
  });

  it("starts the workspace review fixer through the encoded REST endpoint", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          success: true,
          target: rawWorkspaceReviewTarget(),
          monitor: rawWorkspaceReviewMonitor({
            review_outcome: "blocking",
            review_gate_status: "blocking",
            review_blocking_summary: "Fix the blocking finding.",
            review_fixer_status: "running",
            review_fixer_run_id: "fixer-run-1",
            review_fixer_conversation_id: "conversation-1",
          }),
          is_current: true,
          is_outdated: false,
          should_show_tab: true,
          started: true,
          skipped_reason: null,
        }),
    });

    const result = await startAgentWorkspaceReviewFixer("conversation/1", {
      confirmation: {
        targetScope: "workspace_delta",
        diffFingerprint: "fingerprint-1",
        artifactId: "artifact-1",
        artifactVersion: 3,
        blockingFingerprint: "blocking-1",
      },
      runtimeOverride: {
        provider: "codex",
        model: "gpt-5.5",
        effort: "high",
        serviceTier: "standard",
        coordinationMode: "solo",
        personaId: null,
      },
    });

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl(
        "agent-workspaces/conversation%2F1/workspace-review-fixer-runs",
      ),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          confirmation: {
            target_scope: "workspace_delta",
            diff_fingerprint: "fingerprint-1",
            artifact_id: "artifact-1",
            artifact_version: 3,
            blocking_fingerprint: "blocking-1",
          },
          runtime_override: {
            provider: "codex",
            model: "gpt-5.5",
            effort: "high",
            service_tier: "standard",
            coordination_mode: "solo",
            persona_id: null,
          },
        }),
      },
    );
    expect(result.started).toBe(true);
    expect(result.isCurrent).toBe(true);
    expect(result.monitor.reviewFixerStatus).toBe("running");
    expect(result.monitor.reviewFixerRunId).toBe("fixer-run-1");
  });

  it("approves an exact blocking workspace Review through the encoded REST endpoint", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          success: true,
          monitor: rawWorkspaceReviewMonitor({
            review_outcome: "blocking",
            review_gate_status: "passed",
            review_gate_bypassed_at: "2026-06-18T12:06:00Z",
            review_gate_bypassed_target_scope: "workspace_delta",
            review_gate_bypassed_diff_fingerprint: "fingerprint-1",
            review_gate_bypassed_artifact_id: "review-artifact-1",
            review_gate_bypassed_artifact_version: 2,
          }),
        }),
    });

    const result = await approveAgentWorkspaceReviewAnyway("conversation/1", {
      targetScope: "workspace_delta",
      diffFingerprint: "fingerprint-1",
      artifactId: "review-artifact-1",
      artifactVersion: 2,
    });

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl(
        "agent-workspaces/conversation%2F1/workspace-review-approve-anyway",
      ),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          target_scope: "workspace_delta",
          diff_fingerprint: "fingerprint-1",
          artifact_id: "review-artifact-1",
          artifact_version: 2,
        }),
      },
    );
    expect(result.monitor.reviewOutcome).toBe("blocking");
    expect(result.monitor.reviewGateStatus).toBe("passed");
    expect(result.monitor.reviewGateBypassedArtifactVersion).toBe(2);
  });

  it("submits and skips agent workspace PR review actions through encoded REST endpoints", async () => {
    mockFetch
      .mockResolvedValueOnce({
        ok: true,
        json: () =>
          Promise.resolve({
            success: true,
            monitor: rawMonitor(),
            action: rawAction({
              status: "submitted",
              submitted_review_id: "review-1",
              resolved_at: "2026-06-18T12:03:00Z",
            }),
            submitted_review_id: "review-1",
            submitted_review_url:
              "https://github.com/aigentive/ralphx.app/pull/411#pullrequestreview-1",
          }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: () =>
          Promise.resolve({
            success: true,
            monitor: rawMonitor({ status: "watching" }),
            action: rawAction({
              status: "skipped",
              resolved_at: "2026-06-18T12:04:00Z",
            }),
          }),
      });

    const submitted = await submitAgentWorkspacePrReviewAction(
      "conversation/1",
      "action/1",
      "approve",
    );
    const skipped = await skipAgentWorkspacePrReviewAction(
      "conversation/1",
      "action/1",
      null,
    );

    expect(mockFetch).toHaveBeenNthCalledWith(
      1,
      backendApiUrl(
        "agent-workspaces/conversation%2F1/pr-review-actions/action%2F1/submit",
      ),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action_kind: "approve" }),
      },
    );
    expect(mockFetch).toHaveBeenNthCalledWith(
      2,
      backendApiUrl(
        "agent-workspaces/conversation%2F1/pr-review-actions/action%2F1/skip",
      ),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reason: null }),
      },
    );
    expect(submitted.submittedReviewId).toBe("review-1");
    expect(submitted.action.status).toBe("submitted");
    expect(skipped.action.status).toBe("skipped");
  });

  it("updates Review PR Auto Approve through the typed REST setting endpoint", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          success: true,
          monitor: rawMonitor({ auto_approve_enabled: false }),
        }),
    });

    const result = await setAgentWorkspacePrReviewAutoApprove(
      "conversation/1",
      false,
    );

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl("agent-workspaces/conversation%2F1/pr-review-settings"),
      {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ auto_approve_enabled: false }),
      },
    );
    expect(result.monitor.autoApproveEnabled).toBe(false);
    expect(result.monitor.firstActionResolved).toBe(false);
  });

  it("updates Review PR monitoring through the typed REST setting endpoint", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          success: true,
          monitor: rawMonitor({
            monitor_enabled: false,
            status: "paused",
          }),
        }),
    });

    const result = await setAgentWorkspacePrReviewMonitoring(
      "conversation/1",
      false,
      "cancel_current",
    );

    expect(mockFetch).toHaveBeenCalledWith(
      backendApiUrl("agent-workspaces/conversation%2F1/pr-review-settings"),
      {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          monitor_enabled: false,
          active_review_policy: "cancel_current",
        }),
      },
    );
    expect(result.monitor.monitorEnabled).toBe(false);
    expect(result.monitor.status).toBe("paused");
  });

  it("lists and mutates agent conversation issues through REST endpoints", async () => {
    const rawIssue = {
      id: "issue-1",
      project_id: "project-1",
      conversation_id: "conversation-1",
      source_task_id: "task-1",
      source_context_type: "review",
      source_context_id: "review-1",
      source_agent_name: "ralphx-execution-reviewer",
      issue_kind: "plan_drift",
      severity: "high",
      status: "open",
      blocking_scope: "followup_only",
      title: "Plan drift",
      summary: "Reviewer found unrelated work.",
      evidence: "src/unrelated.rs",
      recommendation: "Create a follow-up.",
      blocker_fingerprint: "scope:task-1",
      canonical_fingerprint: "v1:scope-drift:task:task-1:files:abc123",
      canonical_scope_kind: "task",
      canonical_scope_subject: "task-1",
      canonical_family: "scope-drift",
      superseded_by_issue_id: null,
      occurrence_count: 1,
      occurrences: [
        {
          id: "occurrence-1",
          issue_id: "issue-1",
          source_task_id: "task-1",
          source_context_type: "review",
          source_context_id: "review-1",
          source_agent_name: "ralphx-execution-reviewer",
          issue_kind: "plan_drift",
          severity: "high",
          blocking_scope: "followup_only",
          title: "Plan drift",
          summary: "Reviewer found unrelated work.",
          evidence: "src/unrelated.rs",
          recommendation: "Create a follow-up.",
          raw_blocker_fingerprint: "scope:task-1",
          canonical_fingerprint: "v1:scope-drift:task:task-1:files:abc123",
          dedupe_decision: "created",
          created_at: "2026-06-25T12:01:00Z",
        },
      ],
      followup_title: "Investigate drift",
      followup_prompt: "Plan the unrelated work separately.",
      auto_followup_eligible: true,
      linked_followup_conversation_id: null,
      created_at: "2026-06-25T12:00:00Z",
      updated_at: "2026-06-25T12:01:00Z",
      resolved_at: null,
    };
    mockFetch
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ issues: [rawIssue] }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: () =>
          Promise.resolve({
            issue: {
              ...rawIssue,
              status: "resolved",
              resolved_at: "2026-06-25T12:02:00Z",
            },
          }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: () =>
          Promise.resolve({
            issue: {
              ...rawIssue,
              linked_followup_conversation_id: "followup-conversation-1",
            },
            followup: { reused_existing: false },
          }),
      });

    const issues = await listAgentConversationIssues("conversation-1", {
      includeResolved: true,
    });
    const resolved = await updateAgentConversationIssueStatus(
      "issue-1",
      "resolved",
    );
    const converted = await convertAgentConversationIssueFollowup("issue-1");

    expect(mockFetch).toHaveBeenNthCalledWith(
      1,
      backendApiUrl("agent_conversation_issues/list"),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          conversation_id: "conversation-1",
          include_resolved: true,
        }),
      },
    );
    expect(mockFetch).toHaveBeenNthCalledWith(
      2,
      backendApiUrl("agent_conversation_issues/status"),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ issue_id: "issue-1", status: "resolved" }),
      },
    );
    expect(mockFetch).toHaveBeenNthCalledWith(
      3,
      backendApiUrl("agent_conversation_issues/convert_followup"),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ issue_id: "issue-1" }),
      },
    );
    expect(issues[0]).toMatchObject({
      id: "issue-1",
      sourceTaskId: "task-1",
      blockerFingerprint: "scope:task-1",
      canonicalFingerprint: "v1:scope-drift:task:task-1:files:abc123",
      occurrenceCount: 1,
      autoFollowupEligible: true,
    });
    expect(issues[0].occurrences[0]).toMatchObject({
      id: "occurrence-1",
      issueId: "issue-1",
      rawBlockerFingerprint: "scope:task-1",
      dedupeDecision: "created",
    });
    expect(resolved.status).toBe("resolved");
    expect(resolved.resolvedAt).toBe("2026-06-25T12:02:00Z");
    expect(converted.linkedFollowupConversationId).toBe(
      "followup-conversation-1",
    );
  });

  it("surfaces backend error detail for agent workspace PR review requests", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 409,
      statusText: "Conflict",
      json: () => Promise.resolve({ detail: "No linked pull request" }),
    });

    await expect(
      getAgentWorkspacePrReviewContext("conversation-1"),
    ).rejects.toThrow("409 Conflict: No linked pull request");
  });
});

describe("startAgentConversationInvokeInput", () => {
  it("omits projectId for standalone chat starts", () => {
    expect(
      startAgentConversationInvokeInput({
        content: "hello",
        conversationId: "standalone-1",
        mode: "chat",
      }),
    ).toEqual({
      content: "hello",
      conversationId: "standalone-1",
      mode: "chat",
    });
  });
  it("maps persona-builder refine provenance without inventing a standalone project", () => {
    expect(
      startAgentConversationInvokeInput({
        content: "Refine this voice",
        mode: "persona_builder",
        sourcePersonaId: "persona-reviewer",
      }),
    ).toEqual({
      content: "Refine this voice",
      mode: "persona_builder",
      sourcePersonaId: "persona-reviewer",
    });
  });
  it("includes only projectId and content when all optional fields are absent", () => {
    const out = startAgentConversationInvokeInput({
      projectId: "project-1",
      content: "hello",
    });

    expect(out).toEqual({ projectId: "project-1", content: "hello" });
  });

  it("omits null/undefined optional scalar fields", () => {
    const out = startAgentConversationInvokeInput({
      projectId: "project-1",
      content: "hello",
      conversationId: null,
      providerHarness: null,
      modelId: null,
      logicalEffort: null,
      personaId: null,
      base: null,
    });

    expect(out).toEqual({ projectId: "project-1", content: "hello" });
    expect(out).not.toHaveProperty("conversationId");
    expect(out).not.toHaveProperty("providerHarness");
    expect(out).not.toHaveProperty("modelOverride");
    expect(out).not.toHaveProperty("logicalEffort");
    expect(out).not.toHaveProperty("personaId");
    expect(out).not.toHaveProperty("baseRefKind");
  });

  it("maps all populated optional fields, renaming modelId to modelOverride", () => {
    const out = startAgentConversationInvokeInput({
      projectId: "project-1",
      content: "do the thing",
      conversationId: "conversation-9",
      providerHarness: "codex",
      modelId: "gpt-5.5",
      logicalEffort: "xhigh",
      personaId: "persona-reviewer",
      codexFastMode: true,
      mode: "chat",
      teamIntent: {
        coordinationMode: "rx_native_team",
        strategy: "execution",
      },
      composerProjectReferences: [{ path: "src/main.ts", kind: "file" }],
      composerIntegrationReferences: [
        { provider: "linear", kind: "linear", id: "ISS-1" },
      ],
      composerArtifactReferences: [{ artifactId: "artifact-1", kind: "plan" }],
      base: {
        kind: "local_branch",
        branchMode: "linked",
        ref: "feature/x",
        displayName: "PR #7",
        sourcePullRequest: {
          number: 7,
          url: "https://github.com/owner/repo/pull/7",
          title: "Add x",
          headRefName: "feature/x",
          baseRefName: "main",
          headRefOid: "deadbeef",
        },
      },
    });

    expect(out).toEqual({
      projectId: "project-1",
      content: "do the thing",
      conversationId: "conversation-9",
      providerHarness: "codex",
      modelOverride: "gpt-5.5",
      logicalEffort: "xhigh",
      personaId: "persona-reviewer",
      codexFastMode: true,
      mode: "chat",
      teamIntent: {
        coordinationMode: "rx_native_team",
        strategy: "execution",
      },
      composerProjectReferences: [{ path: "src/main.ts", kind: "file" }],
      composerIntegrationReferences: [
        { provider: "linear", kind: "linear", id: "ISS-1" },
      ],
      composerArtifactReferences: [{ artifactId: "artifact-1", kind: "plan" }],
      baseRefKind: "local_branch",
      baseBranchMode: "linked",
      baseRef: "feature/x",
      baseDisplayName: "PR #7",
      baseSourcePullRequest: {
        number: 7,
        url: "https://github.com/owner/repo/pull/7",
        title: "Add x",
        headRefName: "feature/x",
        baseRefName: "main",
        headRefOid: "deadbeef",
      },
    });
  });

  it("prefers capabilityIntent while retaining teamIntent compatibility", () => {
    expect(
      startAgentConversationInvokeInput({
        projectId: "project-1",
        content: "use Ultra",
        capabilityIntent: { coordinationMode: "codex_native_ultra" },
        teamIntent: { coordinationMode: "rx_native_team" },
      }),
    ).toEqual({
      projectId: "project-1",
      content: "use Ultra",
      capabilityIntent: { coordinationMode: "codex_native_ultra" },
    });
  });

  it("sets base ref fields but omits baseSourcePullRequest when base has no sourcePullRequest", () => {
    const out = startAgentConversationInvokeInput({
      projectId: "project-1",
      content: "hello",
      base: {
        kind: "project_default",
        ref: "main",
        displayName: "main",
      },
    });

    expect(out).toMatchObject({
      baseRefKind: "project_default",
      baseRef: "main",
      baseDisplayName: "main",
    });
    expect(out).not.toHaveProperty("baseSourcePullRequest");
  });

  it("serializes isolated branch mode for pull request start bases", () => {
    const out = startAgentConversationInvokeInput({
      projectId: "project-1",
      content: "review the PR",
      mode: "review_pr",
      base: {
        kind: "local_branch",
        branchMode: "isolated",
        ref: "feature/pr-default",
        displayName: "PR #42",
        sourcePullRequest: {
          number: 42,
          title: "Default isolated PR",
          url: "https://github.com/owner/repo/pull/42",
          headRefName: "feature/pr-default",
          baseRefName: "main",
          headRefOid: "abc123",
        },
      },
    });

    expect(out).toMatchObject({
      mode: "review_pr",
      baseRefKind: "local_branch",
      baseBranchMode: "isolated",
      baseRef: "feature/pr-default",
      baseSourcePullRequest: expect.objectContaining({
        number: 42,
        headRefName: "feature/pr-default",
      }),
    });
  });

  it("filters out empty composer reference arrays", () => {
    const out = startAgentConversationInvokeInput({
      projectId: "project-1",
      content: "hello",
      composerProjectReferences: [],
      composerIntegrationReferences: [],
      composerArtifactReferences: [],
    });

    expect(out).toEqual({ projectId: "project-1", content: "hello" });
    expect(out).not.toHaveProperty("composerProjectReferences");
    expect(out).not.toHaveProperty("composerIntegrationReferences");
    expect(out).not.toHaveProperty("composerArtifactReferences");
  });
});

describe("transformStartAgentConversationResponse", () => {
  it("transforms a full raw response into a StartAgentConversationResult", () => {
    const raw = StartAgentConversationResponseSchema.parse({
      conversation: {
        id: "conversation-chat",
        context_type: "project",
        context_id: "project-1",
        claude_session_id: null,
        provider_session_id: null,
        provider_harness: "codex",
        service_tier: "fast",
        agent_mode: "chat",
        coordination_mode: "rx_native_team",
        title: "Chat",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
        archived_at: null,
      },
      workspace: {
        conversation_id: "conversation-chat",
        project_id: "project-1",
        mode: "chat",
        branch_mode: "linked",
        base_ref_kind: "local_branch",
        base_ref: "feature/agent-screen",
        base_display_name: "PR #42",
        base_commit: null,
        branch_name: "ralphx/demo/agent-conversation-chat",
        worktree_path: "/tmp/ralphx/conversation-chat",
        linked_ideation_session_id: null,
        linked_plan_branch_id: null,
        source_pull_request: {
          number: 42,
          url: "https://github.com/owner/repo/pull/42",
          title: "Add PR picker",
          head_ref_name: "feature/agent-screen",
          base_ref_name: "main",
          head_ref_oid: "abc123",
        },
        publication_pr_number: null,
        publication_pr_url: null,
        publication_pr_status: null,
        publication_push_status: null,
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
      },
      send_result: {
        conversation_id: "conversation-chat",
        agent_run_id: "run-chat",
        is_new_conversation: true,
      },
    });

    const result = transformStartAgentConversationResponse(raw);

    expect(result.conversation.id).toBe("conversation-chat");
    expect(result.conversation.agentMode).toBe("chat");
    expect(result.conversation.coordinationMode).toBe("rx_native_team");
    expect(result.conversation.serviceTier).toBe("fast");
    expect(result.workspace).toMatchObject({
      conversationId: "conversation-chat",
      mode: "chat",
      branchMode: "linked",
      baseRef: "feature/agent-screen",
      sourcePullRequest: expect.objectContaining({
        number: 42,
        headRefName: "feature/agent-screen",
        baseRefName: "main",
      }),
    });
    expect(result.sendResult).toMatchObject({
      conversationId: "conversation-chat",
      agentRunId: "run-chat",
      isNewConversation: true,
    });
  });

  it("maps a null workspace to null", () => {
    const raw = StartAgentConversationResponseSchema.parse({
      conversation: {
        id: "conversation-2",
        context_type: "project",
        context_id: "project-1",
        claude_session_id: null,
        provider_session_id: null,
        provider_harness: null,
        agent_mode: "chat",
        title: "Chat",
        message_count: 0,
        last_message_at: null,
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:00:00Z",
        archived_at: null,
      },
      workspace: null,
      send_result: {
        conversation_id: "conversation-2",
        agent_run_id: "run-2",
        is_new_conversation: true,
      },
    });

    const result = transformStartAgentConversationResponse(raw);

    expect(result.workspace).toBeNull();
    expect(result.conversation.id).toBe("conversation-2");
  });
});
