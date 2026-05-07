import {
  getAgentsViewTestMocks,
  mockAgentViewData,
  mockSessionWithData,
  renderAgentsView,
  resetAgentSessionState,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { useAgentTerminalStore } from "./agentTerminalStore";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";

const {
  artifactPaneModuleLoadedMock,
  getAgentConversationWorkspaceFreshnessMock,
  getAgentConversationWorkspaceMock,
  integratedChatPanelRenderMock,
  preloadAgentTerminalExperienceMock,
  preloadAgentsArtifactPaneMock,
  terminalDrawerModuleLoadedMock,
} = getAgentsViewTestMocks();

describe("AgentsView performance", () => {
  beforeEach(setupAgentsViewTest);

  it("does not load the terminal drawer module until the terminal opens", async () => {
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
    expect(terminalDrawerModuleLoadedMock).not.toHaveBeenCalled();
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

  it("paints the terminal frame before loading the heavy drawer", async () => {
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
      height: "0px",
      opacity: "0",
      pointerEvents: "none",
      transition: "none",
    });
    expect(preloadAgentTerminalExperienceMock).not.toHaveBeenCalled();
    expect(terminalDrawerModuleLoadedMock).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(getAgentConversationWorkspaceFreshnessMock).toHaveBeenCalledWith(
        "conversation-1"
      )
    );
    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Commit & Publish"
      )
    );
    await new Promise((resolve) => window.setTimeout(resolve, 0));
    integratedChatPanelRenderMock.mockClear();

    fireEvent.click(screen.getByTestId("agents-terminal-toggle"));

    expect(screen.getByTestId("agent-terminal-host-chat")).toBe(closedHost);
    expect(closedHost).toHaveStyle({
      height: "260px",
      opacity: "1",
      pointerEvents: "auto",
      transition: "none",
    });
    expect(screen.getByTestId("agent-terminal-loading-shell")).toBeInTheDocument();
    expect(preloadAgentTerminalExperienceMock).not.toHaveBeenCalled();
    expect(terminalDrawerModuleLoadedMock).not.toHaveBeenCalled();
    expect(integratedChatPanelRenderMock).not.toHaveBeenCalled();

    await waitFor(() => expect(preloadAgentTerminalExperienceMock).toHaveBeenCalledTimes(1));
    await screen.findByTestId("agent-terminal-drawer");
  });

  it("collapses the terminal frame before unmounting the heavy drawer", async () => {
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
      height: "0px",
      opacity: "0",
      pointerEvents: "none",
      transition: "none",
    });
    expect(screen.getByTestId("agent-terminal-drawer")).toBeInTheDocument();
    expect(integratedChatPanelRenderMock).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(screen.queryByTestId("agent-terminal-drawer")).not.toBeInTheDocument()
    );
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

});
