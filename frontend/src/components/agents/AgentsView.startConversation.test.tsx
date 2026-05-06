import {
  getAgentsViewTestMocks,
  mockAgentViewData,
  renderAgentsView,
  resetAgentSessionState,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { QueryClient } from "@tanstack/react-query";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import { useAgentSessionStore } from "@/stores/agentSessionStore";
import {
  agentProjectFixture as project,
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";

const {
  archiveConversationMock,
  createConversationMock,
  getPlanBranchesMock,
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
      selectedConversationId: null,
      lastSelectedConversationByProjectId: {
        "project-1": "conversation-restored",
      },
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
      messages: [],
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

    renderAgentsView();

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
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "conversation-seeded"
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

  it("does not hydrate branch base options until the starter base picker gets intent", async () => {
    mockAgentViewData();

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    await new Promise((resolve) => window.setTimeout(resolve, 0));

    expect(getPlanBranchesMock).not.toHaveBeenCalled();
    expect(listIdeationSessionsMock).not.toHaveBeenCalled();
    expect(listConversationsMock).not.toHaveBeenCalled();
    expect(listAgentConversationWorkspacesByProjectMock).not.toHaveBeenCalled();

    fireEvent.pointerEnter(screen.getByTestId("agents-start-base"));

    await waitFor(() => expect(getPlanBranchesMock).toHaveBeenCalledWith("project-1"));
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
