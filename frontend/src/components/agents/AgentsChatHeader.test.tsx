import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { chatApi, type ConversationStatsResponse } from "@/api/chat";
import { useConversationStats } from "@/hooks/useConversationStats";
import { useConversationTicket } from "@/hooks/useTicketing";
import { useChatStore } from "@/stores/chatStore";
import { useProjectStore } from "@/stores/projectStore";
import { useTicketingStore } from "@/stores/ticketingStore";
import { useUiStore } from "@/stores/uiStore";
import { toast } from "sonner";
import { AgentsChatFocusBar, AgentsChatHeader } from "./AgentsChatHeader";
import { AgentsChatHeaderController } from "./AgentsChatHeaderController";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
  renderWithAgentProviders as renderWithProviders,
} from "./agentsTestFixtures";

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
  },
}));

vi.mock("@/hooks/useTicketing", () => ({
  useConversationTicket: vi.fn(),
}));

vi.mock("@/hooks/useConversationStats", () => ({
  useConversationStats: vi.fn(),
}));

function conversationStats(
  overrides: Partial<ConversationStatsResponse> = {},
): ConversationStatsResponse {
  return {
    conversationId: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    providerHarness: "codex",
    upstreamProvider: null,
    providerProfile: null,
    messageUsageTotals: {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      processedTokens: null,
      estimatedUsd: null,
    },
    runUsageTotals: {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      processedTokens: null,
      estimatedUsd: null,
    },
    effectiveUsageTotals: {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      processedTokens: null,
      estimatedUsd: null,
    },
    usageCoverage: {
      providerMessageCount: 0,
      providerMessagesWithUsage: 0,
      runCount: 0,
      runsWithUsage: 0,
      effectiveRunConversationCount: 0,
      effectiveMessageConversationCount: 0,
      legacyEstimatedSampleCount: 0,
      fallbackEstimatedSampleCount: 0,
      uncountedSampleCount: 0,
      effectiveTotalsSource: "none",
    },
    attributionCoverage: {
      providerMessageCount: 0,
      providerMessagesWithAttribution: 0,
      runCount: 0,
      runsWithAttribution: 0,
    },
    byHarness: [],
    byUpstreamProvider: [],
    byModel: [],
    byEffort: [],
    ...overrides,
  };
}

describe("AgentsChatHeader", () => {
  beforeEach(() => {
    vi.mocked(useConversationTicket).mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationTicket>);
    vi.mocked(useConversationStats).mockReturnValue({
      data: conversationStats(),
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationStats>);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
    window.localStorage.clear();
    act(() => {
      useChatStore.setState({ agentStatus: {}, isSending: {} });
      useTicketingStore.getState().reset();
      useProjectStore.setState({ activeProjectId: null });
      useUiStore.setState({ currentView: "agents" });
    });
  });

  it("opens the linked ticket in the artifact sidebar from the header ticket button", () => {
    vi.mocked(useConversationTicket).mockReturnValue({
      data: {
        ticketRef: { provider: "linear", id: "LIN-1", key: "LIN-1" },
        projectId: "project-2",
        title: "Fix Linear tickets",
        url: null,
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationTicket>);

    const onSelectArtifact = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-linked", projectId: "project-2" })}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={onSelectArtifact}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open ticket LIN-1" }));

    // Opens the Linear issue tab in the artifact sidebar instead of navigating away.
    expect(onSelectArtifact).toHaveBeenCalledWith("linear");
    expect(useUiStore.getState().currentView).toBe("agents");
  });

  it("opens the linked jira ticket in the jira artifact tab", () => {
    vi.mocked(useConversationTicket).mockReturnValue({
      data: {
        ticketRef: { provider: "jira", id: "10001", key: "RX-42" },
        projectId: "project-2",
        title: "Fix Jira tickets",
        url: null,
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationTicket>);

    const onSelectArtifact = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-jira", projectId: "project-2" })}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={onSelectArtifact}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open ticket RX-42" }));

    expect(onSelectArtifact).toHaveBeenCalledWith("jira");
  });

  it("opens the linked ClickUp ticket in the ClickUp artifact tab", () => {
    vi.mocked(useConversationTicket).mockReturnValue({
      data: {
        ticketRef: { provider: "clickup", id: "task-1", key: "TASK-1" },
        projectId: "project-2",
        title: "Fix ClickUp tickets",
        url: null,
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationTicket>);

    const onSelectArtifact = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-clickup", projectId: "project-2" })}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={onSelectArtifact}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open ticket TASK-1" }));

    expect(onSelectArtifact).toHaveBeenCalledWith("clickup");
  });

  it("does not render the linked ticket button when no ticket is linked", () => {
    vi.mocked(useConversationTicket).mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationTicket>);

    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(
      screen.queryByTestId("agents-linked-ticket-button"),
    ).not.toBeInTheDocument();
  });

  it("falls back to the ticket key in the aria-label when title is missing", () => {
    vi.mocked(useConversationTicket).mockReturnValue({
      data: {
        ticketRef: { provider: "linear", id: "lin-uuid", key: "ENG-7" },
        projectId: "project-1",
        title: null,
        url: null,
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationTicket>);

    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(
      screen.getByRole("button", { name: "Open ticket ENG-7" }),
    ).toBeInTheDocument();
  });

  it("falls back to the ticket id in the aria-label when title and key are missing", () => {
    vi.mocked(useConversationTicket).mockReturnValue({
      data: {
        ticketRef: { provider: "linear", id: "lin-uuid-only" },
        projectId: "project-1",
        title: null,
        url: null,
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationTicket>);

    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(
      screen.getByRole("button", { name: "Open ticket lin-uuid-only" }),
    ).toBeInTheDocument();
  });

  it("enables the linked ticket query only when a conversation exists", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-enabled" })}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(useConversationTicket).toHaveBeenCalledWith("conversation-enabled", {
      enabled: true,
    });
  });

  it("disables the linked ticket query when there is no conversation", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={null}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(useConversationTicket).toHaveBeenCalledWith(undefined, {
      enabled: false,
    });
  });

  it("opts the title button out of the high-contrast default button border", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-chat-title-button")).toHaveAttribute(
      "data-theme-button-skip",
      "true"
    );
  });

  it("can hide the inline title when the breadcrumb owns rename", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
        showTitle={false}
      />
    );

    expect(screen.queryByTestId("agents-chat-title-button")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-chat-title-group")).toBeInTheDocument();
  });

  it("renames from the legacy inline title path when it is visible", async () => {
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ title: "Old title" })}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={onRenameConversation}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Edit agent title" }));
    const input = screen.getByRole("textbox", { name: "Agent title" });
    fireEvent.change(input, { target: { value: "New title" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() =>
      expect(onRenameConversation).toHaveBeenCalledWith("conversation-1", "New title")
    );
  });

  it("cancels the legacy inline title editor on Escape", () => {
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ title: "Stable title" })}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={onRenameConversation}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Edit agent title" }));
    const input = screen.getByRole("textbox", { name: "Agent title" });
    fireEvent.change(input, { target: { value: "Discarded title" } });
    fireEvent.keyDown(input, { key: "Escape" });

    expect(onRenameConversation).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Edit agent title" })).toHaveTextContent(
      "Stable title"
    );
  });

  it("hides artifact shortcut buttons while the artifact pane is open", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "ideation" })}
        workspace={conversationWorkspace({ mode: "ideation" })}
        artifactOpen
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.queryByLabelText("Plan")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Close panel")).toBeInTheDocument();
  });

  it("does not render redundant runtime metadata in the title area", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.queryByText("Provider")).not.toBeInTheDocument();
    expect(screen.queryByText("Model")).not.toBeInTheDocument();
    expect(screen.queryByText("Mode")).not.toBeInTheDocument();
    expect(screen.queryByText("Default")).not.toBeInTheDocument();
  });

  it("shows only conversation stats in the Agents chat header chips", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        modelDisplay={{ id: "gpt-5.4", label: "gpt-5.4" }}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("chat-session-chips")).toBeInTheDocument();
    expect(screen.getByTestId("chat-session-stats-button")).toBeInTheDocument();
    expect(screen.queryByTestId("chat-session-provider-badge")).not.toBeInTheDocument();
    expect(screen.queryByText("gpt-5.4")).not.toBeInTheDocument();
  });

  it("keeps the workspace chat header neutral without a focus badge", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        chatFocus={{ type: "workspace" }}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.queryByTestId("agents-chat-focus-badge")).not.toBeInTheDocument();
  });

  it("keeps ideation focus out of the primary title row", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        chatFocus={{
          type: "ideation",
          conversationId: "conversation-1",
          sessionId: "session-child",
        }}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-chat-header")).toHaveAttribute(
      "data-focus-type",
      "ideation",
    );
    expect(screen.queryByTestId("agents-chat-focus-badge")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-chat-title-group")).not.toHaveClass(
      "border-l-2",
    );
    expect(screen.queryByTestId("agents-chat-focus-return")).not.toBeInTheDocument();
  });

  it("shows a back to workspace chat action for child chat focus", async () => {
    const user = userEvent.setup();
    const onBackToWorkspaceChat = vi.fn();

    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        chatFocus={{ type: "workspace_review", conversationId: "review-child" }}
        artifactOpen={false}
        activeArtifactTab="review"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
        onBackToWorkspaceChat={onBackToWorkspaceChat}
      />
    );

    const backButton = screen.getByRole("button", {
      name: "Back to Workspace Chat",
    });
    expect(backButton).toBeInTheDocument();

    await user.click(backButton);

    expect(onBackToWorkspaceChat).toHaveBeenCalledTimes(1);
  });

  it.each([
    { type: "workspace_repair" as const, conversationId: "repair-child" },
    { type: "pr_fixer" as const, conversationId: "pr-fixer-child" },
  ])("shows a back action for $type focus", (chatFocus) => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        chatFocus={chatFocus}
        artifactOpen={false}
        activeArtifactTab="review"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
        onBackToWorkspaceChat={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: "Back to Workspace Chat" }),
    ).toBeInTheDocument();
  });

  it("keeps verification focus out of the primary title row", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        chatFocus={{
          type: "verification",
          conversationId: "conversation-1",
          parentSessionId: "session-parent",
          childSessionId: "verification-child",
        }}
        artifactOpen={false}
        activeArtifactTab="verification"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-chat-header")).toHaveAttribute(
      "data-focus-type",
      "verification",
    );
    expect(screen.queryByTestId("agents-chat-focus-badge")).not.toBeInTheDocument();
  });

  it("constrains long focused-chat titles so header controls remain reachable", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({
          title:
            "Add execution bar to Agents screen layout with enough words to collide with header buttons",
        })}
        workspace={null}
        chatFocus={{
          type: "verification",
          conversationId: "conversation-1",
          parentSessionId: "session-parent",
          childSessionId: "verification-child",
        }}
        artifactOpen
        activeArtifactTab="verification"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-chat-title-button")).toHaveClass(
      "max-w-full",
    );
    expect(screen.getByTestId("agents-terminal-toggle")).toBeInTheDocument();
    expect(screen.getByLabelText("Close panel")).toBeInTheDocument();
  });

  it("renders child chat navigation in a separate focus switcher row", () => {
    const onSelectFocus = vi.fn();

    renderWithProviders(
      <AgentsChatFocusBar
        activeType="verification"
        options={[
          {
            type: "workspace",
            label: "Workspace",
            description: "Show the workspace agent chat",
          },
          {
            type: "ideation",
            label: "Ideation",
            description: "Show the attached ideation chat",
            tone: "accent",
          },
          {
            type: "verification",
            label: "Verification",
            description: "Show the verification agent chat",
            tone: "warning",
          },
        ]}
        onSelectFocus={onSelectFocus}
      />,
    );

    expect(screen.getByTestId("agents-chat-focus-bar")).not.toHaveAttribute("style");
    fireEvent.click(screen.getByTestId("agents-chat-focus-trigger"));
    expect(screen.getByTestId("agents-chat-focus-return")).toHaveAttribute(
      "data-active",
      "false",
    );
    expect(screen.getByTestId("agents-chat-focus-option-verification")).toHaveAttribute(
      "data-active",
      "true",
    );

    fireEvent.click(screen.getByTestId("agents-chat-focus-option-ideation"));

    expect(onSelectFocus).toHaveBeenCalledWith("ideation");
  });

  it("adds an opaque chat background to the focus switcher in split surfaces", () => {
    renderWithProviders(
      <AgentsChatFocusBar
        activeType="workspace"
        options={[
          {
            type: "workspace",
            label: "Workspace",
            description: "Show the workspace agent chat",
          },
          {
            type: "ideation",
            label: "Ideation",
            description: "Show the attached ideation chat",
            tone: "accent",
          },
        ]}
        surfaceBackground
        onSelectFocus={vi.fn()}
      />,
    );

    expect(screen.getByTestId("agents-chat-focus-bar")).toHaveStyle({
      backgroundColor: "var(--bg-base)",
    });
  });

  it("marks conversation stats as pending while the active Agents turn has no usage yet", async () => {
    vi.mocked(useConversationStats).mockReturnValue({
      data: conversationStats(),
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationStats>);
    act(() => {
      useChatStore
        .getState()
        .setAgentStatus("project:conversation-1", "generating");
    });

    const user = userEvent.setup();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        modelDisplay={{ id: "gpt-5.4", label: "gpt-5.4" }}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    await user.click(screen.getByTestId("chat-session-stats-button"));

    expect(
      await screen.findByText(
        "Usage totals are pending until the provider reports the current turn.",
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Pending")).toHaveLength(5);

    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(
        screen.queryByText(
          "Usage totals are pending until the provider reports the current turn.",
        ),
      ).not.toBeInTheDocument();
    });
  });

  it("shows workspace status in the left header group", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-chat-title-group")).toContainElement(
      screen.getByTestId("agents-workspace-status")
    );
    expect(screen.getByTestId("agents-workspace-status")).toHaveTextContent(
      "agent-abcdef12"
    );
    expect(screen.getByTestId("agents-workspace-status")).not.toHaveClass("border");
    expect(screen.getByTestId("agents-workspace-status")).toHaveStyle({
      background: "transparent",
    });
    expect(screen.getByTestId("chat-session-chips")).toBeInTheDocument();
  });

  it("renders a provided workspace control instead of the default status pill", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
        workspaceControl={
          <div data-testid="agents-header-workspace-control">BASE: main</div>
        }
      />,
    );

    expect(screen.getByTestId("agents-chat-title-group")).toContainElement(
      screen.getByTestId("agents-header-workspace-control"),
    );
    expect(
      screen.queryByTestId("agents-workspace-status"),
    ).not.toBeInTheDocument();
  });

  it("shows the workspace branch status inside the focus subheader", () => {
    renderWithProviders(
      <AgentsChatFocusBar
        activeType="workspace"
        options={[
          {
            type: "workspace",
            label: "Workspace",
            description: "Show the workspace agent chat",
          },
        ]}
        workspace={{
          conversationId: "conversation-1",
          projectId: "project-1",
          mode: "edit",
          baseRefKind: "project_default",
          baseRef: "main",
          baseDisplayName: "Project default (main)",
          baseCommit: null,
          branchName: "ralphx/ralphx/agent-abcdef12",
          worktreePath: "/tmp/ralphx/conversation-1",
          linkedIdeationSessionId: null,
          linkedPlanBranchId: null,
          publicationPrNumber: null,
          publicationPrUrl: null,
          publicationPrStatus: null,
          publicationPushStatus: null,
          status: "active",
          createdAt: "2026-04-23T09:00:00Z",
          updatedAt: "2026-04-23T09:00:00Z",
        }}
        onSelectFocus={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-workspace-status")).toHaveTextContent(
      "agent-abcdef12"
    );
    expect(screen.getByTestId("agents-workspace-status")).toHaveTextContent("active");
  });

  it("shows a commit and publish shortcut for editable workspaces", () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    const openPublishPane = vi.fn();
    const openWorkspaceTarget = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1" })}
        workspace={{
          conversationId: "conversation-1",
          projectId: "project-1",
          mode: "edit",
          baseRefKind: "project_default",
          baseRef: "main",
          baseDisplayName: "Project default (main)",
          baseCommit: null,
          branchName: "ralphx/ralphx/agent-abcdef12",
          worktreePath: "/tmp/ralphx/conversation-1",
          linkedIdeationSessionId: null,
          linkedPlanBranchId: null,
          publicationPrNumber: null,
          publicationPrUrl: null,
          publicationPrStatus: null,
          publicationPushStatus: null,
          status: "active",
          createdAt: "2026-04-23T09:00:00Z",
          updatedAt: "2026-04-23T09:00:00Z",
        }}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={publish}
        onOpenPublishPane={openPublishPane}
        workspaceOpenTargets={[
          { id: "cursor", label: "Cursor", kind: "editor" },
          { id: "file-manager", label: "Finder", kind: "fileManager" },
        ]}
        onOpenWorkspaceTarget={openWorkspaceTarget}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    const openWorkspace = screen.getByTestId("agents-open-workspace");
    expect(screen.getByTestId("agents-open-workspace-current-target")).toHaveTextContent(
      "Cursor"
    );
    const publishWorkspace = screen.getByTestId("agents-publish-workspace");
    expect(
      openWorkspace.compareDocumentPosition(publishWorkspace) &
        Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();

    fireEvent.click(openWorkspace);
    expect(openWorkspaceTarget).toHaveBeenCalledWith("cursor");

    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    expect(openPublishPane).toHaveBeenCalledTimes(1);
    expect(publish).not.toHaveBeenCalled();
  });

  it("shows the commit and publish shortcut for linked edit workspaces", () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    const openPublishPane = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({
          id: "conversation-1",
          agentMode: "edit",
        })}
        workspace={conversationWorkspace({
          mode: "edit",
          linkedIdeationSessionId: "planning-session-1",
        })}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={publish}
        onOpenPublishPane={openPublishPane}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    expect(openPublishPane).toHaveBeenCalledTimes(1);
    expect(publish).not.toHaveBeenCalled();
  });

  it("opens a selected workspace target from the header dropdown", () => {
    const openWorkspaceTarget = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        workspaceOpenTargets={[
          { id: "cursor", label: "Cursor", kind: "editor" },
          { id: "iterm2", label: "iTerm2", kind: "terminal" },
          { id: "terminal", label: "Terminal", kind: "terminal" },
          { id: "file-manager", label: "Finder", kind: "fileManager" },
        ]}
        onOpenWorkspaceTarget={openWorkspaceTarget}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={vi.fn().mockResolvedValue(undefined)}
        onOpenPublishPane={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.pointerDown(screen.getByTestId("agents-open-workspace-options"), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(screen.getByRole("menuitem", { name: "Finder" }));

    expect(openWorkspaceTarget).toHaveBeenCalledWith("file-manager");
    expect(screen.getByTestId("agents-open-workspace-current-target")).toHaveTextContent(
      "Finder"
    );

    fireEvent.click(screen.getByTestId("agents-open-workspace"));
    expect(openWorkspaceTarget).toHaveBeenLastCalledWith("file-manager");
    expect(window.localStorage.getItem("ralphx:agents:preferred-workspace-open-target")).toBe(
      "file-manager"
    );
  });

  it("opens an external terminal target and persists it as the external preference", () => {
    const openWorkspaceTarget = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        workspaceOpenTargets={[
          { id: "cursor", label: "Cursor", kind: "editor" },
          { id: "iterm2", label: "iTerm2", kind: "terminal" },
          { id: "terminal", label: "Terminal", kind: "terminal" },
          { id: "file-manager", label: "Finder", kind: "fileManager" },
        ]}
        onOpenWorkspaceTarget={openWorkspaceTarget}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.pointerDown(screen.getByTestId("agents-open-workspace-options"), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(screen.getByRole("menuitem", { name: "Terminal" }));

    expect(openWorkspaceTarget).toHaveBeenCalledWith("terminal");
    expect(screen.getByTestId("agents-open-workspace-current-target")).toHaveTextContent(
      "Terminal"
    );
    expect(window.localStorage.getItem("ralphx:agents:preferred-workspace-open-target")).toBe(
      "terminal"
    );
  });

  it("toggles Built-in Terminal without changing the external preference", () => {
    const toggleTerminal = vi.fn();
    const preloadTerminal = vi.fn();
    window.localStorage.setItem(
      "ralphx:agents:preferred-workspace-open-target",
      "cursor",
    );
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        terminalOpen
        workspaceOpenTargets={[
          { id: "cursor", label: "Cursor", kind: "editor" },
          { id: "terminal", label: "Terminal", kind: "terminal" },
        ]}
        onOpenWorkspaceTarget={vi.fn()}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={toggleTerminal}
        onPreloadTerminal={preloadTerminal}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-terminal-toggle")).toBeInTheDocument();
    fireEvent.pointerDown(screen.getByTestId("agents-open-workspace-options"), {
      button: 0,
      ctrlKey: false,
    });
    const builtInTerminal = screen.getByRole("menuitem", {
      name: "Built-in Terminal",
    });
    expect(screen.getByTestId("agents-built-in-terminal-open-indicator")).toBeInTheDocument();

    fireEvent.pointerEnter(builtInTerminal);
    fireEvent.focus(builtInTerminal);
    fireEvent.click(builtInTerminal);

    expect(preloadTerminal).toHaveBeenCalledTimes(2);
    expect(toggleTerminal).toHaveBeenCalledTimes(1);
    expect(window.localStorage.getItem("ralphx:agents:preferred-workspace-open-target")).toBe(
      "cursor"
    );
    expect(screen.getByTestId("agents-open-workspace-current-target")).toHaveTextContent(
      "Cursor"
    );
  });

  it("keeps archived Built-in Terminal enabled without preloading from the menu", () => {
    const toggleTerminal = vi.fn();
    const preloadTerminal = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        terminalArchivedReason="Workspace archived after PR merge. Send a follow-up to continue in a fresh workspace."
        workspaceOpenTargets={[{ id: "cursor", label: "Cursor", kind: "editor" }]}
        onOpenWorkspaceTarget={vi.fn()}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={toggleTerminal}
        onPreloadTerminal={preloadTerminal}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.pointerDown(screen.getByTestId("agents-open-workspace-options"), {
      button: 0,
      ctrlKey: false,
    });
    const builtInTerminal = screen.getByRole("menuitem", {
      name: "Built-in Terminal",
    });
    expect(builtInTerminal).not.toHaveAttribute("data-disabled");

    fireEvent.pointerEnter(builtInTerminal);
    fireEvent.focus(builtInTerminal);
    fireEvent.click(builtInTerminal);

    expect(preloadTerminal).not.toHaveBeenCalled();
    expect(toggleTerminal).toHaveBeenCalledTimes(1);
  });

  it("disables Built-in Terminal in the Open menu with the unavailable reason", () => {
    const toggleTerminal = vi.fn();
    const preloadTerminal = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        terminalUnavailableReason="Terminal is unavailable for this workspace"
        workspaceOpenTargets={[{ id: "cursor", label: "Cursor", kind: "editor" }]}
        onOpenWorkspaceTarget={vi.fn()}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={toggleTerminal}
        onPreloadTerminal={preloadTerminal}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.pointerDown(screen.getByTestId("agents-open-workspace-options"), {
      button: 0,
      ctrlKey: false,
    });
    const builtInTerminal = screen.getByRole("menuitem", {
      name: "Built-in Terminal unavailable: Terminal is unavailable for this workspace",
    });
    expect(builtInTerminal).toHaveAttribute("data-disabled");
    expect(builtInTerminal).toHaveAttribute(
      "title",
      "Terminal is unavailable for this workspace",
    );

    fireEvent.pointerEnter(builtInTerminal);
    fireEvent.focus(builtInTerminal);
    fireEvent.click(builtInTerminal);

    expect(preloadTerminal).not.toHaveBeenCalled();
    expect(toggleTerminal).not.toHaveBeenCalled();
  });

  it("shows an opening state while launching a workspace target", () => {
    const openWorkspaceTarget = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        workspaceOpenTargets={[
          { id: "cursor", label: "Cursor", kind: "editor" },
          { id: "file-manager", label: "Finder", kind: "fileManager" },
        ]}
        openingWorkspaceTargetId="file-manager"
        onOpenWorkspaceTarget={openWorkspaceTarget}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={vi.fn().mockResolvedValue(undefined)}
        onOpenPublishPane={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    const openWorkspace = screen.getByTestId("agents-open-workspace");
    expect(openWorkspace).toBeDisabled();
    expect(openWorkspace).toHaveAttribute("aria-busy", "true");
    expect(openWorkspace).toHaveTextContent("Opening");
    expect(screen.getByTestId("agents-open-workspace-current-target")).toHaveTextContent(
      "Finder"
    );
    expect(screen.getByTestId("agents-open-workspace-options")).toBeDisabled();
  });

  it("keeps the workspace opening state visible briefly after the launcher returns", async () => {
    let resolveOpenWorkspace: (() => void) | null = null;
    vi.spyOn(chatApi, "listWorkspaceOpenTargets").mockResolvedValue([
      { id: "cursor", label: "Cursor", kind: "editor" },
      { id: "file-manager", label: "Finder", kind: "fileManager" },
    ]);
    const openWorkspace = vi
      .spyOn(chatApi, "openAgentConversationWorkspace")
      .mockImplementation(
        () =>
          new Promise<void>((resolve) => {
            resolveOpenWorkspace = resolve;
          }),
      );

    renderWithProviders(
      <AgentsChatHeaderController
        conversation={conversation({ id: "conversation-1" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        hasAutoOpenArtifacts={false}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={vi.fn().mockResolvedValue(undefined)}
        onOpenPublishPane={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    const openButton = await screen.findByTestId("agents-open-workspace");

    vi.useFakeTimers();
    await act(async () => {
      fireEvent.click(openButton);
      await Promise.resolve();
    });

    expect(openWorkspace).toHaveBeenCalledWith("conversation-1", "cursor");
    expect(openButton).toBeDisabled();
    expect(openButton).toHaveTextContent("Opening");

    await act(async () => {
      resolveOpenWorkspace?.();
      await Promise.resolve();
      await Promise.resolve();
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2499);
    });
    expect(openButton).toBeDisabled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(openButton).not.toBeDisabled();
  });

  it("opens the workspace using the workspace owner conversation id", async () => {
    vi.spyOn(chatApi, "listWorkspaceOpenTargets").mockResolvedValue([
      { id: "cursor", label: "Cursor", kind: "editor" },
    ]);
    const openWorkspace = vi
      .spyOn(chatApi, "openAgentConversationWorkspace")
      .mockResolvedValue(undefined);

    renderWithProviders(
      <AgentsChatHeaderController
        conversation={conversation({ id: "selected-conversation" })}
        workspace={conversationWorkspace({
          conversationId: "workspace-conversation",
          mode: "ideation",
          linkedPlanBranchId: "plan-branch-1",
        })}
        hasAutoOpenArtifacts={false}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={vi.fn().mockResolvedValue(undefined)}
        onOpenPublishPane={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByTestId("agents-open-workspace"));

    await waitFor(() =>
      expect(openWorkspace).toHaveBeenCalledWith(
        "workspace-conversation",
        "cursor",
      ),
    );
  });

  it("clears the workspace opening state immediately when launch fails", async () => {
    vi.spyOn(chatApi, "listWorkspaceOpenTargets").mockResolvedValue([
      { id: "cursor", label: "Cursor", kind: "editor" },
    ]);
    vi.spyOn(chatApi, "openAgentConversationWorkspace").mockRejectedValue(
      new Error("Cursor failed")
    );

    renderWithProviders(
      <AgentsChatHeaderController
        conversation={conversation({ id: "conversation-1" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        hasAutoOpenArtifacts={false}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={vi.fn().mockResolvedValue(undefined)}
        onOpenPublishPane={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    const openButton = await screen.findByTestId("agents-open-workspace");
    fireEvent.click(openButton);

    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Cursor failed"));
    expect(openButton).not.toBeDisabled();
    expect(openButton).toHaveTextContent("Open");
  });

  it("shows the commit and publish shortcut for ideation workspaces linked to execution branches", () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    const openPublishPane = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({
          id: "conversation-1",
          agentMode: "ideation",
        })}
        workspace={conversationWorkspace({
          conversationId: "conversation-1",
          mode: "ideation",
          linkedIdeationSessionId: "session-1",
          linkedPlanBranchId: "plan-branch-1",
        })}
        artifactOpen={false}
        activeArtifactTab="tasks"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={publish}
        onOpenPublishPane={openPublishPane}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    expect(openPublishPane).toHaveBeenCalledTimes(1);
    expect(publish).not.toHaveBeenCalled();
  });

  it("uses the publish action as a pane shortcut instead of immediately publishing", () => {
    const openPublishPane = vi.fn();
    const publish = vi.fn().mockResolvedValue(undefined);
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1" })}
        workspace={conversationWorkspace()}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={publish}
        onOpenPublishPane={openPublishPane}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    expect(openPublishPane).toHaveBeenCalledTimes(1);
    expect(publish).not.toHaveBeenCalled();
  });

  it("labels the publish shortcut as a base update when the branch is stale", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1", agentMode: "edit" })}
        workspace={conversationWorkspace({
          mode: "edit",
          baseRef: "feature/agent-screen",
        })}
        artifactOpen={false}
        activeArtifactTab="plan"
        publishShortcutLabel="Update from feature/agent-screen"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={vi.fn().mockResolvedValue(undefined)}
        onOpenPublishPane={vi.fn()}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
      "Update from feature/agent-screen"
    );
  });

  it("locks the publish shortcut when the effective workspace is publishing in the background", () => {
    const openPublishPane = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1", agentMode: "edit" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        publishShortcutWorkspace={conversationWorkspace({
          conversationId: "conversation-2",
          mode: "edit",
          publicationPushStatus: "  PuShInG  ",
        })}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={vi.fn().mockResolvedValue(undefined)}
        onOpenPublishPane={openPublishPane}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    const publishShortcut = screen.getByRole("button", { name: "Publishing" });
    expect(publishShortcut).toBeDisabled();
    expect(publishShortcut).toHaveTextContent("Publishing");

    fireEvent.click(publishShortcut);

    expect(openPublishPane).not.toHaveBeenCalled();
  });

  it("hides the publish header shortcut while the publish pane is open", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ id: "conversation-1", agentMode: "edit" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        artifactOpen
        activeArtifactTab="publish"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onPublishWorkspace={vi.fn().mockResolvedValue(undefined)}
        onOpenPublishPane={vi.fn()}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.queryByTestId("agents-publish-workspace")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-workspace-status")).toBeInTheDocument();
  });

  it("shows the Plan shortcut for edit-mode project conversations", () => {
    const onSelectArtifact = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "edit" })}
        workspace={conversationWorkspace({ mode: "edit" })}
        availableArtifactTabs={["plan"]}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={onSelectArtifact}
      />
    );

    fireEvent.click(screen.getByLabelText("Plan"));

    expect(onSelectArtifact).toHaveBeenCalledWith("plan");
    expect(screen.queryByLabelText("Verification")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Proposals")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Tasks")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Open artifacts")).toBeInTheDocument();
  });

  it("shows the attached Plan shortcut for linked edit workspaces", () => {
    const onSelectArtifact = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "edit" })}
        workspace={conversationWorkspace({
          mode: "edit",
          linkedIdeationSessionId: "planning-session-1",
        })}
        availableArtifactTabs={["plan", "verification"]}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={onSelectArtifact}
      />
    );

    fireEvent.click(screen.getByLabelText("Plan"));

    expect(onSelectArtifact).toHaveBeenCalledWith("plan");
    expect(screen.queryByLabelText("Verification")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Proposals")).not.toBeInTheDocument();
  });

  it("shows ideation artifact shortcuts without a standalone Proposals shortcut", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "ideation" })}
        workspace={conversationWorkspace({ mode: "ideation" })}
        availableArtifactTabs={["plan", "verification", "tasks"]}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByLabelText("Plan")).toBeInTheDocument();
    expect(screen.getByLabelText("Verification")).toBeInTheDocument();
    expect(screen.queryByLabelText("Proposals")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Tasks")).toBeInTheDocument();
  });

  it("shows plan artifact shortcuts and the artifact toggle for plan-mode conversations with a plan", () => {
    const onSelectArtifact = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "plan" })}
        workspace={conversationWorkspace({ mode: "plan" })}
        availableArtifactTabs={["plan", "verification"]}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={onSelectArtifact}
      />
    );

    fireEvent.click(screen.getByLabelText("Plan"));

    expect(onSelectArtifact).toHaveBeenCalledWith("plan");
    expect(screen.getByLabelText("Verification")).toBeInTheDocument();
    expect(screen.getByLabelText("Open artifacts")).toBeInTheDocument();
  });

  it("shows plan-mode artifact controls before a plan exists", () => {
    const onSelectArtifact = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "plan" })}
        workspace={conversationWorkspace({ mode: "plan" })}
        availableArtifactTabs={["plan"]}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={onSelectArtifact}
      />
    );

    fireEvent.click(screen.getByLabelText("Plan"));

    expect(onSelectArtifact).toHaveBeenCalledWith("plan");
    expect(screen.queryByLabelText("Verification")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Open artifacts")).toBeInTheDocument();
  });

  it("hides ideation artifact shortcuts when no artifact tabs are available yet", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "ideation" })}
        workspace={conversationWorkspace({ mode: "ideation" })}
        availableArtifactTabs={[]}
        artifactOpen={false}
        activeArtifactTab="plan"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.queryByLabelText("Plan")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Verification")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Proposals")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Tasks")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Open artifacts")).toBeInTheDocument();
  });

  it("toggles the terminal from the header when a workspace is available", () => {
    const toggleTerminal = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={conversationWorkspace()}
        artifactOpen={false}
        activeArtifactTab="plan"
        terminalOpen={false}
        terminalUnavailableReason={null}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={toggleTerminal}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId("agents-terminal-toggle"));

    expect(toggleTerminal).toHaveBeenCalledTimes(1);
  });

  it("keeps the terminal toolbar visible in compact layouts", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={conversationWorkspace()}
        artifactOpen={false}
        activeArtifactTab="plan"
        terminalOpen={false}
        terminalUnavailableReason={null}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-chat-header-toolbar")).not.toHaveClass("hidden");
    expect(screen.getByTestId("agents-terminal-toggle")).toBeInTheDocument();
  });

  it("preloads terminal code when the terminal header action receives intent", () => {
    const preloadTerminal = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={conversationWorkspace()}
        artifactOpen={false}
        activeArtifactTab="plan"
        terminalOpen={false}
        terminalUnavailableReason={null}
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onPreloadTerminal={preloadTerminal}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    const toggle = screen.getByTestId("agents-terminal-toggle");
    fireEvent.pointerEnter(toggle);
    fireEvent.focus(toggle);

    expect(preloadTerminal).toHaveBeenCalledTimes(2);
  });

  it("keeps archived terminal action enabled without preloading terminal code", () => {
    const toggleTerminal = vi.fn();
    const preloadTerminal = vi.fn();
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={conversationWorkspace({ publicationPrStatus: "merged" })}
        artifactOpen={false}
        activeArtifactTab="plan"
        terminalOpen={false}
        terminalUnavailableReason={null}
        terminalArchivedReason="Workspace archived after PR merge. Send a follow-up to continue in a fresh workspace."
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={toggleTerminal}
        onPreloadTerminal={preloadTerminal}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    const toggle = screen.getByTestId("agents-terminal-toggle");
    expect(toggle).not.toBeDisabled();
    expect(toggle).toHaveAccessibleName("Show archived terminal");

    fireEvent.pointerEnter(toggle);
    fireEvent.focus(toggle);
    fireEvent.click(toggle);

    expect(preloadTerminal).not.toHaveBeenCalled();
    expect(toggleTerminal).toHaveBeenCalledTimes(1);
  });

  it("disables the terminal header action for branchless conversations", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "chat" })}
        workspace={null}
        artifactOpen={false}
        activeArtifactTab="plan"
        terminalOpen={false}
        terminalUnavailableReason="Terminal requires a workspace-backed conversation"
        onRenameConversation={vi.fn().mockResolvedValue(undefined)}
        onToggleTerminal={vi.fn()}
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    expect(screen.getByTestId("agents-terminal-toggle")).toBeDisabled();
  });
});
