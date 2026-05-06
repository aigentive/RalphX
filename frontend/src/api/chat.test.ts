import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  parseContentBlocks,
  parseToolCalls,
  listConversations,
  listConversationsPage,
  getConversation,
  getConversationMessagesPage,
  getConversationStats,
  createConversation,
  updateConversationTitle,
  spawnConversationSessionNamer,
  archiveConversation,
  restoreConversation,
  getAgentRunStatus,
  getAgentConversationWorkspaceFreshness,
  listAgentConversationWorkspacePublicationEvents,
  listAgentConversationWorkspacesByProject,
  updateAgentConversationWorkspaceFromBase,
  startAgentConversation,
  switchAgentConversationMode,
  sendAgentMessage,
  getQueuedAgentMessages,
  deleteQueuedAgentMessage,
  isChatServiceAvailable,
  stopAgent,
  isAgentRunning,
  chatApi,
  getConversationActiveState,
  getChildSessionStatus,
} from "./chat";
import type { ConversationActiveStateResponse } from "./chat";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

describe("chat api", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    delete window.__TAURI_INTERNALS__;
    delete window.__mockChatApi;
  });

  it("parses tool calls", () => {
    const parsed = parseToolCalls('[{"id":"t1","name":"bash","arguments":{"command":"ls"}}]');
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toMatchObject({ id: "t1", name: "bash" });
  });

  it("preserves parent tool linkage on parsed tool calls", () => {
    const parsed = parseToolCalls('[{"id":"t1","name":"bash","arguments":{"command":"ls"},"parent_tool_use_id":"delegate-1"}]');
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toMatchObject({
      id: "t1",
      name: "bash",
      parentToolUseId: "delegate-1",
    });
  });

  it("parses content blocks", () => {
    const parsed = parseContentBlocks('[{"type":"text","text":"hello"}]');
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toMatchObject({ type: "text", text: "hello" });
  });

  it("preserves parent tool linkage on parsed content blocks", () => {
    const parsed = parseContentBlocks('[{"type":"tool_use","id":"tool-1","name":"bash","arguments":{"command":"ls"},"parent_tool_use_id":"delegate-1"}]');
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toMatchObject({
      type: "tool_use",
      id: "tool-1",
      name: "bash",
      parentToolUseId: "delegate-1",
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
      claudeSessionId: null,
    });
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
      "sidebar"
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

    await listConversationsPage("project", "p-page", 1, 0, true, undefined, true);

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

    await spawnConversationSessionNamer("conversation-42", "fix the agents landing flow");

    expect(mockInvoke).toHaveBeenCalledWith("spawn_session_namer", {
      conversationId: "conversation-42",
      firstMessage: "fix the agents landing flow",
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
          metadata: "{\"verification_result\":true}",
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

    expect(mockInvoke).toHaveBeenCalledWith("get_agent_conversation", { conversationId: "c1" });
    expect(result.messages[0]).toMatchObject({
      id: "m1",
      conversationId: "c1",
      createdAt: "2026-01-24T10:00:00Z",
      metadata: "{\"verification_result\":true}",
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

    expect(mockInvoke).toHaveBeenCalledWith("get_agent_conversation_messages_page", {
      conversationId: "c1",
      limit: 40,
      offset: 0,
    });
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
        estimated_usd: 0.42,
      },
      run_usage_totals: {
        input_tokens: 999,
        output_tokens: 111,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        estimated_usd: 1.25,
      },
      effective_usage_totals: {
        input_tokens: 120,
        output_tokens: 40,
        cache_creation_tokens: 5,
        cache_read_tokens: 8,
        estimated_usd: 0.42,
      },
      usage_coverage: {
        provider_message_count: 1,
        provider_messages_with_usage: 1,
        run_count: 1,
        runs_with_usage: 1,
        effective_totals_source: "messages",
      },
      attribution_coverage: {
        provider_message_count: 1,
        provider_messages_with_attribution: 1,
        run_count: 1,
        runs_with_attribution: 1,
      },
      by_harness: [{
        key: "codex",
        count: 1,
        usage: {
          input_tokens: 120,
          output_tokens: 40,
          cache_creation_tokens: 5,
          cache_read_tokens: 8,
          estimated_usd: 0.42,
        },
      }],
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

    const result = await updateConversationTitle("c-title", " Review agent title ");

    expect(mockInvoke).toHaveBeenCalledWith("update_agent_conversation_title", {
      conversationId: "c-title",
      title: "Review agent title",
    });
    expect(result.title).toBe("Review agent title");
  });

  it("archives conversation", async () => {
    mockInvoke.mockResolvedValue({
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
    });

    const result = await archiveConversation("c-archive");

    expect(mockInvoke).toHaveBeenCalledWith("archive_agent_conversation", {
      conversationId: "c-archive",
    });
    expect(result.archivedAt).toBe("2026-01-24T10:01:00Z");
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
        status: "active",
        created_at: "2026-01-24T10:00:00Z",
        updated_at: "2026-01-24T10:01:00Z",
      },
    ]);

    const result = await listAgentConversationWorkspacesByProject("project-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "list_agent_conversation_workspaces_by_project",
      { projectId: "project-1" }
    );
    expect(result[0]).toMatchObject({
      conversationId: "conversation-1",
      projectId: "project-1",
      branchName: "ralphx/demo/agent-conversation-1",
    });
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
      { conversationId: "conversation-1" }
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
      base_ref: "feature/agent-screen",
      base_display_name: "Current branch (feature/agent-screen)",
      target_ref: "origin/feature/agent-screen",
      captured_base_commit: "old-base",
      target_base_commit: "new-base",
      is_base_ahead: true,
      has_uncommitted_changes: true,
      unpublished_commit_count: 2,
    });

    const result = await getAgentConversationWorkspaceFreshness("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_workspace_freshness",
      { conversationId: "conversation-1" }
    );
    expect(result).toMatchObject({
      conversationId: "conversation-1",
      baseRef: "feature/agent-screen",
      targetRef: "origin/feature/agent-screen",
      isBaseAhead: true,
      hasUncommittedChanges: true,
      unpublishedCommitCount: 2,
    });
  });

  it("updates an agent conversation workspace from its base branch", async () => {
    mockInvoke.mockResolvedValue({
      workspace: {
        conversation_id: "conversation-1",
        project_id: "project-1",
        mode: "edit",
        base_ref_kind: "current_branch",
        base_ref: "feature/agent-screen",
        base_display_name: "Current branch (feature/agent-screen)",
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
      target_ref: "origin/feature/agent-screen",
      base_commit: "new-base",
    });

    const result = await updateAgentConversationWorkspaceFromBase("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "update_agent_conversation_workspace_from_base",
      { conversationId: "conversation-1" }
    );
    expect(result).toMatchObject({
      updated: true,
      targetRef: "origin/feature/agent-screen",
      baseCommit: "new-base",
      workspace: {
        conversationId: "conversation-1",
        baseCommit: "new-base",
        publicationPushStatus: "refreshed",
      },
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
        estimatedUsd: null,
      },
      runUsageTotals: {
        inputTokens: 2535967,
        outputTokens: 13593,
        cacheCreationTokens: 0,
        cacheReadTokens: 2434048,
        estimatedUsd: null,
      },
      effectiveUsageTotals: {
        inputTokens: 2535967,
        outputTokens: 13593,
        cacheCreationTokens: 0,
        cacheReadTokens: 2434048,
        estimatedUsd: null,
      },
      usageCoverage: {
        providerMessageCount: 1,
        providerMessagesWithUsage: 1,
        runCount: 1,
        runsWithUsage: 1,
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
            estimatedUsd: null,
          },
        },
      ],
      byUpstreamProvider: [],
      byModel: [{ key: "gpt-5.4", count: 1, usage: {
        inputTokens: 2535967,
        outputTokens: 13593,
        cacheCreationTokens: 0,
        cacheReadTokens: 2434048,
        estimatedUsd: null,
      } }],
      byEffort: [{ key: "medium", count: 1, usage: {
        inputTokens: 2535967,
        outputTokens: 13593,
        cacheCreationTokens: 0,
        cacheReadTokens: 2434048,
        estimatedUsd: null,
      } }],
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
        base_ref_kind: "current_branch",
        base_ref: "feature/agent-screen",
        base_display_name: "Current branch (feature/agent-screen)",
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
      mode: "chat",
      base: {
        kind: "current_branch",
        ref: "feature/agent-screen",
        displayName: "Current branch (feature/agent-screen)",
      },
    });

    expect(mockInvoke).toHaveBeenCalledWith("start_agent_conversation", {
      input: {
        projectId: "project-1",
        content: "What changed?",
        providerHarness: "codex",
        modelOverride: "gpt-5.5",
        logicalEffort: "xhigh",
        mode: "chat",
        baseRefKind: "current_branch",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
      },
    });
    expect(result.conversation.agentMode).toBe("chat");
    expect(result.workspace).toMatchObject({
      mode: "chat",
      baseRefKind: "current_branch",
      baseRef: "feature/agent-screen",
    });
  });

  it("switches an existing agent conversation mode", async () => {
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
      mode: "edit",
    });

    expect(mockInvoke).toHaveBeenCalledWith("switch_agent_conversation_mode", {
      input: {
        conversationId: "conversation-chat",
        mode: "edit",
      },
    });
    expect(result.conversation.agentMode).toBe("edit");
    expect(result.workspace?.mode).toBe("edit");
  });

  it("uses the web-mode chat mock for child session status when available", async () => {
    window.__mockChatApi = {
      reset: vi.fn(),
      seedScenario: vi.fn(),
      listScenarios: vi.fn().mockReturnValue([]),
      listConversations: vi.fn(),
      getConversation: vi.fn(),
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

    expect(window.__mockChatApi.getChildSessionStatus).toHaveBeenCalledWith("child-1");
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

    await sendAgentMessage("project", "p1", "Hello", undefined, undefined, {
      conversationId: "c1",
      providerHarness: "codex",
      modelId: "gpt-5.4",
      logicalEffort: "high",
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
      },
    });
  });

  it("lists queued messages", async () => {
    mockInvoke.mockResolvedValueOnce([{ id: "q1", content: "queued", created_at: "2026-01-24T10:00:00Z", is_editing: false }]);

    const list = await getQueuedAgentMessages("project", "p1");

    expect(list).toHaveLength(1);
  });

  it("deletes queued message", async () => {
    mockInvoke.mockResolvedValue(true);
    const result = await deleteQueuedAgentMessage("project", "p1", "q1");
    expect(result).toBe(true);
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

  it("exports chatApi namespace", () => {
    expect(chatApi.sendAgentMessage).toBe(sendAgentMessage);
    expect(chatApi.listConversations).toBe(listConversations);
    expect(chatApi.listAgentConversationWorkspacesByProject).toBe(
      listAgentConversationWorkspacesByProject
    );
    expect(chatApi.listAgentConversationWorkspacePublicationEvents).toBe(
      listAgentConversationWorkspacePublicationEvents
    );
    expect(chatApi.switchAgentConversationMode).toBe(switchAgentConversationMode);
    expect(chatApi.archiveConversation).toBe(archiveConversation);
    expect(chatApi.restoreConversation).toBe(restoreConversation);
    expect(chatApi.getConversationActiveState).toBe(getConversationActiveState);
  });
});

describe("getConversationActiveState", () => {
  let mockFetch: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockFetch = vi.fn();
    global.fetch = mockFetch;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("fetches conversation active state with stats fields", async () => {
    const mockResponse: ConversationActiveStateResponse = {
      is_active: true,
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
    };

    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockResponse),
    });

    const result = await getConversationActiveState("conv-123");

    expect(mockFetch).toHaveBeenCalledWith(
      "http://localhost:3847/api/conversations/conv-123/active-state"
    );
    expect(result.is_active).toBe(true);
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
      "Failed to get conversation active state: 404"
    );
  });
});
