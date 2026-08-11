import {
  fireAgentViewEvent,
  getAgentsViewTestMocks,
  mockAgentViewData,
  mockAgentSidebarData,
  mockSessionWithData,
  mockSidebarBreakpoint,
  renderAgentsView,
  resetAgentSessionState,
  selectSidebarConversationRow,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ideationApi } from "@/api/ideation";
import { useIdeationEvents } from "@/hooks/useIdeationEvents";
import { defaultIdeationSettings } from "@/types/ideation-config";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { AgentsView } from "./AgentsView";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
  renderWithAgentProviders as renderWithProviders,
} from "./agentsTestFixtures";
const {
  getAgentConversationRuntimeStatusesMock,
  getAgentConversationWorkspaceFreshnessMock,
  getAgentConversationWorkspaceMock,
  getLatestChildSessionIdMock,
  getWorkspaceReviewContextMock,
  loadBranchBaseOptionsMock,
  loadPullRequestBaseOptionsMock,
  updateWorkspaceFromBaseMock,
  useProjectAgentConversationsMock,
  useConversationMock,
  useConversationSummaryMock,
} = getAgentsViewTestMocks();

function AgentsViewWithIdeationEvents() {
  useIdeationEvents();
  return <AgentsView projectId="project-1" onCreateProject={vi.fn()} />;
}

describe("AgentsView", () => {
  beforeEach(setupAgentsViewTest);

  it("deselects the active agent and shows the starter when its row is clicked again", async () => {
    mockAgentViewData();

    renderAgentsView();
    const row = selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(screen.getByTestId("agents-active-conversation-panel")).toHaveStyle({
      minWidth: "600px",
    });

    fireEvent.click(within(row).getAllByRole("button")[0] ?? row);

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
  });

  it("places a supplied footer under the chat and artifact split, not under the sidebar", () => {
    mockAgentViewData();

    renderAgentsView({
      footer: <div data-testid="agents-view-footer-content">Execution footer</div>,
    });

    const contentColumn = screen.getByTestId("agents-content-column");
    const splitContainer = screen.getByTestId("agents-split-container");
    const footerShell = screen.getByTestId("agents-footer-shell");
    const sidebarContainer = screen.getByTestId("agents-sidebar-container");

    expect(contentColumn).toContainElement(splitContainer);
    expect(contentColumn).toContainElement(footerShell);
    expect(footerShell).toContainElement(screen.getByTestId("agents-view-footer-content"));
    expect(sidebarContainer).not.toContainElement(footerShell);
  });

  it("enables the conversation base picker while the workspace agent is idle", async () => {
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue({
      conversationId: "conversation-1",
      projectId: "project-1",
      mode: "edit",
      baseRefKind: "project_default",
      baseRef: "main",
      baseDisplayName: "Project default (main)",
      baseCommit: null,
      branchName: "ralphx/demo/agent-conversation-1",
      worktreePath: "/tmp/ralphx/conversation-1",
      linkedIdeationSessionId: null,
      linkedPlanBranchId: null,
      publicationPrNumber: null,
      publicationPrUrl: null,
      publicationPrStatus: null,
      publicationPushStatus: null,
      status: "active",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    });

    renderAgentsView();
    selectSidebarConversationRow();

    const baseLine = await screen.findByTestId("agents-conversation-base");
    expect(baseLine).toHaveTextContent("Project default (main)");
    expect(
      within(baseLine).getByRole("button", {
        name: "Change workspace base branch",
      }),
    ).toBeEnabled();
  });

  it("rebases an idle workspace from the conversation base picker", async () => {
    const workspace = conversationWorkspace({
      branchName: "ralphx/demo/agent-conversation-1",
      worktreePath: "/tmp/ralphx/conversation-1",
    });
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue(workspace);

    renderAgentsView();
    selectSidebarConversationRow();

    const baseLine = await screen.findByTestId("agents-conversation-base");
    fireEvent.click(
      within(baseLine).getByRole("button", {
        name: "Change workspace base branch",
      }),
    );

    await waitFor(() =>
      expect(loadBranchBaseOptionsMock).toHaveBeenCalledWith({
        projectId: "project-1",
        workingDirectory: "/tmp/ralphx/conversation-1",
        includeAgentBranches: false,
      }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: /feature\/new-base/i }),
    );
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();

    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Rebase workspace",
      }),
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1", {
        kind: "local_branch",
        ref: "feature/new-base",
        displayName: "feature/new-base",
      }),
    );
  });

  it("rebases an idle workspace from a PR selected in the conversation base picker", async () => {
    const workspace = conversationWorkspace({
      branchName: "ralphx/demo/agent-conversation-1",
      worktreePath: "/tmp/ralphx/conversation-1",
    });
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue(workspace);

    renderAgentsView();
    selectSidebarConversationRow();

    const baseLine = await screen.findByTestId("agents-conversation-base");
    fireEvent.click(
      within(baseLine).getByRole("button", {
        name: "Change workspace base branch",
      }),
    );
    fireEvent.click(await screen.findByRole("tab", { name: "PRs" }));

    await waitFor(() =>
      expect(loadPullRequestBaseOptionsMock).toHaveBeenCalledWith({
        projectId: "project-1",
        query: "",
      }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: /#42 Add PR base/i }),
    );
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();

    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Rebase workspace",
      }),
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1", {
        kind: "local_branch",
        ref: "feature/pr-base",
        displayName: "PR #42: Add PR base",
        sourcePullRequest: {
          number: 42,
          url: "https://github.com/mock/project/pull/42",
          title: "Add PR base",
          headRefName: "feature/pr-base",
          baseRefName: "main",
          headRefOid: "pr-head-sha",
        },
      }),
    );
  });

  it("keeps the conversation base picker disabled while the agent is generating", async () => {
    const activeConversation = conversation({ agentMode: "edit" });
    mockAgentViewData(activeConversation);
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" }),
    );
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      [activeConversation.id]: {
        conversationId: activeConversation.id,
        isRunning: true,
        agentStatus: "generating",
        primarySource: "workspace",
        summaryLabel: "Agent running",
        items: [],
      },
    });

    renderAgentsView();
    selectSidebarConversationRow();

    const baseLine = await screen.findByTestId("agents-conversation-base");
    await waitFor(() =>
      expect(
        within(baseLine).getByRole("button", {
          name: "Change workspace base branch",
        }),
      ).toBeDisabled(),
    );
    expect(loadBranchBaseOptionsMock).not.toHaveBeenCalled();
  });

  it("keeps the conversation base picker clickable while the live agent is waiting for input", async () => {
    const activeConversation = conversation({ agentMode: "edit" });
    mockAgentViewData(activeConversation);
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" }),
    );
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      [activeConversation.id]: {
        conversationId: activeConversation.id,
        isRunning: true,
        agentStatus: "waiting_for_input",
        primarySource: "workspace",
        summaryLabel: "Agent running",
        items: [],
      },
    });

    renderAgentsView();
    selectSidebarConversationRow();

    const baseLine = await screen.findByTestId("agents-conversation-base");
    const picker = within(baseLine).getByRole("button", {
      name: "Change workspace base branch",
    });
    await waitFor(() => expect(picker).toBeEnabled());

    fireEvent.click(picker);
    await waitFor(() =>
      expect(loadBranchBaseOptionsMock).toHaveBeenCalledWith({
        projectId: "project-1",
        workingDirectory: "/tmp/ralphx/conversation-1",
        includeAgentBranches: false,
      }),
    );
  });

  it("shows source PR metadata in the conversation base line", async () => {
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue({
      conversationId: "conversation-1",
      projectId: "project-1",
      mode: "chat",
      baseRefKind: "local_branch",
      baseRef: "feature/source-pr",
      baseDisplayName: "feature/source-pr",
      baseCommit: null,
      branchName: "ralphx/demo/agent-conversation-1",
      worktreePath: "/tmp/ralphx/conversation-1",
      linkedIdeationSessionId: null,
      linkedPlanBranchId: null,
      sourcePullRequest: {
        number: 42,
        url: "https://github.com/owner/repo/pull/42",
        title: "Source PR",
        headRefName: "feature/source-pr",
        baseRefName: "main",
        headRefOid: "abc123",
      },
      publicationPrNumber: null,
      publicationPrUrl: null,
      publicationPrStatus: null,
      publicationPushStatus: null,
      status: "active",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    });

    renderAgentsView();
    selectSidebarConversationRow();

    const baseLine = await screen.findByTestId("agents-conversation-base");
    expect(baseLine).toHaveTextContent("PR #42: Source PR");
    expect(baseLine).toHaveTextContent("feature/source-pr");
  });

  it("preflights workspace freshness on selection without duplicate rapid reselect checks", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    let resolveWorkspace: (
      workspace: ReturnType<typeof conversationWorkspace>
    ) => void = () => undefined;
    const workspacePromise = new Promise<ReturnType<typeof conversationWorkspace>>(
      (resolve) => {
        resolveWorkspace = resolve;
      }
    );
    getAgentConversationWorkspaceMock.mockReturnValue(workspacePromise);

    renderAgentsView();
    const row = selectSidebarConversationRow();

    await waitFor(() =>
      expect(getAgentConversationWorkspaceMock).toHaveBeenCalled()
    );
    fireEvent.click(within(row).getAllByRole("button")[0] ?? row);
    fireEvent.click(within(row).getAllByRole("button")[0] ?? row);
    expect(getAgentConversationWorkspaceFreshnessMock).not.toHaveBeenCalled();

    resolveWorkspace(conversationWorkspace({ mode: "edit" }));

    await waitFor(() =>
      expect(getAgentConversationWorkspaceFreshnessMock).toHaveBeenCalledTimes(1)
    );
    expect(getAgentConversationWorkspaceFreshnessMock).toHaveBeenCalledWith(
      "conversation-1",
      { scope: "local" }
    );
  });

  it("requests freshness for a published Plan-mode workspace", async () => {
    mockAgentViewData(conversation({ agentMode: "plan" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "plan",
        publicationPrNumber: 42,
        publicationPrStatus: "open",
      })
    );

    renderAgentsView();
    const row = selectSidebarConversationRow();
    fireEvent.click(within(row).getAllByRole("button")[0] ?? row);

    await waitFor(() =>
      expect(getAgentConversationWorkspaceFreshnessMock).toHaveBeenCalledWith(
        "conversation-1",
        { scope: "local" }
      )
    );
  });

  it("does not request freshness for a missing Plan-mode workspace", async () => {
    mockAgentViewData(conversation({ agentMode: "plan" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "plan", status: "missing" })
    );

    renderAgentsView();
    const row = selectSidebarConversationRow();
    fireEvent.click(within(row).getAllByRole("button")[0] ?? row);

    await waitFor(() =>
      expect(getAgentConversationWorkspaceMock).toHaveBeenCalled()
    );
    expect(getAgentConversationWorkspaceFreshnessMock).not.toHaveBeenCalled();
  });

  it("does not hydrate attached ideation session data for edit conversations", async () => {
    const agentConversation = conversation({ agentMode: "edit" });
    mockAgentViewData(agentConversation);
    useConversationMock.mockImplementation((conversationId: string | null) => ({
      data:
        conversationId === agentConversation.id
          ? {
              conversation: agentConversation,
              messages: [
                {
                  id: "message-1",
                  conversationId: agentConversation.id,
                  role: "assistant",
                  content: "",
                  toolCalls: [
                    {
                      id: "tool-1",
                      name: "v1_start_ideation",
                      arguments: {},
                      result: { session_id: "session-1" },
                    },
                  ],
                  contentBlocks: [],
                  createdAt: "2026-04-23T09:00:00Z",
                },
              ],
            }
          : null,
      isLoading: false,
    }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" })
    );

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(getAgentConversationWorkspaceMock).toHaveBeenCalledWith("conversation-1")
    );
    expect(vi.mocked(ideationApi.sessions.getWithData)).not.toHaveBeenCalled();
  });

  it("focuses the main chat on an attached ideation run when Open Run is used", async () => {
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "ideation", linkedIdeationSessionId: "session-1" })
    );
    mockSessionWithData({ id: "session-1", planArtifactId: "plan-1" });

    renderAgentsView();
    selectSidebarConversationRow();

    const panel = await screen.findByTestId("integrated-chat-panel");
    expect(panel).toHaveAttribute("data-conversation-id-override", "conversation-1");
    expect(panel).toHaveAttribute("data-ideation-session-id", "");
    expect(
      await screen.findByTestId("agents-conversation-workspace-line"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("mock-open-child-session"));

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-ideation-session-id",
        "session-child",
      );
    });
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-conversation-id-override",
      "",
    );
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-send-conversation-id",
      "",
    );
    expect(screen.getByTestId("agents-composer-chat-focus-pill")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-workspace-status")).not.toBeInTheDocument();

    // Open dropdown and select Workspace
    fireEvent.click(screen.getByTestId("agents-composer-chat-focus-pill"));
    fireEvent.click(
      screen.getByTestId("agents-composer-chat-focus-option-workspace"),
    );

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "conversation-1",
      );
    });
    expect(
      await screen.findByTestId("agents-conversation-workspace-line"),
    ).toBeInTheDocument();
  });

  it("uses the direct reviewer summary instead of stale paged metadata and focus hints", async () => {
    const workspaceConversation = conversation({
      id: "conversation-1",
      agentMode: "ideation",
      logicalEffort: "low",
    });
    const stalePagedReviewer = conversation({
      id: "review-conversation-1",
      parentConversationId: workspaceConversation.id,
      providerHarness: "claude",
      logicalModel: "sonnet",
      logicalEffort: "low",
    });
    const durableReviewerSummary = conversation({
      id: stalePagedReviewer.id,
      parentConversationId: workspaceConversation.id,
      providerHarness: "codex",
      logicalModel: "gpt-5.5",
      logicalEffort: "xhigh",
    });
    mockAgentViewData(workspaceConversation);
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
      }),
    );
    getWorkspaceReviewContextMock.mockResolvedValue({
      success: true,
      workspace: conversationWorkspace({
        conversationId: workspaceConversation.id,
        mode: "ideation",
      }),
      events: [],
      target: null,
      monitor: {
        conversationId: workspaceConversation.id,
        status: "reviewing",
        reviewOutcome: "none",
        reviewGateStatus: "reviewing",
        reviewConversationId: stalePagedReviewer.id,
        reviewArtifactId: null,
        reviewArtifactVersion: null,
        lastError: null,
      },
      isCurrent: false,
      isOutdated: false,
      shouldShowTab: true,
    });
    vi.mocked(ideationApi.settings.get).mockResolvedValue(defaultIdeationSettings);
    mockSessionWithData({ id: "session-1", planArtifactId: "plan-1" });
    useProjectAgentConversationsMock.mockReturnValue({
      data: [workspaceConversation, stalePagedReviewer],
      conversations: [workspaceConversation, stalePagedReviewer],
      isLoading: false,
      isSuccess: true,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useConversationSummaryMock.mockImplementation((conversationId: string | null) => ({
      data:
        conversationId === durableReviewerSummary.id
          ? durableReviewerSummary
          : conversationId === workspaceConversation.id
            ? workspaceConversation
            : null,
      isLoading: false,
    }));
    useAgentSessionStore.getState().setRuntimeForConversation(
      workspaceConversation.id,
      "project-1",
      { provider: "codex", modelId: "gpt-5.5", effort: "xhigh" },
    );

    renderAgentsView();
    selectSidebarConversationRow();
    await screen.findByTestId("integrated-chat-panel");
    fireEvent.click(await screen.findByRole("button", { name: "Open artifacts" }));
    fireEvent.click(
      await screen.findByTestId(
        "mock-focus-workspace-review-with-stale-runtime",
        {},
        { timeout: 5_000 },
      ),
    );

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        durableReviewerSummary.id,
      );
      expect(
        getAgentsViewTestMocks().integratedChatPanelRenderMock,
      ).toHaveBeenCalledWith(
        expect.objectContaining({
          conversationIdOverride: durableReviewerSummary.id,
          sendOptions: expect.objectContaining({
            providerHarness: "codex",
            modelId: "gpt-5.5",
            logicalEffort: "xhigh",
          }),
        }),
      );
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.pointerMove(screen.getByRole("button", { name: /^Provider,/ }));
    fireEvent.click(screen.getByTestId("agent-composer-runtime-provider-claude"));

    await waitFor(() => {
      expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveTextContent(
        "sonnet",
      );
    });
    fireEvent.pointerMove(screen.getByRole("button", { name: /^Model,/ }));
    expect(
      screen.getByTestId("agent-composer-runtime-model-sonnet"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agent-composer-runtime-model-gpt-5.5"),
    ).not.toBeInTheDocument();
    expect(
      useAgentSessionStore.getState().roleRuntimeOverridesByConversationId[
        workspaceConversation.id
      ]?.workspace_reviewer,
    ).toMatchObject({ provider: "claude", model: "sonnet" });
    expect(
      useAgentSessionStore.getState().runtimeByConversationId[
        workspaceConversation.id
      ],
    ).toEqual({ provider: "codex", modelId: "gpt-5.5", effort: "xhigh" });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.click(screen.getByTestId("agents-composer-chat-focus-pill"));
    fireEvent.click(
      screen.getByTestId("agents-composer-chat-focus-option-workspace"),
    );
    fireEvent.click(screen.getByTestId("agents-composer-chat-focus-pill"));
    fireEvent.click(
      screen.getByTestId("agents-composer-chat-focus-option-workspace_review"),
    );

    await waitFor(() => {
      expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveTextContent(
        "sonnet",
      );
      expect(
        getAgentsViewTestMocks().integratedChatPanelRenderMock,
      ).toHaveBeenCalledWith(
        expect.objectContaining({
          sendOptions: expect.objectContaining({
            providerHarness: "claude",
            modelId: "sonnet",
          }),
        }),
      );
    });
  });

  it("shows the chat focus switcher on workspace chat when the latest archived/completed verification child is hydrated", async () => {
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "ideation", linkedIdeationSessionId: "session-1" })
    );
    mockSessionWithData({
      id: "session-1",
      planArtifactId: "plan-1",
      verificationStatus: "verified",
      verificationInProgress: false,
    });
    getLatestChildSessionIdMock.mockResolvedValue({
      sessionId: "session-1",
      purpose: "verification",
      latestChildSessionId: "verification-child",
    });

    renderAgentsView();
    selectSidebarConversationRow();

    // Workspace chat hosts the focus switcher inline in the composer.
    expect(
      await screen.findByTestId("agents-composer-chat-focus-pill"),
    ).toHaveTextContent("Workspace");
    await waitFor(() => {
      expect(getLatestChildSessionIdMock).toHaveBeenCalledWith(
        "session-1",
        "verification",
        { includeArchived: true },
      );
    });

    // Open dropdown and select Verification
    fireEvent.click(screen.getByTestId("agents-composer-chat-focus-pill"));
    fireEvent.click(
      screen.getByTestId("agents-composer-chat-focus-option-verification"),
    );

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-ideation-session-id",
        "verification-child",
      );
    });
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-conversation-id-override",
      "",
    );
  });

  it("adds Verification to the live composer selector when a verification child is created", async () => {
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "ideation", linkedIdeationSessionId: "session-1" })
    );
    mockSessionWithData({ id: "session-1", planArtifactId: "plan-1" });
    getLatestChildSessionIdMock.mockResolvedValue({
      sessionId: "session-1",
      purpose: "verification",
      latestChildSessionId: null,
    });

    renderWithProviders(<AgentsViewWithIdeationEvents />);
    selectSidebarConversationRow();

    const focusPill = await screen.findByTestId("agents-composer-chat-focus-pill");
    expect(focusPill).toHaveTextContent("Workspace");
    await waitFor(() => {
      expect(getLatestChildSessionIdMock).toHaveBeenCalledWith(
        "session-1",
        "verification",
        { includeArchived: true },
      );
    });

    fireEvent.click(focusPill);
    expect(
      screen.queryByTestId("agents-composer-chat-focus-option-verification"),
    ).not.toBeInTheDocument();

    await act(async () => {
      fireAgentViewEvent("ideation:child_session_created", {
        sessionId: "verification-child-live",
        parentSessionId: "session-1",
        title: "Verification Session",
        purpose: "verification",
      });
    });

    await waitFor(() => {
      expect(
        screen.getByTestId("agents-composer-chat-focus-option-verification"),
      ).toBeInTheDocument();
    });

    fireEvent.click(
      screen.getByTestId("agents-composer-chat-focus-option-verification"),
    );

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-ideation-session-id",
        "verification-child-live",
      );
    });
  });

  it("does NOT auto-switch the chat focus when the Plan artifact tab is selected", async () => {
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "ideation", linkedIdeationSessionId: "session-1" })
    );
    mockSessionWithData({ id: "session-1", planArtifactId: "plan-1" });
    resetAgentSessionState({
      artifactByConversationId: {
        "conversation-1": {
          isOpen: false,
          activeTab: "plan",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() => {
      expect(screen.getByLabelText("Plan")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByLabelText("Plan"));

    // Workspace chat stays selected — clicking artifact tabs no longer
    // auto-focuses the attached ideation chat. The user opts in via the
    // composer chat-focus pill explicitly.
    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "conversation-1",
      );
    });
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-ideation-session-id",
      "",
    );
  });

  it("ignores a stale proposal focus request after another conversation is selected", async () => {
    const firstConversation = conversation({
      id: "conversation-1",
      agentMode: "plan",
      title: "Plan conversation",
    });
    const secondConversation = conversation({
      id: "conversation-2",
      agentMode: "edit",
      title: "Current conversation",
    });
    const conversations = [firstConversation, secondConversation];
    mockAgentViewData(firstConversation);
    mockAgentSidebarData(conversations);
    useProjectAgentConversationsMock.mockReturnValue({
      data: conversations,
      conversations,
      isLoading: false,
      isSuccess: true,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useConversationMock.mockImplementation((conversationId: string | null) => {
      const current = conversations.find((item) => item.id === conversationId);
      return {
        data: current ? { conversation: current, messages: [] } : null,
        isLoading: false,
      };
    });
    getAgentConversationWorkspaceMock.mockImplementation(
      async (conversationId: string) =>
        conversationWorkspace({
          conversationId,
          mode: conversationId === "conversation-1" ? "plan" : "edit",
          linkedIdeationSessionId:
            conversationId === "conversation-1" ? "session-1" : "session-2",
        }),
    );
    useAgentArtifactUiStore.setState({
      artifactByConversationId: {
        "conversation-2": {
          isOpen: true,
          activeTab: "plan",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();
    const secondConversationRow = await screen.findByTestId(
      "agents-session-conversation-2",
    );
    fireEvent.click(within(secondConversationRow).getAllByRole("button")[0]!);

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "conversation-2",
      );
    });

    fireEvent.click(await screen.findByTestId("mock-focus-stale-proposals-session"));

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "conversation-2",
      );
    });
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-ideation-session-id",
      "",
    );
  });

  it("returns Review child focus to the workspace chat when another artifact tab is selected", async () => {
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "ideation", linkedIdeationSessionId: "session-1" })
    );
    getWorkspaceReviewContextMock.mockResolvedValue({
      success: true,
      workspace: conversationWorkspace({ mode: "ideation" }),
      events: [],
      target: {
        scope: "workspace_delta",
        baseRef: "main",
        baseSha: "base-sha",
        headRef: "HEAD",
        headSha: "head-sha",
        diffFingerprint: "fingerprint-1",
        sourcePullRequestNumber: null,
      },
      monitor: {
        conversationId: "conversation-1",
        status: "ready",
        reviewOutcome: "passed",
        reviewGateStatus: "passed",
        reviewConversationId: "review-conversation-1",
        reviewArtifactId: "review-artifact-1",
        reviewArtifactVersion: 1,
        lastError: null,
      },
      isCurrent: true,
      isOutdated: false,
      shouldShowTab: true,
    });
    mockSessionWithData({ id: "session-1", planArtifactId: "plan-1" });
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: false,
          activeTab: "review",
          taskMode: "graph",
        },
      },
    });
    useAgentArtifactUiStore.setState({
      artifactByConversationId: {
        "conversation-1": {
          isOpen: false,
          activeTab: "review",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();

    const focusPill = await screen.findByTestId("agents-composer-chat-focus-pill");
    fireEvent.click(focusPill);
    fireEvent.click(
      await screen.findByTestId("agents-composer-chat-focus-option-workspace_review"),
    );

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "review-conversation-1",
      );
    });

    fireEvent.click(await screen.findByLabelText("Plan"));

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "conversation-1",
      );
    });
    expect(screen.getByTestId("agents-composer-chat-focus-pill")).toHaveTextContent(
      "Workspace",
    );
  });

  it("keeps Review child focus when another artifact tab is selected while Review is running", async () => {
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "ideation", linkedIdeationSessionId: "session-1" })
    );
    getWorkspaceReviewContextMock.mockResolvedValue({
      success: true,
      workspace: conversationWorkspace({ mode: "ideation" }),
      events: [],
      target: {
        scope: "workspace_delta",
        baseRef: "main",
        baseSha: "base-sha",
        headRef: "HEAD",
        headSha: "head-sha",
        diffFingerprint: "fingerprint-1",
        sourcePullRequestNumber: null,
      },
      monitor: {
        conversationId: "conversation-1",
        status: "reviewing",
        reviewOutcome: "none",
        reviewGateStatus: "reviewing",
        reviewConversationId: "review-conversation-1",
        reviewArtifactId: null,
        reviewArtifactVersion: null,
        lastError: null,
      },
      isCurrent: false,
      isOutdated: false,
      shouldShowTab: true,
    });
    mockSessionWithData({ id: "session-1", planArtifactId: "plan-1" });
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: false,
          activeTab: "review",
          taskMode: "graph",
        },
      },
    });
    useAgentArtifactUiStore.setState({
      artifactByConversationId: {
        "conversation-1": {
          isOpen: false,
          activeTab: "review",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "review-conversation-1",
      );
    });

    fireEvent.click(await screen.findByLabelText("Plan"));

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "review-conversation-1",
      );
    });
    expect(screen.getByTestId("agents-composer-chat-focus-pill")).toHaveTextContent(
      "Review",
    );
  });

  it("focuses the main chat on a verification child selected from artifacts", async () => {
    mockAgentViewData();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "ideation", linkedIdeationSessionId: "session-parent" })
    );
    useAgentArtifactUiStore.setState({
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "verification",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();
    selectSidebarConversationRow();

    const focusButton = await screen.findByTestId("mock-focus-verification-session");
    fireEvent.click(focusButton);

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-ideation-session-id",
        "verification-child",
      );
    });
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-conversation-id-override",
      "",
    );
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-send-conversation-id",
      "",
    );
    expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
      "data-focused-ideation-session-id",
      "session-parent",
    );
    // Composer pill shows Verification as the active focus
    expect(screen.getByTestId("agents-composer-chat-focus-pill")).toHaveTextContent(
      "Verification",
    );
    // Open dropdown and switch to Ideation
    fireEvent.click(screen.getByTestId("agents-composer-chat-focus-pill"));
    fireEvent.click(
      screen.getByTestId("agents-composer-chat-focus-option-ideation"),
    );

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-ideation-session-id",
        "session-parent",
      );
    });
    // Composer pill now shows Ideation
    expect(screen.getByTestId("agents-composer-chat-focus-pill")).toHaveTextContent(
      "Ideation",
    );
    expect(screen.queryByTestId("agents-workspace-status")).not.toBeInTheDocument();
  });

  it("uses a collapsed sidebar strip on small screens and opens the overlay on demand", async () => {
    mockSidebarBreakpoint({ isLarge: false, isMedium: false });
    mockAgentViewData();

    renderAgentsView();

    expect(screen.getByTestId("agents-sidebar-toggle-strip")).toBeInTheDocument();
    expect(screen.getByTestId("agents-sidebar")).not.toBeVisible();

    fireEvent.click(screen.getByTestId("agents-sidebar-toggle-strip"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-sidebar")).toBeInTheDocument()
    );
    expect(screen.getByTestId("agents-sidebar-overlay-backdrop")).toBeInTheDocument();
  });
});
