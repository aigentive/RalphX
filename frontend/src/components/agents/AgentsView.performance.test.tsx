import {
  getAgentsViewTestMocks,
  mockAgentViewData,
  mockSessionWithData,
  renderAgentsView,
  resetAgentSessionState,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { useChatStore } from "@/stores/chatStore";

import { useAgentTerminalStore } from "./agentTerminalStore";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";

const {
  artifactPaneModuleLoadedMock,
  getAgentConversationRuntimeStatusesMock,
  getAgentConversationWorkspaceFreshnessMock,
  getAgentConversationWorkspaceMock,
  getWorkspaceChangeSummaryMock,
  getWorkspaceReviewMock,
  integratedChatPanelRenderMock,
  preloadAgentTerminalExperienceMock,
  preloadAgentsArtifactPaneMock,
  terminalDrawerUnmountMock,
} = getAgentsViewTestMocks();

function holdDeferredFrames() {
  const originalRequestAnimationFrame = window.requestAnimationFrame;
  const originalCancelAnimationFrame = window.cancelAnimationFrame;
  const callbacks = new Map<number, FrameRequestCallback>();
  let nextId = 1;

  window.requestAnimationFrame = ((callback: FrameRequestCallback) => {
    const id = nextId;
    nextId += 1;
    callbacks.set(id, callback);
    return id;
  }) as typeof window.requestAnimationFrame;
  window.cancelAnimationFrame = ((id: number) => {
    callbacks.delete(id);
  }) as typeof window.cancelAnimationFrame;

  return {
    flush() {
      const queuedCallbacks = [...callbacks.values()];
      callbacks.clear();
      for (const callback of queuedCallbacks) {
        callback(performance.now());
      }
    },
    restore() {
      callbacks.clear();
      window.requestAnimationFrame = originalRequestAnimationFrame;
      window.cancelAnimationFrame = originalCancelAnimationFrame;
    },
  };
}

describe("AgentsView performance", () => {
  beforeEach(setupAgentsViewTest);

  it("paints the collapsed terminal header before initializing the terminal", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" })
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    const host = await screen.findByTestId("agent-terminal-host-chat");
    expect(host).toHaveStyle({
      height: "36px",
      opacity: "1",
      pointerEvents: "auto",
      transition: "none",
    });
    expect(screen.getByText("Closed")).toBeInTheDocument();
    expect(preloadAgentTerminalExperienceMock).not.toHaveBeenCalled();

    const drawer = await screen.findByTestId("agent-terminal-drawer");
    expect(drawer).toHaveAttribute("data-expanded", "false");
    expect(preloadAgentTerminalExperienceMock).not.toHaveBeenCalled();
  });

  it("paints a collapsed terminal shell with the cached running status", async () => {
    const deferredFrames = holdDeferredFrames();
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" })
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });
    useAgentTerminalStore.setState({
      statusByConversationId: { "conversation-1": "running" },
    });

    try {
      renderAgentsView();

      const shellHeader = await screen.findByTestId(
        "agent-terminal-loading-shell-header"
      );
      expect(within(shellHeader).getByText("running")).toBeInTheDocument();
      expect(screen.getByTestId("agent-terminal-host-chat")).toHaveStyle({
        height: "36px",
      });
      expect(preloadAgentTerminalExperienceMock).not.toHaveBeenCalled();
    } finally {
      deferredFrames.restore();
    }
  });

  it("opens the terminal from the first-paint shell header", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" })
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    const host = await screen.findByTestId("agent-terminal-host-chat");
    expect(host).toHaveStyle({
      height: "36px",
    });

    fireEvent.click(screen.getByTestId("agent-terminal-loading-shell-header"));

    expect(host).toHaveStyle({
      height: "260px",
    });
    expect(screen.getByText("Starting terminal...")).toBeInTheDocument();
    expect(preloadAgentTerminalExperienceMock).not.toHaveBeenCalled();

    expect(await screen.findByTestId("agent-terminal-drawer")).toHaveAttribute(
      "data-expanded",
      "true"
    );
  });

  it("opens the first-paint terminal shell from keyboard activation", async () => {
    const deferredFrames = holdDeferredFrames();
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" })
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    try {
      renderAgentsView();

      const host = await screen.findByTestId("agent-terminal-host-chat");
      const shellHeader = await screen.findByTestId("agent-terminal-loading-shell-header");
      expect(host).toHaveStyle({
        height: "36px",
      });

      fireEvent.keyDown(shellHeader, { key: "Escape" });
      expect(host).toHaveStyle({
        height: "36px",
      });

      fireEvent.keyDown(shellHeader, { key: "Enter" });

      await waitFor(() =>
        expect(host).toHaveStyle({
          height: "260px",
        }),
      );
      deferredFrames.flush();
      expect(await screen.findByTestId("agent-terminal-drawer")).toHaveAttribute(
        "data-expanded",
        "true"
      );
    } finally {
      deferredFrames.restore();
    }
  });

  it("paints the artifact panel frame before hydrating the heavy pane", async () => {
    mockAgentViewData(
      conversation({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
        agentMode: "ideation",
      })
    );
    mockSessionWithData();
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    const closedPane = screen.getByTestId("agents-artifact-resizable-pane");
    expect(closedPane).toHaveStyle({
      width: "0px",
      minWidth: "0px",
      maxWidth: "0px",
      opacity: "0",
      pointerEvents: "none",
      transition: "none",
    });
    expect(preloadAgentsArtifactPaneMock).not.toHaveBeenCalled();
    expect(artifactPaneModuleLoadedMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByLabelText("Open artifacts"));

    expect(screen.getByTestId("agents-artifact-resizable-pane")).toBe(closedPane);
    expect(closedPane).toHaveStyle({
      width: "66.666667%",
      minWidth: "600px",
      maxWidth: "calc(100% - 600px)",
      opacity: "1",
      pointerEvents: "auto",
      transition: "none",
    });
    expect(screen.getByTestId("agents-artifact-pane-loading")).toBeInTheDocument();
    expect(preloadAgentsArtifactPaneMock).not.toHaveBeenCalled();
    expect(artifactPaneModuleLoadedMock).not.toHaveBeenCalled();

    await waitFor(() => expect(preloadAgentsArtifactPaneMock).toHaveBeenCalledTimes(1));
    await screen.findByTestId("agents-artifact-pane");
  });

  it("paints the terminal frame before loading the terminal runtime", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" })
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    const closedHost = await screen.findByTestId("agent-terminal-host-chat");
    expect(closedHost).toHaveStyle({
      height: "36px",
      opacity: "1",
      pointerEvents: "auto",
      transition: "none",
    });
    expect(preloadAgentTerminalExperienceMock).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(getAgentConversationWorkspaceFreshnessMock).toHaveBeenCalledWith(
        "conversation-1",
        { scope: "local" }
      )
    );
    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Commit & Publish"
      )
    );
    await new Promise((resolve) => window.setTimeout(resolve, 0));
    expect(screen.getByTestId("agent-terminal-drawer")).toHaveAttribute(
      "data-expanded",
      "false"
    );
    integratedChatPanelRenderMock.mockClear();

    fireEvent.click(screen.getByTestId("agents-terminal-toggle"));

    expect(screen.getByTestId("agent-terminal-host-chat")).toBe(closedHost);
    expect(closedHost).toHaveStyle({
      height: "260px",
      opacity: "1",
      pointerEvents: "auto",
      transition: "none",
    });
    expect(screen.getByTestId("agent-terminal-drawer")).toHaveAttribute(
      "data-expanded",
      "true"
    );
    expect(preloadAgentTerminalExperienceMock).not.toHaveBeenCalled();
    expect(integratedChatPanelRenderMock).not.toHaveBeenCalled();

    await waitFor(() => expect(preloadAgentTerminalExperienceMock).toHaveBeenCalledTimes(1));
    expect(await screen.findByTestId("agent-terminal-drawer")).toHaveAttribute(
      "data-expanded",
      "true"
    );
  });

  it("collapses the terminal frame without unmounting the heavy drawer", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" })
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "auto",
    });

    renderAgentsView();

    const host = await screen.findByTestId("agent-terminal-host-chat");
    await screen.findByTestId("agent-terminal-drawer");
    expect(host).toHaveStyle({
      height: "260px",
      opacity: "1",
      pointerEvents: "auto",
      transition: "none",
    });
    integratedChatPanelRenderMock.mockClear();

    fireEvent.click(screen.getByTestId("agents-terminal-toggle"));

    expect(host).toHaveStyle({
      height: "36px",
      opacity: "1",
      pointerEvents: "auto",
      transition: "none",
    });
    expect(screen.getByTestId("agent-terminal-drawer")).toHaveAttribute(
      "data-expanded",
      "false"
    );
    expect(integratedChatPanelRenderMock).not.toHaveBeenCalled();
    expect(terminalDrawerUnmountMock).not.toHaveBeenCalled();
  });

  it("does not re-render the chat panel when toggling the artifact pane", async () => {
    mockAgentViewData(
      conversation({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
        agentMode: "ideation",
      })
    );
    mockSessionWithData();
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    integratedChatPanelRenderMock.mockClear();

    fireEvent.click(screen.getByLabelText("Open artifacts"));

    expect(screen.getByTestId("agents-artifact-resizable-pane")).toBeInTheDocument();
    expect(screen.getByTestId("agents-artifact-pane-loading")).toBeInTheDocument();
    expect(integratedChatPanelRenderMock).not.toHaveBeenCalled();

    await screen.findByTestId("agents-artifact-pane");
    integratedChatPanelRenderMock.mockClear();

    fireEvent.click(screen.getByLabelText("Close panel"));

    expect(screen.getByTestId("agents-artifact-resizable-pane")).toHaveStyle({
      width: "0px",
      minWidth: "0px",
      maxWidth: "0px",
      opacity: "0",
      pointerEvents: "none",
    });
    expect(screen.getByTestId("agents-artifact-pane")).toBeInTheDocument();
    expect(integratedChatPanelRenderMock).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(screen.queryByTestId("agents-artifact-pane")).not.toBeInTheDocument()
    );
  });

  it("warms the artifact pane on publish shortcut intent", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    fireEvent.pointerEnter(await screen.findByTestId("agents-publish-workspace"));

    expect(preloadAgentsArtifactPaneMock).not.toHaveBeenCalled();
    await waitFor(() => expect(preloadAgentsArtifactPaneMock).toHaveBeenCalledTimes(1));
  });

  it("loads the composer diff summary after focused input is idle", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" })
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    const textbox = await screen.findByPlaceholderText(
      "Ask the agent to plan, build, debug, or review something"
    );
    fireEvent.focus(textbox);
    await new Promise((resolve) => window.setTimeout(resolve, 500));

    expect(getWorkspaceChangeSummaryMock).not.toHaveBeenCalled();
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();

    fireEvent.change(textbox, { target: { value: "typing while summary waits" } });
    await new Promise((resolve) => window.setTimeout(resolve, 500));

    expect(getWorkspaceChangeSummaryMock).not.toHaveBeenCalled();
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();

    await waitFor(
      () => expect(getWorkspaceChangeSummaryMock).toHaveBeenCalledTimes(1),
      { timeout: 2_000 }
    );
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
  });

  it("refreshes the composer diff summary while the edit agent is generating without polling full review", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" })
    );
    getWorkspaceChangeSummaryMock
      .mockResolvedValueOnce({
        supportsWorktreeModes: true,
        staged: { fileCount: 0, additions: 0, deletions: 0 },
        unstaged: { fileCount: 0, additions: 0, deletions: 0 },
      })
      .mockResolvedValue({
        supportsWorktreeModes: true,
        staged: { fileCount: 0, additions: 0, deletions: 0 },
        unstaged: { fileCount: 1, additions: 12, deletions: 3 },
      });
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": {
        conversationId: "conversation-1",
        isRunning: true,
        agentStatus: "generating",
        primarySource: "workspace",
        summaryLabel: "Agent running",
        items: [],
      },
    });
    useChatStore
      .getState()
      .setAgentStatus("project:conversation-1", "generating");

    renderAgentsView();

    expect(screen.getByTestId("agents-conversation-submit")).toHaveTextContent("Stop");

    await waitFor(() => expect(getWorkspaceChangeSummaryMock).toHaveBeenCalledTimes(1), {
      timeout: 3_000,
    });
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
    expect(
      screen.queryByTestId("diff-filter-trigger")
    ).not.toBeInTheDocument();

    await waitFor(
      () => expect(getWorkspaceChangeSummaryMock.mock.calls.length).toBeGreaterThanOrEqual(2),
      { timeout: 5_000 },
    );
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
    expect(await screen.findByTestId("diff-filter-trigger")).toHaveTextContent("Unstaged");
    expect(screen.getByTestId("agents-composer-workspace-changes-count")).toHaveTextContent(
      "1 file",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-additions")).toHaveTextContent(
      "+12",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-deletions")).toHaveTextContent(
      "−3",
    );
  }, 10_000);

});
