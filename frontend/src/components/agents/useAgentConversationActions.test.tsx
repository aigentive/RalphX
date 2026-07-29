import { act, renderHook } from "@testing-library/react";
import { QueryClient } from "@tanstack/react-query";
import type { SetStateAction } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";

import {
  chatApi,
  setAgentConversationMuted,
  type AgentConversationWorkspace,
  type ArchiveConversationResult,
  type ChatMessageResponse,
  type ForkAgentConversationResult,
} from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { chatKeys } from "@/hooks/useChat";
import type { Project } from "@/types/project";
import type { ChatConversation } from "@/types/chat-conversation";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { useAgentConversationActions } from "./useAgentConversationActions";

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      archiveConversation: vi.fn(),
      forkAgentConversation: vi.fn(),
      getConversation: vi.fn(),
      spawnConversationSessionNamer: vi.fn(),
    },
    setAgentConversationMuted: vi.fn(),
  };
});

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/api/ideation", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/ideation")>();
  return {
    ...actual,
    ideationApi: {
      ...actual.ideationApi,
      sessions: {
        ...actual.ideationApi.sessions,
        archive: vi.fn(),
      },
    },
  };
});

const NOW = "2026-05-22T00:00:00.000Z";

function archivedResult(conversation: AgentConversation): ArchiveConversationResult {
  return {
    conversation,
    cleanup: {
      runtimeShutdownSucceeded: true,
      cleanupClaim: "claimed",
      localCleanup: "cleaned",
      message: null,
    },
  };
}

function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });
}

function createConversation(
  overrides: Partial<ChatConversation> = {}
): ChatConversation {
  return {
    id: "parent-conversation",
    contextType: "project",
    contextId: "project-1",
    claudeSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    coordinationMode: "solo",
    upstreamProvider: null,
    providerProfile: null,
    logicalModel: null,
    effectiveModelId: null,
    logicalEffort: null,
    effectiveEffort: null,
    agentMode: "edit",
    parentConversationId: null,
    title: "Parent conversation",
    messageCount: 1,
    lastMessageAt: NOW,
    createdAt: NOW,
    updatedAt: NOW,
    archivedAt: null,
    ...overrides,
  };
}

function createMessage(
  overrides: Partial<ChatMessageResponse> = {}
): ChatMessageResponse {
  return {
    id: "message-1",
    sessionId: null,
    projectId: null,
    taskId: null,
    role: "user",
    content: "please fix the title",
    metadata: null,
    parentMessageId: null,
    conversationId: "conversation-auto-rename",
    toolCalls: null,
    contentBlocks: null,
    sender: null,
    createdAt: NOW,
    ...overrides,
  };
}

function createWorkspace(
  overrides: Partial<AgentConversationWorkspace> = {}
): AgentConversationWorkspace {
  return {
    conversationId: "child-conversation",
    projectId: "project-1",
    mode: "edit",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    baseCommit: "base-sha",
    branchName: "ralphx/test/child",
    worktreePath: "/tmp/ralphx/test/child",
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    sourcePullRequest: null,
    modeSwitchLocked: false,
    modeSwitchLockReason: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: null,
    prAutofixEnabled: false,
    prAutoMergeDesired: false,
    prAutoMergeMethod: "squash",
    prAutoMergeCurrent: null,
    prSupervisionStatus: null,
    prSupervisionSummary: null,
    prSupervisionUpdatedAt: null,
    status: "active",
    createdAt: NOW,
    updatedAt: NOW,
    ...overrides,
  };
}

function trackedSetter<T>(initialValue: T) {
  let value = initialValue;
  const setter = vi.fn((next: SetStateAction<T>) => {
    value =
      typeof next === "function"
        ? (next as (current: T) => T)(value)
        : next;
  });
  return {
    get value() {
      return value;
    },
    setter,
  };
}

function renderActions(
  queryClient = createQueryClient(),
  overrides: Partial<Parameters<typeof useAgentConversationActions>[0]> = {}
) {
  const conversations = trackedSetter<Record<string, AgentConversation>>({});
  const workspaces = trackedSetter<Record<string, AgentConversationWorkspace>>({});
  const selectedConversationId = trackedSetter<string | null>(null);
  const args = {
    activeProjectId: "project-1",
    clearAgentConversationSelection: vi.fn(),
    clearAutoManagedTitle: vi.fn(),
    closeSidebarOverlay: vi.fn(),
    findConversationById: vi.fn(() => null),
    focusedProjectId: "project-1",
    invalidateProjectConversations: vi.fn(() => Promise.resolve()),
    isSidebarOverlayOpen: false,
    projectId: "project-1",
    projects: [] as Project[],
    queryClient,
    selectConversation: vi.fn(),
    selectedConversationId: null,
    selectedProjectId: "project-1",
    setActiveConversation: vi.fn(),
    setOptimisticConversationsById: conversations.setter,
    setOptimisticWorkspacesByConversationId: workspaces.setter,
    setFocusedProject: vi.fn(),
    setStartConversationDraft: vi.fn(),
    setOptimisticSelectedConversationId: selectedConversationId.setter,
    setRuntimeForConversation: vi.fn(),
    ...overrides,
  };
  const hook = renderHook(() => useAgentConversationActions(args));
  return {
    ...hook,
    args,
    conversations,
    queryClient,
    selectedConversationId,
    workspaces,
  };
}

describe("useAgentConversationActions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("opens a project-locked Persona Builder for a project conversation", () => {
    const { args, result } = renderActions();
    const conversation = createConversation({
      contextType: "project",
      contextId: "project-1",
    });

    act(() => {
      result.current.handleStartPersonaBuilder({
        ...conversation,
        projectId: "project-1",
        ideationSessionId: null,
      });
    });

    expect(args.setStartConversationDraft).toHaveBeenCalledWith({
      projectId: "project-1",
      projectLocked: true,
      mode: "persona_builder",
    });
    expect(args.setFocusedProject).toHaveBeenCalledWith("project-1");
    expect(args.clearAgentConversationSelection).toHaveBeenCalledOnce();
  });

  it("does not open Persona Builder from a standalone conversation", () => {
    const { args, result } = renderActions();
    const conversation = createConversation({
      contextType: "standalone",
      contextId: "standalone-1",
    });

    act(() => {
      result.current.handleStartPersonaBuilder({
        ...conversation,
        projectId: null,
        ideationSessionId: null,
      });
    });

    expect(args.setStartConversationDraft).not.toHaveBeenCalled();
    expect(args.clearAgentConversationSelection).not.toHaveBeenCalled();
  });

  it("hydrates local state, workspace cache, selection, and runtime after forking", async () => {
    const parentConversation = createConversation();
    const childConversation = createConversation({
      id: "child-conversation",
      providerSessionId: "child-thread",
      providerHarness: "codex",
      logicalModel: "gpt-5.5",
      effectiveModelId: "gpt-5.5",
      logicalEffort: "high",
      effectiveEffort: "high",
      parentConversationId: "parent-conversation",
      title: "[Fork] Parent conversation",
    });
    const workspace = createWorkspace();
    const forkResult: ForkAgentConversationResult = {
      parentConversation,
      conversation: childConversation,
      workspace,
      providerSessionForked: true,
      copiedMessageCount: 2,
      copiedTimelineItemCount: 3,
    };
    vi.mocked(chatApi.forkAgentConversation).mockResolvedValueOnce(forkResult);
    const {
      args,
      conversations,
      queryClient,
      result,
      selectedConversationId,
      workspaces,
    } = renderActions();

    let returned: ForkAgentConversationResult | undefined;
    await act(async () => {
      returned = await result.current.handleForkConversation("parent-conversation");
    });

    expect(returned).toBe(forkResult);
    expect(chatApi.forkAgentConversation).toHaveBeenCalledWith("parent-conversation");
    expect(
      queryClient.getQueryData(chatKeys.conversationSummary("child-conversation"))
    ).toBe(childConversation);
    expect(
      queryClient.getQueryData(agentWorkspaceKeys.workspace("child-conversation"))
    ).toBe(workspace);
    expect(conversations.value["child-conversation"].id).toBe("child-conversation");
    expect(workspaces.value["child-conversation"]).toBe(workspace);
    expect(selectedConversationId.value).toBe("child-conversation");
    expect(args.setFocusedProject).toHaveBeenCalledWith("project-1");
    expect(args.setRuntimeForConversation).toHaveBeenCalledWith(
      "child-conversation",
      "project-1",
      {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "high",
      }
    );
    expect(args.selectConversation).toHaveBeenCalledWith(
      "project-1",
      "child-conversation"
    );
    expect(args.setActiveConversation).toHaveBeenCalledWith(
      getAgentConversationStoreKey({
        ...childConversation,
        projectId: "project-1",
        ideationSessionId: null,
      }),
      "child-conversation"
    );
    expect(args.invalidateProjectConversations).toHaveBeenCalledWith("project-1");
  });

  it("shows a toast and rethrows when forking fails", async () => {
    vi.mocked(chatApi.forkAgentConversation).mockRejectedValueOnce(
      new Error("fork failed")
    );
    const { result } = renderActions();

    await expect(
      result.current.handleForkConversation("parent-conversation")
    ).rejects.toThrow("fork failed");

    expect(toast.error).toHaveBeenCalledWith("Failed to fork conversation", {
      description: "fork failed",
      duration: 10000,
    });
  });

  it("mutes a session and unmutes it from the toast Undo action", async () => {
    const conversation: AgentConversation = {
      ...createConversation({ id: "conversation-mute", title: "Mute me" }),
      projectId: "project-1",
      ideationSessionId: null,
    };
    const { result } = renderActions();

    await act(async () => {
      await result.current.handleSetConversationMuted(conversation, true);
    });

    expect(setAgentConversationMuted).toHaveBeenCalledWith(conversation.id, true);
    expect(toast.success).toHaveBeenCalledWith(
      'Muted "Mute me" — comes back on its next change',
      expect.objectContaining({ action: expect.objectContaining({ label: "Undo" }) })
    );
    const options = vi.mocked(toast.success).mock.calls.at(-1)?.[1];
    if (!options?.action) throw new Error("Mute toast did not include Undo");
    await act(async () => {
      options.action.onClick();
    });
    expect(setAgentConversationMuted).toHaveBeenLastCalledWith(conversation.id, false);
  });

  it.each([
    [
      "failed_operational" as const,
      "Session archived. Local workspace cleanup is pending and will retry automatically.",
    ],
    [
      "failed_unsafe" as const,
      "Session archived, but RalphX refused unsafe local workspace cleanup. Review the workspace metadata before retrying.",
    ],
  ])(
    "keeps archive success visible when local cleanup returns %s",
    async (localCleanup, warning) => {
      const archivedConversation: AgentConversation = {
        ...createConversation({ id: `archive-${localCleanup}` }),
        projectId: "project-1",
        ideationSessionId: null,
      };
      vi.mocked(chatApi.archiveConversation).mockResolvedValue({
        conversation: archivedConversation,
        cleanup: {
          runtimeShutdownSucceeded: true,
          cleanupClaim: "claimed",
          localCleanup,
          message: "cleanup detail",
        },
      });
      const { args, result } = renderActions(createQueryClient(), {
        selectedConversationId: archivedConversation.id,
      });

      await act(async () => {
        await result.current.handleArchiveConversation(archivedConversation, {
          closePullRequest: false,
        });
      });

      expect(args.clearAgentConversationSelection).toHaveBeenCalledTimes(1);
      expect(args.invalidateProjectConversations).toHaveBeenCalledWith("project-1");
      expect(toast.warning).toHaveBeenCalledWith(warning);
      expect(toast.error).not.toHaveBeenCalled();
    }
  );

  it("bulk archives conversations with ideation-first ordering and one invalidation per project", async () => {
    const projectConversation: AgentConversation = {
      ...createConversation({ id: "project-conversation" }),
      projectId: "project-1",
      ideationSessionId: null,
    };
    const ideationConversation: AgentConversation = {
      ...createConversation({
        id: "ideation-conversation",
        contextType: "ideation",
        contextId: "ideation-session-1",
      }),
      projectId: "project-1",
      ideationSessionId: "ideation-session-1",
    };
    vi.mocked(chatApi.archiveConversation).mockImplementation(async (conversationId) =>
      archivedResult(
        conversationId === projectConversation.id
          ? projectConversation
          : ideationConversation
      )
    );
    vi.mocked(ideationApi.sessions.archive).mockResolvedValue(undefined);
    const { args, result } = renderActions(createQueryClient(), {
      selectedConversationId: ideationConversation.id,
    });

    let archiveResult:
      | Awaited<ReturnType<typeof result.current.handleBulkArchiveConversations>>
      | undefined;
    await act(async () => {
      archiveResult = await result.current.handleBulkArchiveConversations([
        { conversation: projectConversation, workspace: null },
        { conversation: ideationConversation, workspace: null },
      ]);
    });

    expect(chatApi.archiveConversation).toHaveBeenNthCalledWith(
      1,
      projectConversation.id,
      { closePullRequest: false }
    );
    expect(chatApi.archiveConversation).toHaveBeenNthCalledWith(
      2,
      ideationConversation.id,
      { closePullRequest: false }
    );
    expect(ideationApi.sessions.archive).toHaveBeenCalledWith("ideation-session-1");
    expect(vi.mocked(ideationApi.sessions.archive).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(chatApi.archiveConversation).mock.invocationCallOrder[1] ?? Infinity
    );
    expect(args.clearAgentConversationSelection).toHaveBeenCalledTimes(1);
    expect(args.invalidateProjectConversations).toHaveBeenCalledTimes(1);
    expect(args.invalidateProjectConversations).toHaveBeenCalledWith("project-1");
    expect(archiveResult).toEqual({
      archivedConversationIds: [projectConversation.id, ideationConversation.id],
      failedConversationIds: [],
      cleanupPendingConversationIds: [],
      cleanupUnsafeConversationIds: [],
    });
    expect(toast.success).toHaveBeenCalledWith("Archived 2 sessions");
  });

  it("continues after a bulk archive failure and returns failed rows for retry", async () => {
    const firstConversation: AgentConversation = {
      ...createConversation({ id: "conversation-success" }),
      projectId: "project-1",
      ideationSessionId: null,
    };
    const failedConversation: AgentConversation = {
      ...createConversation({
        id: "conversation-failure",
        contextId: "project-2",
        title: "Blocked by backend",
      }),
      projectId: "project-2",
      ideationSessionId: null,
    };
    vi.mocked(chatApi.archiveConversation)
      .mockResolvedValueOnce(archivedResult(firstConversation))
      .mockRejectedValueOnce(new Error("archive denied"));
    const { args, result } = renderActions();

    let archiveResult:
      | Awaited<ReturnType<typeof result.current.handleBulkArchiveConversations>>
      | undefined;
    await act(async () => {
      archiveResult = await result.current.handleBulkArchiveConversations([
        { conversation: firstConversation, workspace: null },
        { conversation: failedConversation, workspace: null },
      ]);
    });

    expect(chatApi.archiveConversation).toHaveBeenCalledTimes(2);
    expect(args.invalidateProjectConversations).toHaveBeenCalledTimes(2);
    expect(args.invalidateProjectConversations).toHaveBeenCalledWith("project-1");
    expect(args.invalidateProjectConversations).toHaveBeenCalledWith("project-2");
    expect(archiveResult).toEqual({
      archivedConversationIds: [firstConversation.id],
      failedConversationIds: [failedConversation.id],
      cleanupPendingConversationIds: [],
      cleanupUnsafeConversationIds: [],
    });
    expect(toast.success).toHaveBeenCalledWith("Archived 1 session");
    expect(toast.error).toHaveBeenCalledWith("Failed to archive 1 session", {
      description: expect.stringContaining("archive denied"),
      duration: 10000,
    });
  });

  it("archives a bulk target with an open pull request without closing the PR", async () => {
    const blockedConversation: AgentConversation = {
      ...createConversation({ id: "conversation-stale-open-pr" }),
      projectId: "project-1",
      ideationSessionId: null,
    };
    const { args, result } = renderActions();
    vi.mocked(chatApi.archiveConversation).mockResolvedValue(
      archivedResult(blockedConversation)
    );

    const archiveResult = await result.current.handleBulkArchiveConversations([
      {
        conversation: blockedConversation,
        workspace: createWorkspace({
          conversationId: blockedConversation.id,
          publicationPrNumber: 93,
          publicationPrStatus: "open",
        }),
      },
    ]);

    expect(chatApi.archiveConversation).toHaveBeenCalledWith(blockedConversation.id, {
      closePullRequest: false,
    });
    expect(args.invalidateProjectConversations).toHaveBeenCalledWith("project-1");
    expect(archiveResult).toEqual({
      archivedConversationIds: [blockedConversation.id],
      failedConversationIds: [],
      cleanupPendingConversationIds: [],
      cleanupUnsafeConversationIds: [],
    });
    expect(toast.success).toHaveBeenCalledWith("Archived 1 session");
  });

  it("reruns the session namer from the first user message and conversation provider", async () => {
    const sourceConversation = createConversation({
      id: "conversation-auto-rename",
      contextId: "project-1",
      providerHarness: "codex",
    });
    vi.mocked(chatApi.getConversation).mockResolvedValueOnce({
      conversation: sourceConversation,
      messages: [
        createMessage({
          id: "assistant-1",
          role: "assistant",
          content: "I can help.",
        }),
        createMessage({
          id: "user-1",
          content: "  please analyze the prod app logs  ",
        }),
      ],
    });
    vi.mocked(chatApi.spawnConversationSessionNamer).mockResolvedValueOnce(undefined);
    const agentConversation: AgentConversation = {
      ...sourceConversation,
      projectId: "project-1",
      ideationSessionId: null,
    };
    const { args, result } = renderActions();

    await act(async () => {
      await result.current.handleAutoRenameConversation(agentConversation);
    });

    expect(chatApi.getConversation).toHaveBeenCalledWith("conversation-auto-rename");
    expect(chatApi.spawnConversationSessionNamer).toHaveBeenCalledWith(
      "conversation-auto-rename",
      "please analyze the prod app logs",
      "codex"
    );
    expect(args.clearAutoManagedTitle).toHaveBeenCalledWith(
      "conversation-auto-rename"
    );
    expect(args.invalidateProjectConversations).toHaveBeenCalledWith("project-1");
    expect(toast.success).toHaveBeenCalledWith("Auto rename started");
  });

  it("does not spawn the session namer when no user message is available", async () => {
    const sourceConversation = createConversation({
      id: "conversation-empty-transcript",
      contextId: "project-1",
      providerHarness: "codex",
    });
    vi.mocked(chatApi.getConversation).mockResolvedValueOnce({
      conversation: sourceConversation,
      messages: [
        createMessage({
          id: "assistant-1",
          role: "assistant",
          content: "No user content here.",
        }),
      ],
    });
    const agentConversation: AgentConversation = {
      ...sourceConversation,
      projectId: "project-1",
      ideationSessionId: null,
    };
    const { result } = renderActions();

    await expect(
      act(async () => {
        await result.current.handleAutoRenameConversation(agentConversation);
      })
    ).rejects.toThrow("No user message is available for auto rename");

    expect(chatApi.spawnConversationSessionNamer).not.toHaveBeenCalled();
    expect(toast.error).toHaveBeenCalledWith(
      "No user message is available for auto rename"
    );
  });
});
