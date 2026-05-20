import {
  getAgentsViewTestMocks,
  fireAgentViewNativeDragDropEvent,
  mockAgentViewData,
  renderAgentsView,
  resetAgentSessionState,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import {
  AGENT_TERMINAL_DEFAULT_HEIGHT,
  useAgentTerminalStore,
} from "./agentTerminalStore";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";

const {
  getAgentConversationWorkspaceMock,
  integratedChatPanelRenderMock,
  terminalDrawerMountMock,
  terminalDrawerUnmountMock,
} = getAgentsViewTestMocks();

describe("AgentsView terminal docks", () => {
  beforeEach(setupAgentsViewTest);

  it("clears the terminal drag-over dock when drag ownership clears", () => {
    useAgentTerminalStore.getState().setDraggingConversation("conversation-1");
    useAgentTerminalStore.getState().setDragOverDock("panel");

    useAgentTerminalStore.getState().setDraggingConversation(null);

    expect(useAgentTerminalStore.getState().draggingConversationId).toBeNull();
    expect(useAgentTerminalStore.getState().dragOverDock).toBeNull();
  });

  it("opens the artifact panel and moves an open auto terminal in the same click", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
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

    const drawer = await screen.findByTestId("agent-terminal-drawer");
    expect(drawer).toHaveAttribute("data-placement", "auto");
    await waitFor(() => expect(terminalDrawerMountMock).toHaveBeenCalledTimes(1));
    integratedChatPanelRenderMock.mockClear();

    fireEvent.click(await screen.findByTestId("agents-publish-workspace"));

    expect(screen.getByTestId("agents-artifact-resizable-pane")).toHaveStyle({
      width: "66.666667%",
      opacity: "1",
      pointerEvents: "auto",
    });
    expect(screen.getByTestId("agents-artifact-resizable-pane")).toContainElement(
      screen.getByTestId("agent-terminal-drawer")
    );
    expect(integratedChatPanelRenderMock).not.toHaveBeenCalled();
    expect(terminalDrawerMountMock).toHaveBeenCalledTimes(1);
    expect(terminalDrawerUnmountMock).not.toHaveBeenCalled();
  });

  it("closes the artifact panel and moves an open auto terminal in the same click", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "publish",
          taskMode: "graph",
        },
      },
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "auto",
    });

    renderAgentsView();

    const drawer = await screen.findByTestId("agent-terminal-drawer");
    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-resizable-pane")).toContainElement(drawer)
    );
    await waitFor(() => expect(terminalDrawerMountMock).toHaveBeenCalledTimes(1));
    integratedChatPanelRenderMock.mockClear();

    fireEvent.click(screen.getByLabelText("Close panel"));

    expect(screen.getByTestId("agents-artifact-resizable-pane")).toHaveStyle({
      width: "0px",
      minWidth: "0px",
      maxWidth: "0px",
      opacity: "0",
      pointerEvents: "none",
    });
    expect(screen.getByTestId("agent-terminal-host-chat")).toContainElement(
      screen.getByTestId("agent-terminal-drawer")
    );
    expect(integratedChatPanelRenderMock).not.toHaveBeenCalled();
    expect(terminalDrawerMountMock).toHaveBeenCalledTimes(1);
    expect(terminalDrawerUnmountMock).not.toHaveBeenCalled();
  });

  it("persists terminal dock placement from the terminal control", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "panel",
    });

    renderAgentsView();
    fireEvent.click(await screen.findByTestId("agents-publish-workspace"));
    fireEvent.click(await screen.findByTestId("agent-terminal-placement-chat"));

    expect(useAgentTerminalStore.getState().placement).toBe("chat");
  });

  it("opens the publish pane when terminal placement changes to panel", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
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
    const closedPane = await screen.findByTestId("agents-artifact-resizable-pane");
    expect(closedPane).toHaveStyle({
      width: "0px",
      minWidth: "0px",
      maxWidth: "0px",
      opacity: "0",
      pointerEvents: "none",
    });

    fireEvent.click(await screen.findByTestId("agent-terminal-placement-panel"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-resizable-pane")).toBeInTheDocument()
    );
    expect(useAgentTerminalStore.getState().placement).toBe("panel");
    expect(screen.getByTestId("agents-artifact-resizable-pane")).toContainElement(
      screen.getByTestId("agent-terminal-drawer")
    );
  });

  it("keeps the terminal drawer mounted while moving it between chat and panel docks", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
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

    const drawer = await screen.findByTestId("agent-terminal-drawer");
    await waitFor(() => expect(terminalDrawerMountMock).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("agent-terminal-host-chat")).toContainElement(drawer);

    fireEvent.click(screen.getByTestId("agent-terminal-placement-panel"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-resizable-pane")).toBeInTheDocument()
    );
    await waitFor(() =>
      expect(screen.getByTestId("agent-terminal-host-panel")).toContainElement(
        screen.getByTestId("agent-terminal-drawer")
      )
    );
    expect(terminalDrawerMountMock).toHaveBeenCalledTimes(1);
    expect(terminalDrawerUnmountMock).not.toHaveBeenCalled();
  });

  it("moves the terminal from chat to the visible artifact panel by dropping the header", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "publish",
          taskMode: "graph",
        },
      },
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "chat",
    });

    renderAgentsView();

    const dragHandle = await screen.findByTestId("agent-terminal-drag-handle");
    const panelHost = await screen.findByTestId("agent-terminal-host-panel");

    fireEvent.dragStart(dragHandle);
    expect(panelHost).toHaveAttribute("data-terminal-drop-target", "true");
    expect(panelHost).toHaveStyle({ height: `${AGENT_TERMINAL_DEFAULT_HEIGHT}px` });
    expect(
      screen.getByTestId("agent-terminal-drop-target-panel").getAttribute("style"),
    ).toContain("border-style: dashed");
    expect(
      screen.getByTestId("agent-terminal-drop-target-panel").getAttribute("style"),
    ).toContain("border-color: var(--accent-border)");

    const dataTransfer = { dropEffect: "none" };
    fireEvent.dragOver(panelHost, { dataTransfer });
    expect(dataTransfer.dropEffect).toBe("move");
    expect(
      screen.getByTestId("agent-terminal-drop-target-panel").getAttribute("style"),
    ).toContain("border-color: var(--accent-primary)");
    expect(
      screen.getByTestId("agent-terminal-drop-target-panel").getAttribute("style"),
    ).toContain("color: var(--accent-primary)");
    fireEvent.drop(panelHost);

    expect(useAgentTerminalStore.getState().placement).toBe("panel");
    expect(useAgentTerminalStore.getState().draggingConversationId).toBeNull();
  });

  it("ignores terminal dock drag events before a dock drag is active", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "publish",
          taskMode: "graph",
        },
      },
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "chat",
    });

    renderAgentsView();

    const panelHost = await screen.findByTestId("agent-terminal-host-panel");

    fireEvent.dragOver(panelHost);
    fireEvent.dragLeave(panelHost);
    fireEvent.drop(panelHost);

    expect(useAgentTerminalStore.getState().dragOverDock).toBeNull();
    expect(useAgentTerminalStore.getState().placement).toBe("chat");
  });

  it("commits the drop when drag end reaches the source before the drop target", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "publish",
          taskMode: "graph",
        },
      },
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "chat",
    });

    renderAgentsView();

    const dragHandle = await screen.findByTestId("agent-terminal-drag-handle");
    const panelHost = await screen.findByTestId("agent-terminal-host-panel");

    fireEvent.dragStart(dragHandle);
    expect(panelHost).toHaveAttribute("data-terminal-drop-target", "true");

    fireEvent.dragOver(panelHost);
    fireEvent.dragEnd(dragHandle);
    fireEvent.drop(panelHost);

    expect(useAgentTerminalStore.getState().placement).toBe("panel");
    expect(useAgentTerminalStore.getState().draggingConversationId).toBeNull();
  });

  it("keeps the terminal drop target active across internal drag leave events", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "publish",
          taskMode: "graph",
        },
      },
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "chat",
    });

    renderAgentsView();

    const dragHandle = await screen.findByTestId("agent-terminal-drag-handle");
    const panelHost = await screen.findByTestId("agent-terminal-host-panel");

    vi.useFakeTimers();
    try {
      fireEvent.dragStart(dragHandle);
      fireEvent.dragEnter(panelHost);

      expect(useAgentTerminalStore.getState().dragOverDock).toBe("panel");
      expect(
        screen.getByTestId("agent-terminal-drop-target-panel").getAttribute("style"),
      ).toContain("border-color: var(--accent-primary)");

      const activePanelHost = screen.getByTestId("agent-terminal-host-panel");
      fireEvent.dragLeave(activePanelHost);

      expect(useAgentTerminalStore.getState().dragOverDock).toBe("panel");
      expect(
        screen.getByTestId("agent-terminal-drop-target-panel").getAttribute("style"),
      ).toContain("border-color: var(--accent-primary)");

      fireEvent.dragOver(activePanelHost);
      act(() => {
        vi.advanceTimersByTime(100);
      });

      expect(useAgentTerminalStore.getState().dragOverDock).toBe("panel");

      fireEvent.dragLeave(activePanelHost);
      act(() => {
        vi.advanceTimersByTime(100);
      });

      expect(useAgentTerminalStore.getState().dragOverDock).toBeNull();
      expect(
        screen.getByTestId("agent-terminal-drop-target-panel").getAttribute("style"),
      ).toContain("border-color: var(--accent-border)");
    } finally {
      vi.useRealTimers();
    }
  });

  it("activates and commits the terminal drop target from document-level drag coordinates", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "publish",
          taskMode: "graph",
        },
      },
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "chat",
    });

    renderAgentsView();

    const dragHandle = await screen.findByTestId("agent-terminal-drag-handle");
    const dataTransfer = { dropEffect: "none" };

    fireEvent.dragStart(dragHandle);

    const dropTarget = await screen.findByTestId("agent-terminal-drop-target-panel");
    const elementFromPoint = vi.fn(() => dropTarget);
    const originalElementFromPoint = document.elementFromPoint;
    Object.defineProperty(document, "elementFromPoint", {
      configurable: true,
      value: elementFromPoint,
    });

    try {
      fireEvent.dragOver(document, { clientX: 80, clientY: 24, dataTransfer });

      expect(useAgentTerminalStore.getState().dragOverDock).toBe("panel");
      expect(dropTarget.getAttribute("style")).toContain(
        "border-color: var(--accent-primary)",
      );

      fireEvent.drop(document, { clientX: 80, clientY: 24, dataTransfer });

      expect(useAgentTerminalStore.getState().placement).toBe("panel");
      expect(useAgentTerminalStore.getState().draggingConversationId).toBeNull();
    } finally {
      if (originalElementFromPoint) {
        Object.defineProperty(document, "elementFromPoint", {
          configurable: true,
          value: originalElementFromPoint,
        });
      } else {
        Reflect.deleteProperty(document, "elementFromPoint");
      }
    }
  });

  it("activates and commits the terminal drop target from native webview drag coordinates", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "publish",
          taskMode: "graph",
        },
      },
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "chat",
    });

    renderAgentsView();

    const dragHandle = await screen.findByTestId("agent-terminal-drag-handle");
    const panelHost = await screen.findByTestId("agent-terminal-host-panel");
    vi.spyOn(panelHost, "getBoundingClientRect").mockReturnValue({
      left: 0,
      top: 0,
      right: 360,
      bottom: AGENT_TERMINAL_DEFAULT_HEIGHT,
      width: 360,
      height: AGENT_TERMINAL_DEFAULT_HEIGHT,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });

    fireEvent.dragStart(dragHandle);
    await screen.findByTestId("agent-terminal-drop-target-panel");

    await fireAgentViewNativeDragDropEvent({
      type: "over",
      position: { x: 80, y: 24 },
      paths: [],
    });

    expect(useAgentTerminalStore.getState().dragOverDock).toBe("panel");
    expect(
      screen.getByTestId("agent-terminal-drop-target-panel").getAttribute("style"),
    ).toContain("border-color: var(--accent-primary)");

    await fireAgentViewNativeDragDropEvent({
      type: "over",
      position: { x: 480, y: 24 },
      paths: [],
    });
    await fireAgentViewNativeDragDropEvent({
      type: "drop",
      position: { x: 480, y: 24 },
      paths: [],
    });
    await fireAgentViewNativeDragDropEvent({
      type: "cancel",
      paths: [],
    });

    expect(useAgentTerminalStore.getState().placement).toBe("chat");

    await fireAgentViewNativeDragDropEvent({
      type: "enter",
      position: { x: 80, y: 24 },
      paths: [],
    });
    expect(useAgentTerminalStore.getState().dragOverDock).toBe("panel");

    await fireAgentViewNativeDragDropEvent({
      type: "drop",
      position: { x: 80, y: 24 },
      paths: [],
    });

    expect(useAgentTerminalStore.getState().placement).toBe("panel");
    expect(useAgentTerminalStore.getState().draggingConversationId).toBeNull();
  });

  it("clears stale terminal dock drag state when dragging ends without a target", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "chat",
    });

    renderAgentsView();

    const dragHandle = await screen.findByTestId("agent-terminal-drag-handle");

    vi.useFakeTimers();
    try {
      fireEvent.dragStart(dragHandle);
      fireEvent.dragEnd(dragHandle);

      expect(useAgentTerminalStore.getState().draggingConversationId).toBe("conversation-1");

      fireEvent.dragStart(dragHandle);
      fireEvent.dragEnd(dragHandle);

      act(() => {
        vi.advanceTimersByTime(120);
      });

      expect(useAgentTerminalStore.getState().draggingConversationId).toBeNull();
      expect(useAgentTerminalStore.getState().dragOverDock).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("commits the placement on drag end when the browser does not emit drop", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "publish",
          taskMode: "graph",
        },
      },
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "chat",
    });

    renderAgentsView();

    const dragHandle = await screen.findByTestId("agent-terminal-drag-handle");
    const panelHost = await screen.findByTestId("agent-terminal-host-panel");

    fireEvent.dragStart(dragHandle);
    fireEvent.dragOver(panelHost);
    expect(useAgentTerminalStore.getState().dragOverDock).toBe("panel");

    fireEvent.dragEnd(dragHandle);

    expect(useAgentTerminalStore.getState().placement).toBe("panel");
    expect(useAgentTerminalStore.getState().draggingConversationId).toBeNull();
  });

  it("moves the terminal from the artifact panel back under chat by dropping the header", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "publish",
          taskMode: "graph",
        },
      },
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "panel",
    });

    renderAgentsView();

    const dragHandle = await screen.findByTestId("agent-terminal-drag-handle");
    const chatHost = await screen.findByTestId("agent-terminal-host-chat");

    fireEvent.dragStart(dragHandle);
    expect(chatHost).toHaveAttribute("data-terminal-drop-target", "true");

    fireEvent.dragOver(chatHost);
    fireEvent.drop(chatHost);

    expect(useAgentTerminalStore.getState().placement).toBe("chat");
    expect(useAgentTerminalStore.getState().draggingConversationId).toBeNull();
  });

  it("does not move the terminal to the artifact panel while that target is hidden", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      placement: "chat",
    });

    renderAgentsView();

    const dragHandle = await screen.findByTestId("agent-terminal-drag-handle");
    const panelHost = await screen.findByTestId("agent-terminal-host-panel");

    fireEvent.dragStart(dragHandle);
    expect(panelHost).toHaveAttribute("data-terminal-drop-target", "false");

    fireEvent.drop(panelHost);

    expect(useAgentTerminalStore.getState().placement).toBe("chat");
  });

});
