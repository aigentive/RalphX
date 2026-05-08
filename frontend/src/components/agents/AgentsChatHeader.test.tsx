import { fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { chatApi, type ConversationStatsResponse } from "@/api/chat";
import { useChatStore } from "@/stores/chatStore";
import { AgentsChatFocusBar, AgentsChatHeader } from "./AgentsChatHeader";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
  renderWithAgentProviders as renderWithProviders,
} from "./agentsTestFixtures";

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
      estimatedUsd: null,
    },
    runUsageTotals: {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      estimatedUsd: null,
    },
    effectiveUsageTotals: {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      estimatedUsd: null,
    },
    usageCoverage: {
      providerMessageCount: 0,
      providerMessagesWithUsage: 0,
      runCount: 0,
      runsWithUsage: 0,
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
  afterEach(() => {
    vi.restoreAllMocks();
    useChatStore.setState({ agentStatus: {}, isSending: {} });
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
        chatFocus={{ type: "ideation", sessionId: "session-child" }}
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

  it("keeps verification focus out of the primary title row", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation()}
        workspace={null}
        chatFocus={{
          type: "verification",
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
    vi.spyOn(chatApi, "getConversationStats").mockResolvedValue(conversationStats());
    useChatStore
      .getState()
      .setAgentStatus("project:conversation-1", "generating");

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

    fireEvent.click(screen.getByTestId("chat-session-stats-button"));

    expect(
      await screen.findByText(
        "Usage totals are pending until the provider reports the current turn.",
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Pending")).toHaveLength(4);
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
        onToggleArtifacts={vi.fn()}
        onSelectArtifact={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    expect(openPublishPane).toHaveBeenCalledTimes(1);
    expect(publish).not.toHaveBeenCalled();
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

  it("hides ideation artifact shortcuts for edit-mode conversations", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "edit" })}
        workspace={conversationWorkspace({ mode: "edit" })}
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
  });

  it("shows ideation artifact shortcuts for ideation-mode conversations", () => {
    renderWithProviders(
      <AgentsChatHeader
        conversation={conversation({ agentMode: "ideation" })}
        workspace={conversationWorkspace({ mode: "ideation" })}
        availableArtifactTabs={["plan", "verification", "proposal", "tasks"]}
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
    expect(screen.getByLabelText("Proposals")).toBeInTheDocument();
    expect(screen.getByLabelText("Tasks")).toBeInTheDocument();
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
