import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { AgentConversationWorkspace } from "@/api/chat";
import type { AgentArtifactTab } from "@/stores/agentSessionStore";
import { createTestQueryClient } from "@/test/store-utils";
import { AgentsArtifactPane } from "./AgentsArtifactPane";

const deferredHydrationTimeout = { timeout: 3_000 };

const {
  getWorkspaceChangesMock,
  getWorkspaceDiffMock,
  getWorkspaceCommitsMock,
  getWorkspaceCommitChangesMock,
  getWorkspaceCommitDiffMock,
  listPublicationEventsMock,
  getWorkspaceFreshnessMock,
  updateWorkspaceFromBaseMock,
  closeWorkspacePrMock,
  getArtifactMock,
  getIdeationSessionMock,
  getIdeationChildrenMock,
  useConversationMock,
  useDependencyGraphMock,
  useVerificationStatusMock,
  useGitAuthDiagnosticsMock,
  useGhAuthStatusMock,
  switchGitOriginToSshMock,
  setupGhGitAuthMock,
  loginGhWithBrowserMock,
  resumeDeferredGitStartupMock,
  openUrlMock,
} = vi.hoisted(() => ({
  getWorkspaceChangesMock: vi.fn(),
  getWorkspaceDiffMock: vi.fn(),
  getWorkspaceCommitsMock: vi.fn(),
  getWorkspaceCommitChangesMock: vi.fn(),
  getWorkspaceCommitDiffMock: vi.fn(),
  listPublicationEventsMock: vi.fn(),
  getWorkspaceFreshnessMock: vi.fn(),
  updateWorkspaceFromBaseMock: vi.fn(),
  closeWorkspacePrMock: vi.fn(),
  getArtifactMock: vi.fn(),
  getIdeationSessionMock: vi.fn(),
  getIdeationChildrenMock: vi.fn(),
  useConversationMock: vi.fn(),
  useDependencyGraphMock: vi.fn(),
  useVerificationStatusMock: vi.fn(),
  useGitAuthDiagnosticsMock: vi.fn(),
  useGhAuthStatusMock: vi.fn(),
  switchGitOriginToSshMock: vi.fn(),
  setupGhGitAuthMock: vi.fn(),
  loginGhWithBrowserMock: vi.fn(),
  resumeDeferredGitStartupMock: vi.fn(),
  openUrlMock: vi.fn(),
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      listAgentConversationWorkspacePublicationEvents: (...args: unknown[]) =>
        listPublicationEventsMock(...args),
      getAgentConversationWorkspaceFreshness: (...args: unknown[]) =>
        getWorkspaceFreshnessMock(...args),
      updateAgentConversationWorkspaceFromBase: (...args: unknown[]) =>
        updateWorkspaceFromBaseMock(...args),
      closeAgentWorkspacePr: (...args: unknown[]) =>
        closeWorkspacePrMock(...args),
    },
  };
});

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceFileChanges: (...args: unknown[]) =>
      getWorkspaceChangesMock(...args),
    getAgentConversationWorkspaceFileDiff: (...args: unknown[]) =>
      getWorkspaceDiffMock(...args),
    getAgentConversationWorkspaceCommits: (...args: unknown[]) =>
      getWorkspaceCommitsMock(...args),
    getAgentConversationWorkspaceCommitFileChanges: (...args: unknown[]) =>
      getWorkspaceCommitChangesMock(...args),
    getAgentConversationWorkspaceCommitFileDiff: (...args: unknown[]) =>
      getWorkspaceCommitDiffMock(...args),
  },
}));

vi.mock("@/api/ideation", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/ideation")>();
  return {
    ...actual,
    ideationApi: {
      ...actual.ideationApi,
      sessions: {
        ...actual.ideationApi.sessions,
        getWithData: (...args: unknown[]) => getIdeationSessionMock(...args),
        getChildren: (...args: unknown[]) => getIdeationChildrenMock(...args),
      },
    },
  };
});

vi.mock("@/components/Ideation/VerificationPanel", () => ({
  VerificationPanel: ({ session }: { session: { id: string } }) => (
    <div data-testid="mock-verification-panel">{session.id}</div>
  ),
}));

vi.mock("@/components/tasks/TaskBoard", () => ({
  TaskBoard: ({ onTaskSelect }: { onTaskSelect?: (taskId: string) => void }) => (
    <button
      type="button"
      data-testid="mock-agent-task-card"
      onClick={() => onTaskSelect?.("task-1")}
    >
      Open task
    </button>
  ),
}));

vi.mock("@/components/agents/task-details/AgentsTaskDetailOverlay", () => ({
  AgentsTaskDetailOverlay: ({
    selectedTaskIdOverride,
    onCloseOverride,
  }: {
    selectedTaskIdOverride?: string | null;
    onCloseOverride?: () => void;
  }) =>
    selectedTaskIdOverride ? (
      <div
        data-testid="mock-agent-task-detail"
        data-task-id={selectedTaskIdOverride}
      >
        <button type="button" onClick={onCloseOverride}>
          Close task
        </button>
      </div>
    ) : null,
}));

vi.mock("@/api/artifact", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/artifact")>();
  return {
    ...actual,
    artifactApi: {
      ...actual.artifactApi,
      get: (...args: unknown[]) => getArtifactMock(...args),
    },
  };
});

vi.mock("@/hooks/useChat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/hooks/useChat")>();
  return {
    ...actual,
    useConversationHistoryWindow: (...args: unknown[]) => useConversationMock(...args),
  };
});

vi.mock("@/hooks/useDependencyGraph", () => ({
  useDependencyGraph: (...args: unknown[]) => useDependencyGraphMock(...args),
}));

vi.mock("@/hooks/useVerificationStatus", () => ({
  useVerificationStatus: (...args: unknown[]) => useVerificationStatusMock(...args),
}));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGitAuthDiagnostics: (...args: unknown[]) => useGitAuthDiagnosticsMock(...args),
  useGhAuthStatus: (...args: unknown[]) => useGhAuthStatusMock(...args),
  useSwitchGitOriginToSsh: () => ({
    mutateAsync: switchGitOriginToSshMock,
    isPending: false,
  }),
  useSetupGhGitAuth: () => ({
    mutateAsync: setupGhGitAuthMock,
    isPending: false,
  }),
  useLoginGhWithBrowser: () => ({
    mutateAsync: loginGhWithBrowserMock,
    isPending: false,
  }),
  useResumeDeferredGitStartup: () => ({
    mutateAsync: resumeDeferredGitStartupMock,
    isPending: false,
  }),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (...args: unknown[]) => openUrlMock(...args),
}));

const workspace = (
  overrides: Partial<AgentConversationWorkspace> = {}
): AgentConversationWorkspace => ({
  conversationId: "conversation-1",
  projectId: "project-1",
  mode: "ideation",
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
  createdAt: "2026-04-23T09:00:00Z",
  updatedAt: "2026-04-23T09:00:00Z",
  ...overrides,
});

const conversation = () => ({
  id: "conversation-1",
  contextType: "project" as const,
  contextId: "project-1",
  projectId: "project-1",
  ideationSessionId: null,
  claudeSessionId: null,
  providerSessionId: null,
  providerHarness: "codex",
  agentMode: "edit" as const,
  title: "Agent conversation",
  messageCount: 1,
  lastMessageAt: "2026-04-23T09:00:00Z",
  createdAt: "2026-04-23T09:00:00Z",
  updatedAt: "2026-04-23T09:00:00Z",
  archivedAt: null,
});

function renderPane(
  activeTab: AgentArtifactTab = "tasks",
  paneWorkspace = workspace(),
  onPublishWorkspace = vi.fn(),
  isPublishingWorkspace = false,
  paneConversation = null,
  paneProps: Partial<ComponentProps<typeof AgentsArtifactPane>> = {},
) {
  const queryClient = createTestQueryClient();

  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <div className="h-[480px]">
          <AgentsArtifactPane
            conversation={paneConversation}
            workspace={paneWorkspace}
            activeTab={activeTab}
            taskMode="graph"
            onTabChange={() => {}}
            onTaskModeChange={() => {}}
            onPublishWorkspace={onPublishWorkspace}
            isPublishingWorkspace={isPublishingWorkspace}
            onClose={() => {}}
            {...paneProps}
          />
        </div>
      </TooltipProvider>
    </QueryClientProvider>
  );
}

describe("AgentsArtifactPane", () => {
  beforeEach(() => {
    getWorkspaceChangesMock.mockResolvedValue([
      { path: "frontend/src/App.tsx", status: "modified", additions: 4, deletions: 1 },
    ]);
    getWorkspaceDiffMock.mockResolvedValue({
      filePath: "frontend/src/App.tsx",
      oldContent: "old",
      newContent: "new",
      language: "typescript",
    });
    getWorkspaceCommitsMock.mockResolvedValue([]);
    getWorkspaceCommitChangesMock.mockResolvedValue([
      { path: "frontend/src/App.tsx", status: "modified", additions: 4, deletions: 1 },
    ]);
    getWorkspaceCommitDiffMock.mockResolvedValue({
      filePath: "frontend/src/App.tsx",
      oldContent: "old",
      newContent: "new",
      language: "typescript",
    });
    listPublicationEventsMock.mockResolvedValue([]);
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "main",
      baseDisplayName: "Project default (main)",
      targetRef: "origin/main",
      capturedBaseCommit: "base-sha",
      targetBaseCommit: "base-sha",
      isBaseAhead: false,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
    });
    updateWorkspaceFromBaseMock.mockResolvedValue({
      workspace: workspace({ mode: "edit", baseCommit: "base-sha" }),
      updated: false,
      targetRef: "origin/main",
      baseCommit: "base-sha",
    });
    closeWorkspacePrMock.mockResolvedValue(
      workspace({
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "closed",
        publicationPushStatus: "pushed",
      }),
    );
    getArtifactMock.mockResolvedValue(null);
    getIdeationSessionMock.mockResolvedValue(null);
    getIdeationChildrenMock.mockResolvedValue([]);
    useConversationMock.mockReturnValue({
      data: null,
      isLoading: false,
    });
    useDependencyGraphMock.mockReturnValue({
      data: null,
      isLoading: false,
    });
    useVerificationStatusMock.mockReturnValue({
      data: null,
      isLoading: false,
    });
    useGitAuthDiagnosticsMock.mockReturnValue({
      data: {
        fetchUrl: "git@github.com:mock/project.git",
        pushUrl: "git@github.com:mock/project.git",
        fetchKind: "SSH",
        pushKind: "SSH",
        mixedAuthModes: false,
        canSwitchToSsh: false,
        suggestedSshUrl: null,
      },
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });
    useGhAuthStatusMock.mockReturnValue({
      data: true,
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });
    openUrlMock.mockResolvedValue(undefined);
  });

  it("anchors the active tab border to the bottom edge of the tab bar", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Agent Plan",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        teamMode: null,
        teamConfig: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: "2026-04-23T10:00:00Z",
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        acceptanceStatus: "accepted",
      },
      proposals: [],
      messages: [],
    });

    renderPane(
      "tasks",
      workspace({ mode: "ideation", linkedIdeationSessionId: "session-1" }),
      vi.fn(),
      false,
      conversation(),
    );

    const tabRow = screen.getByTestId("agents-artifact-tab-row");
    const activeTab = await screen.findByTestId("agents-artifact-tab-tasks");
    const inactiveTab = screen.getByTestId("agents-artifact-tab-plan");

    expect(tabRow.getAttribute("style")).toContain(
      "border-color: var(--overlay-faint);"
    );
    expect(activeTab.parentElement?.className).toContain("self-stretch");
    expect(activeTab.className).toContain("self-stretch");
    expect(activeTab.getAttribute("data-theme-button-skip")).toBe("true");
    expect(inactiveTab.getAttribute("data-theme-button-skip")).toBe("true");
    expect(activeTab.className).not.toContain("border-b-2");
    expect(activeTab.querySelector("span[style='background: var(--accent-primary);']")).not.toBeNull();
    expect(inactiveTab.querySelector("span[style='background: var(--accent-primary);']")).toBeNull();
  });

  it("opens task details inside the Agents tasks artifact surface", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Agent Plan",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        teamMode: null,
        teamConfig: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: "2026-04-23T10:00:00Z",
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        acceptanceStatus: "accepted",
      },
      proposals: [],
      messages: [],
    });

    renderPane(
      "tasks",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
      { taskMode: "kanban" },
    );

    fireEvent.click(await screen.findByTestId("mock-agent-task-card"));

    expect(await screen.findByTestId("mock-agent-task-detail")).toHaveAttribute(
      "data-task-id",
      "task-1",
    );
  });

  it("renders only the publish tab for edit workspaces", () => {
    renderPane("publish", workspace({ mode: "edit" }));

    expect(screen.getByTestId("agents-artifact-tab-publish")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-plan")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-verification")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-proposal")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-tasks")).not.toBeInTheDocument();
  });

  it("surfaces git auth repair actions in the publish pane", () => {
    useGitAuthDiagnosticsMock.mockReturnValue({
      data: {
        fetchUrl: "https://github.com/mock/project.git",
        pushUrl: "git@github.com:mock/project.git",
        fetchKind: "HTTPS",
        pushKind: "SSH",
        mixedAuthModes: true,
        canSwitchToSsh: true,
        suggestedSshUrl: "git@github.com:mock/project.git",
      },
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });

    renderPane("publish", workspace({ mode: "edit" }));

    expect(screen.getByTestId("git-auth-repair-panel")).toBeInTheDocument();
    expect(screen.getByText(/Fetch and push use different auth modes/i)).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-switch-ssh")).toBeInTheDocument();
  });

  it("shows a GitHub PR sign-in action for all-SSH publish workspaces when gh is missing", () => {
    useGhAuthStatusMock.mockReturnValue({
      data: false,
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });

    renderPane("publish", workspace({ mode: "edit" }));

    expect(screen.getByTestId("git-auth-repair-panel")).toBeInTheDocument();
    expect(screen.getByText("GitHub PR Access")).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-login-gh")).toBeInTheDocument();
    expect(screen.queryByText(/Run gh auth login/i)).not.toBeInTheDocument();
  });

  it("renders the publish tab for ideation workspaces linked to execution branches", () => {
    renderPane(
      "publish",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "Open",
        publicationPushStatus: "pushed",
      }),
    );

    expect(screen.getByTestId("agents-artifact-tab-publish")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(screen.getByText("PR #90")).toBeInTheDocument();
  });

  it("does not directly publish pipeline-owned ideation workspaces", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    renderPane(
      "publish",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "Open",
        publicationPushStatus: "pushed",
      }),
      publish,
    );

    const publishButton = screen.getByTestId("agents-publish-confirm");
    expect(publishButton).toHaveTextContent("Managed by Tasks");
    expect(publishButton).toBeDisabled();

    fireEvent.click(publishButton);

    expect(publish).not.toHaveBeenCalled();
    expect(screen.getByTestId("agents-publish-actions-menu")).toBeEnabled();

    await userEvent.click(screen.getByTestId("agents-publish-actions-menu"));

    expect(await screen.findByTestId("agents-close-pr")).toHaveTextContent("Close PR");
  });

  it("allows PR maintenance actions for pipeline-owned ideation workspaces", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "old-base",
      targetBaseCommit: "new-base",
      isBaseAhead: true,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
    });
    updateWorkspaceFromBaseMock.mockResolvedValue({
      workspace: workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "Open",
        publicationPushStatus: "pushed",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "new-base",
      }),
      updated: true,
      targetRef: "origin/feature/agent-screen",
      baseCommit: "new-base",
    });

    renderPane(
      "publish",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "Open",
        publicationPushStatus: "pushed",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
      }),
      publish,
    );

    expect(await screen.findByTestId("agents-base-stale")).toHaveTextContent(
      "feature/agent-screen"
    );
    expect(screen.queryByTestId("agents-close-pr")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-actions-menu")).toBeEnabled();
    expect(screen.getByTestId("agents-update-from-base")).toBeEnabled();
    expect(screen.queryByTestId("agents-publish-confirm")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      })
    );
    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1")
    );

    await userEvent.click(screen.getByTestId("agents-publish-actions-menu"));
    await userEvent.click(await screen.findByTestId("agents-close-pr"));
    expect(closeWorkspacePrMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Close PR",
      })
    );
    await waitFor(() =>
      expect(closeWorkspacePrMock).toHaveBeenCalledWith("conversation-1")
    );
    expect(publish).not.toHaveBeenCalled();
  });

  it("renders the publish pane shell before hydrating git-backed publish facts", async () => {
    renderPane("publish", workspace({ mode: "edit" }));

    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(screen.getByText("Loading changed files...")).toBeInTheDocument();
    expect(getWorkspaceChangesMock).not.toHaveBeenCalled();
    expect(getWorkspaceFreshnessMock).not.toHaveBeenCalled();
    expect(listPublicationEventsMock).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(getWorkspaceChangesMock).toHaveBeenCalledWith("conversation-1")
    );
    expect(getWorkspaceFreshnessMock).toHaveBeenCalledWith("conversation-1");
    expect(listPublicationEventsMock).toHaveBeenCalledWith("conversation-1");
  });

  it("does not start ideation queries for edit workspace publish panes", async () => {
    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    await waitFor(() =>
      expect(getWorkspaceChangesMock).toHaveBeenCalledWith("conversation-1")
    );
    expect(useConversationMock).toHaveBeenCalledWith("conversation-1", {
      enabled: false,
      pageSize: 40,
    });
    expect(getIdeationSessionMock).not.toHaveBeenCalled();
    expect(useDependencyGraphMock).toHaveBeenCalledWith("");
    expect(useVerificationStatusMock).toHaveBeenCalledWith(undefined);
  });

  it("does not hydrate graph or verification data for the ideation plan tab", async () => {
    useConversationMock.mockReturnValue({
      data: {
        conversation: conversation(),
        messages: [
          {
            id: "message-1",
            conversationId: "conversation-1",
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
      },
      isLoading: false,
    });
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Agent Plan",
        titleSource: "auto",
        status: "active",
        planArtifactId: null,
        seedTaskId: null,
        parentSessionId: null,
        teamMode: null,
        teamConfig: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });

    renderPane(
      "plan",
      workspace({ mode: "ideation" }),
      vi.fn(),
      false,
      conversation(),
    );

    await waitFor(() => expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"));
    expect(useDependencyGraphMock).toHaveBeenCalledWith("");
    expect(useVerificationStatusMock).toHaveBeenCalledWith(undefined);
  });

  it("uses the focused ideation session as the artifact data source", async () => {
    useConversationMock.mockReturnValue({
      data: {
        conversation: conversation(),
        messages: [
          {
            id: "message-1",
            conversationId: "conversation-1",
            role: "assistant",
            content: "",
            toolCalls: [
              {
                id: "tool-1",
                name: "v1_start_ideation",
                arguments: {},
                result: { session_id: "session-from-workspace" },
              },
            ],
            contentBlocks: [],
            createdAt: "2026-04-23T09:00:00Z",
          },
        ],
      },
      isLoading: false,
    });
    getIdeationSessionMock.mockImplementation(async (sessionId: string) => ({
      session: {
        id: sessionId,
        projectId: "project-1",
        title: "Focused Plan",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        teamMode: null,
        teamConfig: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    }));

    renderPane(
      "plan",
      workspace({ mode: "ideation" }),
      vi.fn(),
      false,
      conversation(),
      { focusedIdeationSessionId: "session-focused" },
    );

    await waitFor(() =>
      expect(getIdeationSessionMock).toHaveBeenCalledWith("session-focused")
    );
    expect(getIdeationSessionMock).not.toHaveBeenCalledWith("session-from-workspace");
    expect(useConversationMock).toHaveBeenCalledWith("conversation-1", {
      enabled: false,
      pageSize: 40,
    });
  });

  it("focuses the newest verification child when the verification tab opens", async () => {
    const onFocusVerificationSession = vi.fn();
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Agent Plan",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        teamMode: null,
        teamConfig: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "verified",
        verificationInProgress: false,
        gapScore: 0,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getIdeationChildrenMock.mockResolvedValue([
      {
        id: "verification-old",
        projectId: "project-1",
        title: "Old verifier",
        titleSource: "auto",
        status: "active",
        planArtifactId: null,
        seedTaskId: null,
        parentSessionId: "session-1",
        teamMode: null,
        teamConfig: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "verification",
        acceptanceStatus: null,
      },
      {
        id: "verification-new",
        projectId: "project-1",
        title: "New verifier",
        titleSource: "auto",
        status: "active",
        planArtifactId: null,
        seedTaskId: null,
        parentSessionId: "session-1",
        teamMode: null,
        teamConfig: null,
        createdAt: "2026-04-23T10:00:00Z",
        updatedAt: "2026-04-23T10:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "verification",
        acceptanceStatus: null,
      },
    ]);
    useVerificationStatusMock.mockReturnValue({
      data: {
        sessionId: "session-1",
        status: "verified",
        inProgress: false,
        gaps: [],
        rounds: [],
        roundDetails: [],
        runHistory: [],
      },
      isLoading: false,
    });

    renderPane(
      "verification",
      workspace({ mode: "ideation", linkedIdeationSessionId: "session-1" }),
      vi.fn(),
      false,
      conversation(),
      { onFocusVerificationSession },
    );

    await waitFor(() =>
      expect(getIdeationChildrenMock).toHaveBeenCalledWith("session-1", "verification")
    );
    await waitFor(() =>
      expect(onFocusVerificationSession).toHaveBeenCalledWith(
        "session-1",
        "verification-new",
      )
    );
  });

  it("hides plan-derived tabs until the attached ideation run has a plan", async () => {
    useConversationMock.mockReturnValue({
      data: {
        conversation: conversation(),
        messages: [
          {
            id: "message-1",
            conversationId: "conversation-1",
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
      },
      isLoading: false,
    });
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Agent Plan",
        titleSource: "auto",
        status: "active",
        planArtifactId: null,
        seedTaskId: null,
        parentSessionId: null,
        teamMode: null,
        teamConfig: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });

    renderPane(
      "plan",
      workspace({ mode: "ideation" }),
      vi.fn(),
      false,
      conversation(),
    );

    await waitFor(() => expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"));
    expect(screen.queryByTestId("agents-artifact-tab-plan")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-verification")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-proposal")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-tasks")).not.toBeInTheDocument();
  });

  it("confirms publish from the publish pane", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    renderPane("publish", workspace({ mode: "edit" }), publish);

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));

    expect(publish).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Commit & Publish",
      })
    );

    await waitFor(() => expect(publish).toHaveBeenCalledWith("conversation-1"));
  });

  it("disables publish when no changed files are detected", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceChangesMock.mockResolvedValue([]);

    renderPane("publish", workspace({ mode: "edit" }), publish);

    const publishButton = await screen.findByTestId("agents-publish-confirm");
    await screen.findByText("No changed files detected yet.");
    await waitFor(() => expect(publishButton).toHaveTextContent("Commit & Publish"));
    expect(publishButton).toBeDisabled();

    fireEvent.click(publishButton);

    expect(publish).not.toHaveBeenCalled();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("disables publish once the workspace branch is pushed and current", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "base-sha",
      targetBaseCommit: "base-sha",
      isBaseAhead: false,
      hasUncommittedChanges: false,
      unpublishedCommitCount: 0,
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      }),
      publish,
    );

    const publishButton = await screen.findByTestId("agents-publish-confirm");
    await waitFor(() => expect(publishButton).toHaveTextContent("Published"));
    expect(publishButton).toBeDisabled();
    expect(screen.getByText("1 changed file published for review.")).toBeInTheDocument();

    fireEvent.click(publishButton);

    expect(publish).not.toHaveBeenCalled();
  });

  it("keeps publish enabled when a pushed workspace has new local commits", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "base-sha",
      targetBaseCommit: "base-sha",
      isBaseAhead: false,
      hasUncommittedChanges: false,
      unpublishedCommitCount: 1,
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      }),
      publish,
    );

    const publishButton = await screen.findByTestId("agents-publish-confirm");
    await waitFor(() => expect(publishButton).toHaveTextContent("Commit & Publish"));
    expect(publishButton).toBeEnabled();

    fireEvent.click(publishButton);
    expect(publish).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Commit & Publish",
      })
    );

    await waitFor(() => expect(publish).toHaveBeenCalledWith("conversation-1"));
  });

  it("opens the published PR from the publish pane", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
      }),
    );

    fireEvent.click(await screen.findByTestId("agents-open-pr"));

    expect(openUrlMock).toHaveBeenCalledWith("https://github.com/mock/project/pull/78");
  });

  it("uses the review subtitle for purpose and shows the readable PR URL", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
      }),
    );

    expect(
      screen.getByText("Review this agent workspace before publishing its draft PR.")
    ).toBeInTheDocument();
    expect(screen.queryByText(/Project default \(main\) →/)).not.toBeInTheDocument();
    const prUrl = await screen.findByTestId("agents-open-pr-url");
    expect(prUrl).toHaveTextContent("github.com/mock/project/pull/78");
    fireEvent.click(prUrl);

    expect(openUrlMock).toHaveBeenCalledWith("https://github.com/mock/project/pull/78");
  });

  it("uses Update from base as the primary action when the base branch moved", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "old-base",
      targetBaseCommit: "new-base",
      isBaseAhead: true,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
    });
    updateWorkspaceFromBaseMock.mockResolvedValue({
      workspace: workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "new-base",
      }),
      updated: true,
      targetRef: "origin/feature/agent-screen",
      baseCommit: "new-base",
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
      }),
      publish,
    );

    expect(await screen.findByTestId("agents-base-stale")).toHaveTextContent(
      "feature/agent-screen"
    );
    expect(screen.getByTestId("agents-publish-status-pill")).toHaveAttribute(
      "style",
      expect.stringContaining("border-color: var(--overlay-weak)"),
    );
    expect(screen.getByTestId("agents-publish-status-pill")).toHaveAttribute(
      "style",
      expect.stringContaining("color: var(--text-secondary)"),
    );
    expect(screen.getByTestId("agents-base-stale")).toHaveAttribute(
      "style",
      expect.stringContaining("border-color: var(--border-subtle)"),
    );
    expect(screen.getByTestId("agents-base-stale-icon")).toHaveAttribute(
      "style",
      expect.stringContaining("color: var(--status-warning)"),
    );
    expect(screen.getByTestId("agents-base-stale")).not.toHaveTextContent(
      "Update this workspace before publishing"
    );
    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      })
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1")
    );
    expect(publish).not.toHaveBeenCalled();
  });

  it("treats merged pull requests as terminal even if the old base moved", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        publicationPrNumber: 91,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      }),
      publish,
    );

    const publishButton = await screen.findByTestId("agents-publish-confirm");
    expect(publishButton).toHaveTextContent("Merged");
    expect(publishButton).toBeDisabled();
    expect(screen.queryByTestId("agents-base-stale")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-update-from-base")).not.toBeInTheDocument();
    expect(
      screen.getByText(
        "PR #91 has been merged. By continuing this conversation, a new workspace branch will be created automatically."
      )
    ).toBeInTheDocument();
    expect(getWorkspaceFreshnessMock).not.toHaveBeenCalled();

    fireEvent.click(publishButton);

    expect(publish).not.toHaveBeenCalled();
  });

  it("locks the base update action while agent repair is pending", async () => {
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "old-base",
      targetBaseCommit: "new-base",
      isBaseAhead: true,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
        publicationPushStatus: "needs_agent",
      }),
    );

    const updateButton = await screen.findByTestId("agents-update-from-base");
    expect(updateButton).toBeDisabled();

    updateWorkspaceFromBaseMock.mockClear();
    fireEvent.click(updateButton);

    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
  });

  it("loads workspace changes for review before publishing", async () => {
    renderPane("publish", workspace({ mode: "edit" }));

    await waitFor(() => expect(screen.getByTestId("agents-review-changes")).toBeEnabled());
    expect(getWorkspaceChangesMock).toHaveBeenCalledWith("conversation-1");
  });

  it("shows workspace branch commits in the review dialog history tab", async () => {
    const user = userEvent.setup();
    getWorkspaceCommitsMock.mockResolvedValue([
      {
        sha: "abc123def456",
        shortSha: "abc123d",
        message: "Update Codex model catalog",
        author: "Agent",
        date: new Date("2026-04-26T09:00:00Z"),
      },
    ]);
    renderPane("publish", workspace({ mode: "edit" }));

    await waitFor(() => expect(screen.getByTestId("agents-review-changes")).toBeEnabled());
    fireEvent.click(screen.getByTestId("agents-review-changes"));
    await waitFor(() =>
      expect(getWorkspaceCommitsMock).toHaveBeenCalledWith("conversation-1")
    );
    await user.click(
      await screen.findByTestId("tab-history", undefined, deferredHydrationTimeout)
    );

    expect(
      await screen.findByTestId("commit-abc123d", undefined, deferredHydrationTimeout)
    ).toHaveTextContent("Update Codex model catalog");
  });

  it("shows workspace publish pipeline status only during active publishing", () => {
    renderPane(
      "publish",
      workspace({ mode: "edit", publicationPushStatus: "pushing" }),
      vi.fn(),
      true,
    );

    expect(screen.getByTestId("agents-publish-pipeline")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-step-checking")).toHaveTextContent(
      "Check workspace"
    );
    expect(screen.getByTestId("agents-publish-step-refreshing")).toHaveTextContent(
      "Refresh branch"
    );
  });

  it("hides the publish pipeline after agent repair terminal state", () => {
    renderPane("publish", workspace({ mode: "edit", publicationPushStatus: "needs_agent" }));

    expect(screen.queryByTestId("agents-publish-pipeline")).not.toBeInTheDocument();
  });

  it("renders durable publish history in the publish pane", async () => {
    listPublicationEventsMock.mockResolvedValue([
      {
        id: "event-1",
        conversationId: "conversation-1",
        step: "refreshing",
        status: "started",
        summary: "Refreshing branch from base",
        classification: null,
        createdAt: "2026-04-26T09:01:00Z",
      },
      {
        id: "event-2",
        conversationId: "conversation-1",
        step: "needs_agent",
        status: "failed",
        summary: "Pre-commit hook failed",
        classification: "agent_fixable",
        createdAt: "2026-04-26T09:02:00Z",
      },
    ]);

    renderPane("publish", workspace({ mode: "edit", publicationPushStatus: "needs_agent" }));

    expect(
      await screen.findByTestId(
        "agents-publish-events",
        undefined,
        deferredHydrationTimeout,
      )
    ).toBeInTheDocument();
    expect(screen.queryByText("Pre-commit hook failed")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agents-publish-history-toggle"));
    expect(screen.getByText("Pre-commit hook failed")).toBeInTheDocument();
    expect(screen.getByText(/agent fixable/i)).toBeInTheDocument();
  });

  it("hides old started publish history rows after publish completes", async () => {
    listPublicationEventsMock.mockResolvedValue([
      {
        id: "event-checking",
        conversationId: "conversation-1",
        step: "checking",
        status: "started",
        summary: "Checking workspace changes",
        classification: null,
        createdAt: "2026-04-26T09:01:00Z",
      },
      {
        id: "event-pushing",
        conversationId: "conversation-1",
        step: "pushing",
        status: "started",
        summary: "Pushing agent branch",
        classification: null,
        createdAt: "2026-04-26T09:02:00Z",
      },
      {
        id: "event-published",
        conversationId: "conversation-1",
        step: "published",
        status: "succeeded",
        summary: "Draft pull request is ready",
        classification: null,
        createdAt: "2026-04-26T09:03:00Z",
      },
    ]);

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      }),
    );

    expect(
      await screen.findByTestId(
        "agents-publish-events",
        undefined,
        deferredHydrationTimeout,
      )
    ).toBeInTheDocument();
    expect(screen.queryByText("Checking workspace changes")).not.toBeInTheDocument();
    expect(screen.queryByText("Pushing agent branch")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agents-publish-history-toggle"));
    expect(screen.queryByText("Checking workspace changes")).not.toBeInTheDocument();
    expect(screen.queryByText("Pushing agent branch")).not.toBeInTheDocument();
    expect(screen.getByText("Draft pull request is ready")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-event-icon-event-published"))
      .toHaveAttribute("data-state", "succeeded");
  });

  it("shows only the latest started publish history row while publishing", async () => {
    listPublicationEventsMock.mockResolvedValue([
      {
        id: "event-checking",
        conversationId: "conversation-1",
        step: "checking",
        status: "started",
        summary: "Checking workspace changes",
        classification: null,
        createdAt: "2026-04-26T09:01:00Z",
      },
      {
        id: "event-pushing",
        conversationId: "conversation-1",
        step: "pushing",
        status: "started",
        summary: "Pushing agent branch",
        classification: null,
        createdAt: "2026-04-26T09:02:00Z",
      },
    ]);

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "pushing",
      }),
      vi.fn(),
      true,
    );

    expect(
      await screen.findByTestId(
        "agents-publish-events",
        undefined,
        deferredHydrationTimeout,
      )
    ).toBeInTheDocument();
    expect(screen.queryByText("Checking workspace changes")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agents-publish-history-toggle"));
    expect(screen.queryByText("Checking workspace changes")).not.toBeInTheDocument();
    expect(screen.getByText("Pushing agent branch")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-event-icon-event-pushing"))
      .toHaveAttribute("data-state", "active");
  });
});
