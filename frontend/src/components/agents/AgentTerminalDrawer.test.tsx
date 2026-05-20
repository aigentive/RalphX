import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";
import { isRalphxTerminalDockDragActive } from "@/lib/internalDragTypes";
import { AgentTerminalDrawer } from "./AgentTerminalDrawer";

const {
  listenMock,
  openAgentTerminalMock,
  closeAgentTerminalMock,
  clearAgentTerminalMock,
  resizeAgentTerminalMock,
  restartAgentTerminalMock,
  writeAgentTerminalMock,
  terminalOpenMock,
} = vi.hoisted(() => ({
  listenMock: vi.fn(),
  openAgentTerminalMock: vi.fn(),
  closeAgentTerminalMock: vi.fn(),
  clearAgentTerminalMock: vi.fn(),
  resizeAgentTerminalMock: vi.fn(),
  restartAgentTerminalMock: vi.fn(),
  writeAgentTerminalMock: vi.fn(),
  terminalOpenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

vi.mock("@/api/terminal", () => ({
  AGENT_TERMINAL_EVENT: "agent-terminal://event",
  AgentTerminalEventSchema: {
    safeParse: vi.fn(() => ({ success: false })),
  },
  DEFAULT_AGENT_TERMINAL_ID: "default",
  closeAgentTerminal: (...args: unknown[]) => closeAgentTerminalMock(...args),
  clearAgentTerminal: (...args: unknown[]) => clearAgentTerminalMock(...args),
  openAgentTerminal: (...args: unknown[]) => openAgentTerminalMock(...args),
  resizeAgentTerminal: (...args: unknown[]) => resizeAgentTerminalMock(...args),
  restartAgentTerminal: (...args: unknown[]) => restartAgentTerminalMock(...args),
  writeAgentTerminal: (...args: unknown[]) => writeAgentTerminalMock(...args),
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 100;
    rows = 24;
    loadAddon = vi.fn();
    open = terminalOpenMock;
    write = vi.fn();
    reset = vi.fn();
    clear = vi.fn();
    focus = vi.fn();
    dispose = vi.fn();
    onData = vi.fn(() => ({ dispose: vi.fn() }));
  },
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();
  },
}));

const workspace = (
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace => ({
  conversationId: "conversation-1",
  projectId: "project-1",
  mode: "edit",
  baseRefKind: "current_branch",
  baseRef: "feature/agent-screen",
  baseDisplayName: "Current branch (feature/agent-screen)",
  baseCommit: "base-sha",
  branchName: "ralphx/ralphx/agent-746b9fa7",
  worktreePath: "/tmp/ralphx/agent-conversation",
  linkedIdeationSessionId: null,
  linkedPlanBranchId: null,
  publicationPrNumber: null,
  publicationPrUrl: null,
  publicationPrStatus: null,
  publicationPushStatus: null,
  status: "active",
  createdAt: "2026-04-26T00:00:00.000Z",
  updatedAt: "2026-04-26T00:00:00.000Z",
  ...overrides,
});

describe("AgentTerminalDrawer", () => {
  let rafCallbacks: FrameRequestCallback[];

  beforeEach(() => {
    vi.useFakeTimers();
    rafCallbacks = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      rafCallbacks.push(callback);
      return rafCallbacks.length;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((handle) => {
      rafCallbacks[handle - 1] = () => undefined;
    });
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe = vi.fn();
        disconnect = vi.fn();
      },
    );

    listenMock.mockReset();
    openAgentTerminalMock.mockReset();
    closeAgentTerminalMock.mockReset();
    clearAgentTerminalMock.mockReset();
    resizeAgentTerminalMock.mockReset();
    restartAgentTerminalMock.mockReset();
    writeAgentTerminalMock.mockReset();
    terminalOpenMock.mockReset();

    listenMock.mockResolvedValue(vi.fn());
    openAgentTerminalMock.mockResolvedValue({
      status: "running",
      cwd: "/tmp/ralphx/agent-conversation",
      workspaceBranch: "ralphx/ralphx/agent-746b9fa7",
      history: "",
      updatedAt: "2026-04-26T00:00:00.000Z",
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("shows a collapsed Closed header before the terminal exists", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={false}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={vi.fn()}
          placement="auto"
          onPlacementChange={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByTestId("agent-terminal-drawer")).toHaveStyle({
      height: "36px",
    });
    expect(screen.getByText("Closed")).toBeInTheDocument();
    expect(screen.getByText("/tmp/ralphx/agent-conversation")).toBeInTheDocument();
    expect(screen.queryByText("Starting terminal...")).not.toBeInTheDocument();
    expect(terminalOpenMock).not.toHaveBeenCalled();
    expect(openAgentTerminalMock).not.toHaveBeenCalled();

    await act(async () => {
      rafCallbacks.forEach((callback) => callback(0));
      await vi.runOnlyPendingTimersAsync();
      await vi.dynamicImportSettled();
    });

    expect(terminalOpenMock).not.toHaveBeenCalled();
    expect(openAgentTerminalMock).not.toHaveBeenCalled();
  });

  it("paints the drawer shell before starting xterm hydration", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={vi.fn()}
          placement="auto"
          onPlacementChange={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByTestId("agent-terminal-drawer")).toBeInTheDocument();
    expect(screen.getByText("Starting terminal...")).toBeInTheDocument();
    expect(rafCallbacks.length).toBeGreaterThanOrEqual(1);
    expect(terminalOpenMock).not.toHaveBeenCalled();
    expect(openAgentTerminalMock).not.toHaveBeenCalled();

    await act(async () => {
      rafCallbacks[0]?.(0);
      await vi.dynamicImportSettled();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(terminalOpenMock).not.toHaveBeenCalled();
    expect(openAgentTerminalMock).not.toHaveBeenCalled();

    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
      await vi.dynamicImportSettled();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(terminalOpenMock).toHaveBeenCalled();
    expect(openAgentTerminalMock).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: "conversation-1",
        terminalId: "default",
      }),
    );
  });

  it("collapses the terminal without closing the backend session", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);
    const onCollapse = vi.fn();

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={onCollapse}
          placement="auto"
          onPlacementChange={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    fireEvent.click(screen.getByLabelText("Collapse terminal"));

    expect(onCollapse).toHaveBeenCalledTimes(1);
    expect(closeAgentTerminalMock).not.toHaveBeenCalled();
  });

  it("toggles expand state from the terminal header", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);
    const onCollapse = vi.fn();

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={onCollapse}
          placement="auto"
          onPlacementChange={vi.fn()}
          onPlacementDragStart={vi.fn()}
          onPlacementDragEnd={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    fireEvent.click(screen.getByTestId("agent-terminal-header"));

    expect(onCollapse).toHaveBeenCalledTimes(1);
    expect(closeAgentTerminalMock).not.toHaveBeenCalled();
  });

  it("toggles expand state from the terminal header keyboard action", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);
    const onExpand = vi.fn();

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={false}
          onHeightChange={vi.fn()}
          onExpand={onExpand}
          onCollapse={vi.fn()}
          placement="auto"
          onPlacementChange={vi.fn()}
          onPlacementDragStart={vi.fn()}
          onPlacementDragEnd={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    fireEvent.keyDown(screen.getByTestId("agent-terminal-header"), {
      key: "Enter",
    });

    expect(onExpand).toHaveBeenCalledTimes(1);
    expect(openAgentTerminalMock).not.toHaveBeenCalled();
  });

  it("selects terminal placement from a dropdown without toggling the header", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);
    const onPlacementChange = vi.fn();
    const onCollapse = vi.fn();

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={onCollapse}
          placement="auto"
          onPlacementChange={onPlacementChange}
          onPlacementDragStart={vi.fn()}
          onPlacementDragEnd={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    fireEvent.pointerDown(screen.getByTestId("agent-terminal-placement"), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Under chat" }));

    expect(onPlacementChange).toHaveBeenCalledWith("chat");
    expect(onCollapse).not.toHaveBeenCalled();
  });

  it("marks terminal dock drags and suppresses the follow-up header click", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);
    const onCollapse = vi.fn();
    const onPlacementDragStart = vi.fn();
    const onPlacementDragEnd = vi.fn();
    const dataTransfer = {
      effectAllowed: "none",
      setData: vi.fn(),
    };

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={onCollapse}
          placement="auto"
          onPlacementChange={vi.fn()}
          onPlacementDragStart={onPlacementDragStart}
          onPlacementDragEnd={onPlacementDragEnd}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    const header = screen.getByTestId("agent-terminal-header");
    fireEvent.keyDown(header, { key: "Escape" });
    expect(onCollapse).not.toHaveBeenCalled();

    fireEvent.dragStart(header, { dataTransfer });

    expect(isRalphxTerminalDockDragActive()).toBe(true);
    expect(onPlacementDragStart).toHaveBeenCalledTimes(1);
    expect(dataTransfer.effectAllowed).toBe("move");
    expect(dataTransfer.setData).toHaveBeenCalledWith(
      "application/x-ralphx-terminal-dock",
      "conversation-1",
    );
    expect(dataTransfer.setData).toHaveBeenCalledWith("text/plain", "conversation-1");

    fireEvent.click(header);

    expect(onCollapse).not.toHaveBeenCalled();

    fireEvent.dragEnd(header);
    fireEvent.dragEnd(header);

    expect(isRalphxTerminalDockDragActive()).toBe(false);
    expect(onPlacementDragEnd).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(150);
    });
  });

  it("opens from the header when WebKit emits a zero-distance dragstart during a click", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);
    const onExpand = vi.fn();
    const onPlacementDragStart = vi.fn();
    const onPlacementDragEnd = vi.fn();
    const dataTransfer = {
      effectAllowed: "none",
      setData: vi.fn(),
    };

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={false}
          onHeightChange={vi.fn()}
          onExpand={onExpand}
          onCollapse={vi.fn()}
          placement="auto"
          onPlacementChange={vi.fn()}
          onPlacementDragStart={onPlacementDragStart}
          onPlacementDragEnd={onPlacementDragEnd}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    const header = screen.getByTestId("agent-terminal-header");
    fireEvent.pointerDown(header, { clientX: 24, clientY: 12 });
    fireEvent.dragStart(header, { clientX: 24, clientY: 12, dataTransfer });
    fireEvent.dragEnd(header, { clientX: 24, clientY: 12 });
    fireEvent.click(header);

    expect(onExpand).toHaveBeenCalledTimes(1);
    expect(onPlacementDragStart).not.toHaveBeenCalled();
    expect(onPlacementDragEnd).not.toHaveBeenCalled();
    expect(isRalphxTerminalDockDragActive()).toBe(false);
  });

  it("hides terminal session controls while collapsed", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={false}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={vi.fn()}
          placement="auto"
          onPlacementChange={vi.fn()}
          onPlacementDragStart={vi.fn()}
          onPlacementDragEnd={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByTestId("agent-terminal-placement")).toBeInTheDocument();
    expect(screen.getByLabelText("Expand terminal")).toBeInTheDocument();
    expect(screen.queryByLabelText("Clear terminal")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Open terminal")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Close terminal")).not.toBeInTheDocument();
  });

  it("keeps expanded terminal session controls wired to clear and restart", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={vi.fn()}
          placement="auto"
          onPlacementChange={vi.fn()}
          onPlacementDragStart={vi.fn()}
          onPlacementDragEnd={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
      rafCallbacks[0]?.(0);
      await vi.runOnlyPendingTimersAsync();
      await vi.dynamicImportSettled();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Clear terminal"));
      fireEvent.click(screen.getByLabelText("Start fresh terminal session"));
      await Promise.resolve();
    });

    expect(clearAgentTerminalMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      terminalId: "default",
      deleteHistory: true,
    });
    expect(restartAgentTerminalMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      terminalId: "default",
      cols: 100,
      rows: 24,
    });
  });

  it("swallows stale Tauri terminal listener cleanup failures", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);
    const cleanupError = new TypeError(
      "undefined is not an object (evaluating 'listeners[eventId].handlerId')",
    );
    const staleUnlisten = vi.fn(() => {
      throw cleanupError;
    });
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    listenMock.mockResolvedValue(staleUnlisten);

    const { unmount } = render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={vi.fn()}
          placement="auto"
          onPlacementChange={vi.fn()}
          onPlacementDragStart={vi.fn()}
          onPlacementDragEnd={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
      rafCallbacks[0]?.(0);
      await vi.runOnlyPendingTimersAsync();
      await vi.dynamicImportSettled();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(() => unmount()).not.toThrow();
    expect(staleUnlisten).toHaveBeenCalledTimes(1);
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining("Failed to unlisten"),
      cleanupError,
    );
  });

  it("requests visual collapse before waiting for the backend terminal close", async () => {
    const dockElement = document.createElement("div");
    document.body.appendChild(dockElement);
    const onCollapse = vi.fn();
    closeAgentTerminalMock.mockReturnValue(new Promise(() => undefined));

    render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={onCollapse}
          placement="auto"
          onPlacementChange={vi.fn()}
          dockElement={dockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    fireEvent.click(screen.getByLabelText("Close terminal"));

    expect(onCollapse).toHaveBeenCalledTimes(1);
    expect(closeAgentTerminalMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      terminalId: "default",
    });
  });

  it("moves the hydrated terminal DOM to a new dock immediately without reopening xterm", async () => {
    const chatDockElement = document.createElement("div");
    const panelDockElement = document.createElement("div");
    document.body.appendChild(chatDockElement);
    document.body.appendChild(panelDockElement);

    const { rerender } = render(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={vi.fn()}
          placement="auto"
          onPlacementChange={vi.fn()}
          dockElement={chatDockElement}
        />
      </TooltipProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    const drawer = screen.getByTestId("agent-terminal-drawer");
    expect(chatDockElement).toContainElement(drawer);

    await act(async () => {
      rafCallbacks[0]?.(0);
      await vi.runOnlyPendingTimersAsync();
      await vi.dynamicImportSettled();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(terminalOpenMock).toHaveBeenCalledTimes(1);
    terminalOpenMock.mockClear();

    rerender(
      <TooltipProvider>
        <AgentTerminalDrawer
          conversationId="conversation-1"
          workspace={workspace()}
          height={220}
          expanded={true}
          onHeightChange={vi.fn()}
          onExpand={vi.fn()}
          onCollapse={vi.fn()}
          placement="auto"
          onPlacementChange={vi.fn()}
          dockElement={panelDockElement}
        />
      </TooltipProvider>,
    );

    expect(chatDockElement).not.toContainElement(drawer);
    expect(panelDockElement).toContainElement(drawer);
    expect(terminalOpenMock).not.toHaveBeenCalled();
    expect(resizeAgentTerminalMock).not.toHaveBeenCalled();

    await act(async () => {
      rafCallbacks[rafCallbacks.length - 1]?.(16);
      await vi.runOnlyPendingTimersAsync();
      await Promise.resolve();
    });

    expect(panelDockElement).toContainElement(drawer);
    expect(terminalOpenMock).not.toHaveBeenCalled();
  });
});
