import {
  getAgentsViewTestMocks,
  mockAgentViewData,
  renderAgentsView,
  resetAgentSessionState,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import {
  agentProjectFixture as project,
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";
import { useStartAgentConversation } from "./useStartAgentConversation";

const {
  archiveConversationMock,
  createConversationMock,
  getPlanBranchesMock,
  integratedChatPanelRenderMock,
  listAgentConversationWorkspacesByProjectMock,
  listConversationsMock,
  listIdeationSessionsMock,
  spawnConversationSessionNamerMock,
  startAgentConversationMock,
  useConversationMock,
  useProjectAgentConversationsMock,
  useProjectsMock,
} = getAgentsViewTestMocks();

describe("AgentsView start conversation", () => {
  beforeEach(setupAgentsViewTest);

  it("defaults to the starter composer when no conversation is selected", async () => {
    mockAgentViewData();

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    expect(screen.getByTestId("agents-start-heading")).toHaveTextContent("Start your agent");
    expect(screen.getByTestId("agents-start-heading-word")).toHaveTextContent("agent");
    expect(screen.getByTestId("agents-start-project")).toBeInTheDocument();
    expect(screen.getByTestId("agents-start-base")).toBeInTheDocument();
    expect(screen.getByTestId("agent-composer-runtime-pill")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-start-new-project")).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    expect(screen.getByTestId("agents-start-mode-edit")).toBeInTheDocument();
    expect(screen.getByTestId("agents-start-new-project")).toBeInTheDocument();
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
  });

  it("restores a persisted selected conversation even when it is outside the first sidebar page", async () => {
    const restoredConversation = conversation({
      id: "conversation-restored",
      title: "Older restored agent",
      contextId: "project-1",
    });
    useProjectsMock.mockReturnValue({
      data: [project],
      isLoading: false,
    });
    useProjectAgentConversationsMock.mockReturnValue({
      data: [],
      conversations: [],
      isLoading: false,
      isSuccess: true,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useConversationMock.mockImplementation((conversationId: string | null) => ({
      data:
        conversationId === "conversation-restored"
          ? {
              conversation: restoredConversation,
              messages: [],
            }
          : null,
      isLoading: false,
    }));
    resetAgentSessionState({
      selectedProjectId: null,
      selectedConversationId: "conversation-restored",
    });

    renderAgentsView();

    expect(await screen.findByTestId("integrated-chat-panel")).toBeInTheDocument();
    expect(screen.getByTestId("agents-session-conversation-restored")).toHaveTextContent(
      "Older restored agent"
    );
  });

  it("starts a new conversation directly from the starter composer and triggers the session namer", async () => {
    const invalidateSpy = vi.spyOn(QueryClient.prototype, "invalidateQueries");
    mockAgentViewData();

    const { queryClient } = renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "fix agent landing flow" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "fix agent landing flow",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
          mode: "edit",
          base: expect.objectContaining({
            kind: "project_default",
            ref: "main",
          }),
        })
      )
    );
    await waitFor(() =>
      expect(spawnConversationSessionNamerMock).toHaveBeenCalledWith(
        "conversation-2",
        "fix agent landing flow"
      )
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(screen.queryByTestId("agents-start-composer")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-workspace-status")).toHaveTextContent(
      "agent-conversation-2"
    );
    expect(useAgentSessionStore.getState().selectedConversationId).toBe("conversation-2");
    expect(queryClient.getQueryData(["chat", "conversations", "conversation-2"])).toEqual({
      conversation: expect.objectContaining({ id: "conversation-2" }),
      messages: [
        expect.objectContaining({
          conversationId: "conversation-2",
          role: "user",
          content: "fix agent landing flow",
        }),
      ],
    });
    expect(
      queryClient.getQueryData(["agents", "conversation-workspace", "conversation-2"])
    ).toEqual(expect.objectContaining({ conversationId: "conversation-2" }));
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        queryKey: ["agents", "project-conversations", "project-1"],
      })
    );
    invalidateSpy.mockRestore();
  });

  it("renders a queued starter prompt and paused explanation when global execution is paused", async () => {
    mockAgentViewData();
    useUiStore.getState().setExecutionPaused(true);
    const queuedPrompt = `build ${"queued feature ".repeat(40)}final`;
    const pausedConversation = conversation({
      id: "conversation-paused",
      contextId: "project-1",
      title: null,
    });
    createConversationMock.mockResolvedValue(pausedConversation);
    startAgentConversationMock.mockResolvedValue({
      conversation: pausedConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-paused",
      }),
      sendResult: {
        conversationId: "conversation-paused",
        agentRunId: "",
        isNewConversation: false,
        wasQueued: true,
        queuedAsPending: false,
        queuedMessageId: "queued-paused-start",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-paused-banner")).toHaveTextContent(
        "Execution is paused"
      )
    );
    expect(screen.getByTestId("agents-start-submit")).toHaveTextContent("Queue Prompt");

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: queuedPrompt },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          conversationId: "conversation-paused",
          content: queuedPrompt,
        })
      )
    );
    await waitFor(() =>
      expect(useChatStore.getState().queuedMessages["project:conversation-paused"]).toEqual([
        expect.objectContaining({
          id: "queued-paused-start",
          content: queuedPrompt,
          isEditing: false,
        }),
      ])
    );

    const queuedEmptyState = await screen.findByTestId("agents-paused-queued-empty-state");
    expect(queuedEmptyState).toHaveTextContent("Execution is paused");
    expect(queuedEmptyState).toHaveTextContent(
      "This prompt will start when execution resumes."
    );
    const queuedPromptPreview = screen.getByTestId("agents-paused-queued-prompt");
    expect(queuedPromptPreview).not.toHaveTextContent(queuedPrompt);
    expect(queuedPromptPreview.textContent).toMatch(/^build queued feature/);
    expect(queuedPromptPreview.textContent).toMatch(/\.\.\.$/);
  });

  it("starts with the remembered runtime when the project has a valid runtime preference", async () => {
    mockAgentViewData();
    resetAgentSessionState({
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "claude",
          modelId: "opus",
          effort: "high",
        },
      },
    });

    renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "use the remembered runtime" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          providerHarness: "claude",
          modelId: "opus",
          logicalEffort: "high",
        })
      )
    );
  });

  it("falls back to the default runtime when the remembered provider is no longer valid", async () => {
    mockAgentViewData();
    resetAgentSessionState({
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "removed-provider" as never,
          modelId: "retired-model",
          effort: "high",
        },
      },
    });

    renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "recover runtime defaults" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
        })
      )
    );
  });

  it("remembers runtime changes made on the starter composer before creating a conversation", async () => {
    mockAgentViewData();

    renderAgentsView();

    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    await userEvent.click(screen.getByTestId("agents-start-provider-claude"));
    await userEvent.click(screen.getByTestId("agents-start-model-opus"));
    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    await userEvent.click(screen.getByTestId("agents-start-effort-max"));

    await waitFor(() =>
      expect(useAgentSessionStore.getState().lastRuntimeByProjectId["project-1"]).toEqual({
        provider: "claude",
        modelId: "opus",
        effort: "max",
      })
    );

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "persist this runtime" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          providerHarness: "claude",
          modelId: "opus",
          logicalEffort: "max",
        })
      )
    );
  });

  it("uses a typed custom model from the existing runtime selector popover", async () => {
    mockAgentViewData();

    renderAgentsView();

    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    const customModelInput = screen.getByTestId("agents-start-model-custom-input");
    await userEvent.clear(customModelInput);
    await userEvent.type(customModelInput, "gpt-5.6{Enter}");
    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    await userEvent.click(screen.getByTestId("agents-start-effort-high"));

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "use a future model" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          providerHarness: "codex",
          modelId: "gpt-5.6",
          logicalEffort: "high",
        })
      )
    );
  });

  it("paints the conversation shell after seeding before the heavy agent start resolves", async () => {
    mockAgentViewData();
    const seededConversation = conversation({
      id: "conversation-seeded",
      contextId: "project-1",
      title: null,
    });
    let resolveStart:
      | ((value: Awaited<ReturnType<typeof startAgentConversationMock>>) => void)
      | null = null;
    createConversationMock.mockResolvedValue(seededConversation);
    startAgentConversationMock.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      })
    );

    const { queryClient } = renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "fix agent landing flow" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(createConversationMock).toHaveBeenCalledWith("project", "project-1")
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(
      queryClient.getQueryData(["chat", "conversations", "conversation-seeded"])
    ).toEqual({
      conversation: expect.objectContaining({ id: "conversation-seeded" }),
      messages: [
        expect.objectContaining({
          conversationId: "conversation-seeded",
          role: "user",
          content: "fix agent landing flow",
        }),
      ],
    });
    expect(
      useChatStore.getState().agentStatus["project:conversation-seeded"]
    ).toBe("generating");
    expect(
      useChatStore.getState().isSending["project:conversation-seeded"]
    ).toBe(true);
    expect(
      useAgentSessionStore.getState().runtimeByConversationId["conversation-seeded"]
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    });
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "conversation-seeded"
    );
    expect(integratedChatPanelRenderMock).toHaveBeenLastCalledWith(
      expect.objectContaining({
        conversationIdOverride: "conversation-seeded",
        storeContextKeyOverride: "project:conversation-seeded",
        agentProcessContextIdOverride: "conversation-seeded",
        sendOptions: expect.objectContaining({
          conversationId: "conversation-seeded",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
        }),
      })
    );
    expect(startAgentConversationMock).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: "conversation-seeded",
        content: "fix agent landing flow",
      })
    );

    resolveStart?.({
      conversation: seededConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-seeded",
      }),
      sendResult: {
        conversationId: "conversation-seeded",
        agentRunId: "run-seeded",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });

    await waitFor(() =>
      expect(spawnConversationSessionNamerMock).toHaveBeenCalledWith(
        "conversation-seeded",
        "fix agent landing flow"
      )
    );
  });

  it("clears optimistic running state when the seeded agent start fails", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const seededConversation = conversation({
      id: "conversation-failed-start",
      contextId: "project-1",
      title: null,
    });
    createConversationMock.mockResolvedValue(seededConversation);
    startAgentConversationMock.mockRejectedValue(new Error("backend unavailable"));
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () =>
        useStartAgentConversation({
          handleAutoManagedTitle: vi.fn(),
          invalidateProjectConversations: vi.fn().mockResolvedValue(undefined),
          queryClient,
          selectConversation: vi.fn(),
          setActiveConversation: useChatStore.getState().setActiveConversation,
          setFocusedProject: vi.fn(),
          setOptimisticConversationsById: vi.fn(),
          setOptimisticSelectedConversationId: vi.fn(),
          setOptimisticWorkspacesByConversationId: vi.fn(),
          setRuntimeForConversation: vi.fn(),
        }),
      { wrapper }
    );

    await expect(
      result.current({
        projectId: "project-1",
        content: "start then fail",
        runtime: {
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        },
        mode: "edit",
        base: null,
        files: [],
      })
    ).rejects.toThrow("backend unavailable");

    expect(
      queryClient.getQueryData(["chat", "conversations", "conversation-failed-start"])
    ).toEqual({
      conversation: expect.objectContaining({ id: "conversation-failed-start" }),
      messages: [
        expect.objectContaining({
          conversationId: "conversation-failed-start",
          role: "user",
          content: "start then fail",
        }),
      ],
    });
    expect(
      useChatStore.getState().agentStatus["project:conversation-failed-start"]
    ).toBeUndefined();
    expect(
      useChatStore.getState().isSending["project:conversation-failed-start"]
    ).toBeUndefined();
  });

  it("falls back to the project default when the remembered branch selection is empty", async () => {
    mockAgentViewData();
    resetAgentSessionState({
      lastBranchBaseSelectionByProjectId: {
        "project-1": "",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
      "Project default (main)"
    );
  });

  it("renders remembered branch base options before hover and refreshes with a loading state on intent", async () => {
    mockAgentViewData();
    let resolveBranches: ((branches: unknown[]) => void) | null = null;
    getPlanBranchesMock.mockReturnValue(
      new Promise((resolve) => {
        resolveBranches = resolve;
      })
    );
    resetAgentSessionState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: [
            {
              key: "project_default:main",
              label: "Project default (main)",
              detail: "Configured project base branch",
              source: "project",
              selection: {
                kind: "project_default",
                ref: "main",
                displayName: "Project default (main)",
              },
            },
            {
              key: "local_branch:feature/cached",
              label: "feature/cached",
              detail: "Local branch",
              source: "local",
              selection: {
                kind: "local_branch",
                ref: "feature/cached",
                displayName: "feature/cached",
              },
            },
          ],
          selectedKey: "local_branch:feature/cached",
          loadedAt: "2026-05-08T00:00:00.000Z",
        },
      },
      lastBranchBaseSelectionByProjectId: {
        "project-1": "local_branch:feature/cached",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    await new Promise((resolve) => window.setTimeout(resolve, 0));

    expect(screen.getByTestId("agents-start-base")).toHaveTextContent("feature/cached");
    expect(getPlanBranchesMock).not.toHaveBeenCalled();
    expect(listIdeationSessionsMock).not.toHaveBeenCalled();
    expect(listConversationsMock).not.toHaveBeenCalled();
    expect(listAgentConversationWorkspacesByProjectMock).not.toHaveBeenCalled();

    await userEvent.click(screen.getByTestId("agents-start-base"));

    await waitFor(() => expect(getPlanBranchesMock).toHaveBeenCalledWith("project-1"));
    expect(screen.getByText("Refreshing branches...")).toBeInTheDocument();
    expect(screen.getAllByText("feature/cached").length).toBeGreaterThan(0);

    await userEvent.click(screen.getByText("Project default (main)"));
    expect(
      useAgentSessionStore.getState().lastBranchBaseSelectionByProjectId["project-1"]
    ).toBe("project_default:main");
    expect(
      useAgentSessionStore.getState().branchBaseCacheByProjectId["project-1"]?.selectedKey
    ).toBe("project_default:main");

    resolveBranches?.([]);
  });

  it("starts a chat-mode conversation from the selected base and shows its workspace", async () => {
    mockAgentViewData();
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({
        id: "conversation-chat",
        contextId: "project-1",
        title: "Branch question",
        agentMode: "chat",
      }),
      workspace: {
        conversationId: "conversation-chat",
        projectId: "project-1",
        mode: "chat",
        baseRefKind: "project_default",
        baseRef: "main",
        baseDisplayName: "Project default (main)",
        baseCommit: null,
        branchName: "ralphx/demo/agent-conversation-chat",
        worktreePath: "/tmp/ralphx/conversation-chat",
        linkedIdeationSessionId: null,
        linkedPlanBranchId: null,
        publicationPrNumber: null,
        publicationPrUrl: null,
        publicationPrStatus: null,
        publicationPushStatus: null,
        status: "active",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      sendResult: {
        conversationId: "conversation-chat",
        agentRunId: "run-chat",
        isNewConversation: true,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });

    renderAgentsView();

    await userEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    await userEvent.click(screen.getByTestId("agents-start-mode-chat"));
    expect(screen.getByTestId("agents-start-base")).toBeInTheDocument();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "what branch am I on?" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "what branch am I on?",
          mode: "chat",
          base: expect.objectContaining({
            kind: "project_default",
            ref: "main",
          }),
        })
      )
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(screen.getByTestId("agents-workspace-status")).toHaveTextContent(
      "agent-conversation-chat"
    );
  });

  it("archives the selected conversation, clears the active view, and refreshes archived counts", async () => {
    const user = userEvent.setup();
    const invalidateSpy = vi.spyOn(QueryClient.prototype, "invalidateQueries");
    mockAgentViewData();
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(await screen.findByText("Archive session"));
    await user.click(screen.getByRole("button", { name: "Archive session" }));

    await waitFor(() =>
      expect(archiveConversationMock).toHaveBeenCalledWith("conversation-1")
    );
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        queryKey: ["agents", "project-conversations", "project-1", "archived-count"],
        refetchType: "active",
      })
    );

    invalidateSpy.mockRestore();
  });

  it("uploads starter attachments against a seeded conversation before sending the first message", async () => {
    mockAgentViewData();
    createConversationMock.mockResolvedValue(
      conversation({ id: "conversation-seeded", contextId: "project-1" })
    );
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({ id: "conversation-seeded", contextId: "project-1" }),
      workspace: {
        conversationId: "conversation-seeded",
        projectId: "project-1",
        mode: "edit",
        baseRefKind: "project_default",
        baseRef: "main",
        baseDisplayName: "Project default (main)",
        baseCommit: null,
        branchName: "ralphx/demo/agent-conversation-seeded",
        worktreePath: "/tmp/ralphx/conversation-seeded",
        linkedIdeationSessionId: null,
        linkedPlanBranchId: null,
        publicationPrNumber: null,
        publicationPrUrl: null,
        publicationPrStatus: null,
        publicationPushStatus: null,
        status: "active",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      sendResult: {
        conversationId: "conversation-seeded",
        agentRunId: "run-2",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    vi.mocked(invoke).mockResolvedValue({ id: "attachment-1" });

    renderAgentsView();

    const fileInput = screen.getByTestId("attachment-file-input");
    const file = new File(["draft"], "notes.md", { type: "text/markdown" });

    fireEvent.change(fileInput, {
      target: { files: [file] },
    });
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "review this note" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(createConversationMock).toHaveBeenCalledWith("project", "project-1")
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("upload_chat_attachment", {
        input: expect.objectContaining({
          conversationId: "conversation-seeded",
          fileName: "notes.md",
          mimeType: "text/markdown",
        }),
      })
    );
    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "review this note",
          conversationId: "conversation-seeded",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
          mode: "edit",
        })
      )
    );
  });

});
