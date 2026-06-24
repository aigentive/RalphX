import { beforeEach, describe, expect, it } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";
import type { ChatConversation } from "@/types/chat-conversation";
import {
  mockChatApi,
  mockGetAgentConversationRuntimeStatuses,
  mockGetAgentRunningStates,
  mockGetConversationSummary,
  mockGetConversationTimelinePage,
  mockListAgentSidebarConversations,
  mockPrecomputeAgentConversationWorkspacePrDescription,
  resetMockChatState,
  seedMockAgentConversationWorkspace,
  seedMockConversation,
} from "./chat";
import { IDEATION_REPLAY_CONTEXTS } from "./chat-scenarios";

const conversation = (
  id: string,
  title: string | null,
  overrides: Partial<ChatConversation> = {}
): ChatConversation => ({
  id,
  contextType: "project",
  contextId: "project-1",
  claudeSessionId: null,
  providerSessionId: `thread-${id}`,
  providerHarness: "codex",
  upstreamProvider: null,
  providerProfile: null,
  title,
  messageCount: 0,
  lastMessageAt: null,
  createdAt: "2026-05-01T10:00:00Z",
  updatedAt: "2026-05-01T10:00:00Z",
  archivedAt: null,
  ...overrides,
});

const workspace = (
  conversationId: string,
  overrides: Partial<AgentConversationWorkspace> = {}
): AgentConversationWorkspace => ({
  conversationId,
  projectId: "project-1",
  mode: "edit",
  baseRefKind: "project_default",
  baseRef: "main",
  baseDisplayName: "Project default (main)",
  baseCommit: null,
  branchName: `ralphx/${conversationId}`,
  worktreePath: `/tmp/${conversationId}`,
  linkedIdeationSessionId: null,
  linkedPlanBranchId: null,
  publicationPrNumber: null,
  publicationPrUrl: null,
  publicationPrStatus: null,
  publicationPushStatus: null,
  status: "active",
  createdAt: "2026-05-01T10:00:00Z",
  updatedAt: "2026-05-01T10:00:00Z",
  ...overrides,
});

describe("mockListAgentSidebarConversations", () => {
  beforeEach(() => {
    resetMockChatState();
  });

  it("groups mock conversations by publication state with PR and branch metadata", async () => {
    const fixtures = [
      conversation("active-1", "Active run", {
        createdAt: "2026-05-06T10:00:00Z",
      }),
      conversation("draft-1", "Draft run", {
        createdAt: "2026-05-05T10:00:00Z",
      }),
      conversation("merged-1", "Merged run", {
        createdAt: "2026-05-04T10:00:00Z",
      }),
      conversation("closed-1", "Closed run", {
        createdAt: "2026-05-03T10:00:00Z",
      }),
      conversation("uncommitted-1", "Uncommitted run", {
        createdAt: "2026-05-02T10:00:00Z",
      }),
      conversation("unpushed-1", "Unpushed run", {
        createdAt: "2026-05-01T10:00:00Z",
      }),
      conversation("unpushed-2", "Second unpushed run", {
        createdAt: "2026-05-01T09:00:00Z",
      }),
    ];
    fixtures.forEach((item) => seedMockConversation(item, []));
    seedMockAgentConversationWorkspace(workspace("draft-1", { publicationPrStatus: "draft" }));
    seedMockAgentConversationWorkspace(
      workspace("merged-1", {
        publicationPrNumber: 91,
        publicationPrStatus: "merged",
      })
    );
    seedMockAgentConversationWorkspace(workspace("closed-1", { publicationPrStatus: "closed" }));
    seedMockAgentConversationWorkspace(
      workspace("uncommitted-1", { publicationPushStatus: "needs_agent" })
    );
    seedMockAgentConversationWorkspace(
      workspace("unpushed-1", { publicationPushStatus: "pending" })
    );
    seedMockAgentConversationWorkspace(
      workspace("unpushed-2", { publicationPushStatus: "description_failed" })
    );

    const result = await mockListAgentSidebarConversations({
      projectIds: [" project-1 ", "project-1", ""],
      groupBy: "publication",
      publicationStates: [
        "active",
        "draft",
        "merged",
        "closed",
        "uncommitted",
        "unpushed",
      ],
      limitPerGroup: 1,
      offsets: { unpushed: 1 },
    });

    expect(result.groups.map((group) => group.label)).toEqual([
      "Active",
      "Draft",
      "Merged",
      "Closed",
      "Uncommitted",
      "Unpushed",
    ]);
    expect(result.groups.find((group) => group.key === "merged")?.rows[0]).toMatchObject({
      refKind: "pull-request",
      refLabel: "PR #91",
      publicationState: "merged",
      publicationLabel: "merged",
    });
    expect(result.groups.find((group) => group.key === "active")?.rows[0]).toMatchObject({
      refKind: "branch",
      refLabel: "master",
      publicationState: "active",
      publicationLabel: null,
    });
    expect(result.groups.find((group) => group.key === "unpushed")).toMatchObject({
      total: 2,
      offset: 1,
      limit: 1,
      hasMore: false,
      rows: [expect.objectContaining({ conversation: expect.objectContaining({ id: "unpushed-2" }) })],
    });
  });

  it("filters archived/search rows and sorts project groups with pinned rows first", async () => {
    seedMockConversation(
      conversation("bravo", "Bravo", {
        createdAt: "2026-05-02T10:00:00Z",
      }),
      []
    );
    seedMockConversation(
      conversation("alpha", "Alpha", {
        createdAt: "2026-05-03T10:00:00Z",
      }),
      []
    );
    seedMockConversation(
      conversation("archive", "Archived Alpha", {
        archivedAt: "2026-05-04T10:00:00Z",
      }),
      []
    );
    seedMockAgentConversationWorkspace(workspace("bravo", { baseRef: "feature/bravo" }));

    const az = await mockListAgentSidebarConversations({
      projectIds: ["project-1"],
      search: " a ",
      sort: "az",
      pinnedConversationIds: ["bravo"],
    });
    expect(az.groups[0].rows.map((row) => row.conversation.id)).toEqual(["bravo", "alpha"]);
    expect(az.groups[0].rows[0]).toMatchObject({
      refKind: "branch",
      refLabel: "feature/bravo",
    });

    const za = await mockListAgentSidebarConversations({
      projectIds: ["project-1"],
      sort: "za",
      includeArchived: true,
    });
    expect(za.groups[0].rows.map((row) => row.conversation.id)).toEqual([
      "bravo",
      "archive",
      "alpha",
    ]);

    const latest = await mockListAgentSidebarConversations({
      projectIds: ["project-1"],
      sort: "latest",
    });
    expect(latest.groups[0].rows.map((row) => row.conversation.id)).toEqual([
      "alpha",
      "bravo",
    ]);

    const archivedOnly = await mockListAgentSidebarConversations({
      projectIds: ["project-1"],
      archivedOnly: true,
    });
    expect(archivedOnly.groups[0].rows.map((row) => row.conversation.id)).toEqual([
      "archive",
    ]);
  });
});

describe("mockGetAgentRunningStates", () => {
  it("returns idle bulk running states for requested context ids", async () => {
    await expect(
      mockGetAgentRunningStates("project", ["conv-1", "conv-2"])
    ).resolves.toEqual({
      "conv-1": { isRunning: false, agentStatus: "idle" },
      "conv-2": { isRunning: false, agentStatus: "idle" },
    });
  });
});

describe("mockGetAgentConversationRuntimeStatuses", () => {
  it("returns idle runtime statuses for requested conversation ids", async () => {
    await expect(
      mockGetAgentConversationRuntimeStatuses(["conv-1", "conv-2"]),
    ).resolves.toEqual({
      "conv-1": {
        conversationId: "conv-1",
        isRunning: false,
        agentStatus: "idle",
        primarySource: null,
        summaryLabel: null,
        items: [],
      },
      "conv-2": {
        conversationId: "conv-2",
        isRunning: false,
        agentStatus: "idle",
        primarySource: null,
        summaryLabel: null,
        items: [],
      },
    });
  });
});

describe("mockGetConversationSummary", () => {
  beforeEach(() => {
    resetMockChatState();
  });

  it("returns seeded conversation metadata without requiring messages", async () => {
    const seeded = conversation("summary-1", "Summary title");
    seedMockConversation(seeded, []);

    await expect(mockGetConversationSummary("summary-1")).resolves.toEqual(seeded);
    await expect(mockChatApi.getConversationSummary("summary-1")).resolves.toEqual(seeded);
  });
});

describe("mockGetConversationTimelinePage", () => {
  beforeEach(() => {
    mockChatApi.reset();
  });

  it("normalizes seeded messages into block-counted timeline pages", async () => {
    mockChatApi.seedScenario("ideation_widget_matrix");
    const conversationId = IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.conversationId;

    const newest = await mockGetConversationTimelinePage(conversationId, 3, null);
    expect(newest.items).toHaveLength(3);
    expect(newest.messages).toHaveLength(3);
    expect(newest.hasOlder).toBe(true);
    expect(newest.oldestLoadedSequence).toBeGreaterThan(1);
    expect(newest.items.map((item) => item.asMessage.id)).toEqual(
      newest.messages.map((message) => message.id)
    );

    const older = await mockGetConversationTimelinePage(
      conversationId,
      2,
      newest.oldestLoadedSequence
    );
    expect(older.items).toHaveLength(2);
    expect(older.newestLoadedSequence).toBeLessThan(
      newest.oldestLoadedSequence ?? Number.MAX_SAFE_INTEGER
    );
  });

  it("uses stable tool ids and carries usage only on the first assistant block", async () => {
    mockChatApi.seedScenario("ideation_widget_matrix");
    const conversationId = IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.conversationId;
    const { messages } = await mockChatApi.getConversation(conversationId);
    const assistant = messages.find(
      (message) => message.id === "msg-ideation-widget-assistant-1"
    );
    if (!assistant) {
      throw new Error("expected seeded assistant message");
    }

    mockChatApi.replaceMessages(conversationId, [
      {
        ...assistant,
        inputTokens: 12,
        outputTokens: 4,
        estimatedUsd: 0.02,
        contentBlocks: assistant.contentBlocks?.slice(0, 2) ?? null,
      },
    ]);

    const page = await mockGetConversationTimelinePage(conversationId, 10, null);

    expect(page.items.map((item) => item.id)).toEqual([
      "block:msg-ideation-widget-assistant-1:proposal-create-1",
      "block:msg-ideation-widget-assistant-1:proposal-update-1",
    ]);
    expect(page.items[0].toolCall).toMatchObject({
      id: "proposal-create-1",
      name: "mcp__ralphx__create_task_proposal",
    });
    expect(page.messages[0]).toMatchObject({
      parentMessageId: "msg-ideation-widget-assistant-1",
      inputTokens: 12,
      outputTokens: 4,
      estimatedUsd: 0.02,
      timelineKind: "tool_use",
      timelineSequence: 1,
    });
    expect(page.messages[1]).toMatchObject({
      inputTokens: null,
      outputTokens: null,
      estimatedUsd: null,
      timelineSequence: 2,
    });
  });
});

describe("mockPrecomputeAgentConversationWorkspacePrDescription", () => {
  beforeEach(() => {
    resetMockChatState();
  });

  it("precomputes PR descriptions only for seeded workspaces", async () => {
    seedMockAgentConversationWorkspace(workspace("conversation-1"));

    await expect(
      mockPrecomputeAgentConversationWorkspacePrDescription("conversation-1")
    ).resolves.toEqual({
      conversationId: "conversation-1",
      status: "ready",
      cacheStatus: "miss",
      reason: null,
    });

    await expect(
      mockPrecomputeAgentConversationWorkspacePrDescription("missing")
    ).resolves.toEqual({
      conversationId: "missing",
      status: "skipped",
      cacheStatus: null,
      reason: "missing_workspace",
    });
  });
});
