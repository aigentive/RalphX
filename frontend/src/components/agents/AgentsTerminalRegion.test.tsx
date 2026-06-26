import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { conversationWorkspaceFixture as conversationWorkspace } from "./agentsTestFixtures";
import { AgentsTerminalRegion } from "./AgentsTerminalRegion";
import { useAgentTerminalStore } from "./agentTerminalStore";

const { preloadAgentTerminalDrawerMock } = vi.hoisted(() => ({
  preloadAgentTerminalDrawerMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn(async () => vi.fn()),
  }),
}));

vi.mock("./agentTerminalPreload", () => ({
  preloadAgentTerminalDrawer: (...args: unknown[]) =>
    preloadAgentTerminalDrawerMock(...args),
}));

describe("AgentsTerminalRegion", () => {
  beforeEach(() => {
    preloadAgentTerminalDrawerMock.mockResolvedValue({
      AgentTerminalDrawer: () => <div data-testid="agent-terminal-drawer" />,
    });
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": true },
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      statusByConversationId: {},
      placement: "auto",
      draggingConversationId: null,
      dragOverDock: null,
    });
  });

  afterEach(() => {
    window.localStorage.clear();
    preloadAgentTerminalDrawerMock.mockReset();
    useAgentTerminalStore.setState({
      openByConversationId: {},
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      statusByConversationId: {},
      placement: "auto",
      draggingConversationId: null,
      dragOverDock: null,
    });
  });

  it("paints an archived terminal shell without loading the drawer", () => {
    render(
      <AgentsTerminalRegion
        conversationId="conversation-1"
        workspace={conversationWorkspace({ publicationPrStatus: "merged" })}
        terminalUnavailableReason={null}
        terminalArchivedReason="Workspace archived after PR merge. Send a follow-up to continue in a fresh workspace."
        hasAutoOpenArtifacts={false}
        chatDockElement={null}
        panelDockElement={null}
        onOpenArtifactTab={vi.fn()}
      />,
    );

    expect(screen.getByTestId("agent-terminal-archived-shell")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Workspace archived after PR merge. Send a follow-up to continue in a fresh workspace.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("agent-terminal-drawer")).not.toBeInTheDocument();
    expect(preloadAgentTerminalDrawerMock).not.toHaveBeenCalled();
  });

  it("keeps archived terminal content collapsed until expanded", () => {
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": false },
    });

    render(
      <AgentsTerminalRegion
        conversationId="conversation-1"
        workspace={conversationWorkspace({ publicationPrStatus: "closed" })}
        terminalUnavailableReason={null}
        terminalArchivedReason="Workspace archived after PR close. Send a follow-up to continue in a fresh workspace."
        hasAutoOpenArtifacts={false}
        chatDockElement={null}
        panelDockElement={null}
        onOpenArtifactTab={vi.fn()}
      />,
    );

    expect(screen.getByTestId("agent-terminal-archived-shell")).toBeInTheDocument();
    expect(screen.getByText("Archived")).toBeInTheDocument();
    expect(
      screen.queryByText(
        "Workspace archived after PR close. Send a follow-up to continue in a fresh workspace.",
      ),
    ).not.toBeInTheDocument();
    expect(preloadAgentTerminalDrawerMock).not.toHaveBeenCalled();
  });

  it("toggles the archived terminal shell with keyboard activation", () => {
    useAgentTerminalStore.setState({
      openByConversationId: { "conversation-1": false },
    });

    render(
      <AgentsTerminalRegion
        conversationId="conversation-1"
        workspace={conversationWorkspace({ publicationPrStatus: "merged" })}
        terminalUnavailableReason={null}
        terminalArchivedReason="Workspace archived after PR merge. Send a follow-up to continue in a fresh workspace."
        hasAutoOpenArtifacts={false}
        chatDockElement={null}
        panelDockElement={null}
        onOpenArtifactTab={vi.fn()}
      />,
    );

    const header = screen.getByTestId("agent-terminal-archived-shell-header");
    fireEvent.keyDown(header, { key: "Escape" });
    expect(useAgentTerminalStore.getState().openByConversationId["conversation-1"]).toBe(
      false,
    );

    fireEvent.keyDown(header, { key: "Enter" });

    expect(useAgentTerminalStore.getState().openByConversationId["conversation-1"]).toBe(
      true,
    );
    expect(preloadAgentTerminalDrawerMock).not.toHaveBeenCalled();
  });
});
