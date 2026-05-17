import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { AgentConversationWorkspace } from "@/api/chat";
import type { AgentArtifactTab } from "@/stores/agentSessionStore";
import { createTestQueryClient } from "@/test/store-utils";
import { AgentsArtifactPane } from "./AgentsArtifactPane";

const {
  getWorkspaceChangesMock,
  getWorkspaceReviewMock,
  getWorkspaceDiffMock,
  getWorkspaceCommitsMock,
  getWorkspaceCommitChangesMock,
  getWorkspaceCommitDiffMock,
  getWorkspacePrAnnotationsMock,
  listPublicationEventsMock,
  getWorkspaceFreshnessMock,
  updateWorkspaceFromBaseMock,
  getArtifactMock,
  getIdeationSessionMock,
  useConversationMock,
  useDependencyGraphMock,
  useVerificationStatusMock,
  openUrlMock,
  toastErrorMock,
  toastSuccessMock,
} = vi.hoisted(() => ({
  getWorkspaceChangesMock: vi.fn(),
  getWorkspaceReviewMock: vi.fn(),
  getWorkspaceDiffMock: vi.fn(),
  getWorkspaceCommitsMock: vi.fn(),
  getWorkspaceCommitChangesMock: vi.fn(),
  getWorkspaceCommitDiffMock: vi.fn(),
  getWorkspacePrAnnotationsMock: vi.fn(),
  listPublicationEventsMock: vi.fn(),
  getWorkspaceFreshnessMock: vi.fn(),
  updateWorkspaceFromBaseMock: vi.fn(),
  getArtifactMock: vi.fn(),
  getIdeationSessionMock: vi.fn(),
  useConversationMock: vi.fn(),
  useDependencyGraphMock: vi.fn(),
  useVerificationStatusMock: vi.fn(),
  openUrlMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastSuccessMock: vi.fn(),
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
    },
  };
});

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceFileChanges: (...args: unknown[]) =>
      getWorkspaceChangesMock(...args),
    getAgentConversationWorkspaceReview: (...args: unknown[]) =>
      getWorkspaceReviewMock(...args),
    getAgentConversationWorkspaceFileDiff: (...args: unknown[]) =>
      getWorkspaceDiffMock(...args),
    getAgentConversationWorkspaceCommits: (...args: unknown[]) =>
      getWorkspaceCommitsMock(...args),
    getAgentConversationWorkspaceCommitFileChanges: (...args: unknown[]) =>
      getWorkspaceCommitChangesMock(...args),
    getAgentConversationWorkspaceCommitFileDiff: (...args: unknown[]) =>
      getWorkspaceCommitDiffMock(...args),
    getAgentConversationWorkspacePrAnnotations: (...args: unknown[]) =>
      getWorkspacePrAnnotationsMock(...args),
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
      },
    },
  };
});

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

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (...args: unknown[]) => openUrlMock(...args),
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function renderPane(
  activeTab: AgentArtifactTab = "tasks",
  paneWorkspace = workspace(),
  onPublishWorkspace = vi.fn(),
  isPublishingWorkspace = false,
  paneConversation = null,
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
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "frontend/src/App.tsx",
          status: "modified",
          additions: 4,
          deletions: 1,
        },
      ],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
    });
    getWorkspaceDiffMock.mockResolvedValue({
      filePath: "frontend/src/App.tsx",
      language: "typescript",
      hunks: [
        {
          oldStart: 1,
          oldLines: 1,
          newStart: 1,
          newLines: 1,
          header: "@@ -1,1 +1,1 @@",
          lines: [
            { kind: "deletion", content: "old", oldLineNum: 1, newLineNum: null },
            { kind: "addition", content: "new", oldLineNum: null, newLineNum: 1 },
          ],
        },
      ],
      oldTotalLines: 1,
      newTotalLines: 1,
      isBinary: false,
    });
    getWorkspaceCommitsMock.mockResolvedValue([]);
    getWorkspaceCommitChangesMock.mockResolvedValue([
      { path: "frontend/src/App.tsx", status: "modified", additions: 4, deletions: 1 },
    ]);
    getWorkspaceCommitDiffMock.mockResolvedValue({
      filePath: "frontend/src/App.tsx",
      language: "typescript",
      hunks: [
        {
          oldStart: 1,
          oldLines: 1,
          newStart: 1,
          newLines: 1,
          header: "@@ -1,1 +1,1 @@",
          lines: [
            { kind: "deletion", content: "old", oldLineNum: 1, newLineNum: null },
            { kind: "addition", content: "new", oldLineNum: null, newLineNum: 1 },
          ],
        },
      ],
      oldTotalLines: 1,
      newTotalLines: 1,
      isBinary: false,
    });
    getWorkspacePrAnnotationsMock.mockResolvedValue({
      prNumber: 78,
      headSha: "head-sha",
      annotations: [],
      sourcesUnavailable: [],
    });
    listPublicationEventsMock.mockResolvedValue([]);
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      freshnessScope: "full",
      baseRef: "main",
      baseDisplayName: "Project default (main)",
      targetRef: "origin/main",
      capturedBaseCommit: "base-sha",
      targetBaseCommit: "base-sha",
      isBaseAhead: false,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
      remoteRefreshed: true,
      worktreeStatusChecked: true,
    });
    updateWorkspaceFromBaseMock.mockResolvedValue({
      workspace: workspace({ mode: "edit", baseCommit: "base-sha" }),
      updated: false,
      targetRef: "origin/main",
      baseCommit: "base-sha",
    });
    getArtifactMock.mockResolvedValue(null);
    getIdeationSessionMock.mockResolvedValue(null);
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
    openUrlMock.mockResolvedValue(undefined);
    toastErrorMock.mockClear();
    toastSuccessMock.mockClear();
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
      "border-color: var(--border-subtle);"
    );
    expect(activeTab.parentElement?.className).toContain("self-stretch");
    expect(activeTab.className).toContain("self-stretch");
    expect(activeTab.getAttribute("data-theme-button-skip")).toBe("true");
    expect(inactiveTab.getAttribute("data-theme-button-skip")).toBe("true");
    expect(activeTab.className).not.toContain("border-b-2");
    expect(activeTab.querySelector("span[style='background: var(--accent-primary);']")).not.toBeNull();
    expect(inactiveTab.querySelector("span[style='background: var(--accent-primary);']")).toBeNull();
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

  it("renders the publish pane shell before hydrating git-backed publish facts", async () => {
    renderPane("publish", workspace({ mode: "edit" }));

    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(screen.getByText("Review changes before publishing.")).toBeInTheDocument();
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
    expect(getWorkspaceChangesMock).not.toHaveBeenCalled();
    expect(getWorkspaceFreshnessMock).not.toHaveBeenCalled();
    expect(listPublicationEventsMock).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(getWorkspaceFreshnessMock).toHaveBeenCalledWith("conversation-1", {
        scope: "full",
      })
    );
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
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
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

  it("confirms publish from the publish pane", () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    renderPane("publish", workspace({ mode: "edit" }), publish);

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));

    expect(publish).toHaveBeenCalledWith("conversation-1");
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
    await screen.findByText("1 changed file published for review.");

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

    expect(publish).toHaveBeenCalledWith("conversation-1");
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
    expect(screen.getByTestId("agents-base-stale")).not.toHaveTextContent(
      "Update this workspace before publishing"
    );
    fireEvent.click(screen.getByTestId("agents-update-from-base"));

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1")
    );
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
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("agents-review-changes"));
    await waitFor(() =>
      expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1"),
    );
  });

  it("precomputes the PR description after review changes load", async () => {
    renderPane("publish", workspace({ mode: "edit" }));

    fireEvent.click(await screen.findByTestId("agents-review-changes"));

    await waitFor(() =>
      expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1"),
    );
    await waitFor(() =>
      expect(precomputePrDescriptionMock).toHaveBeenCalledWith("conversation-1"),
    );
  });

  it("shows workspace branch commits in the review dialog history tab", async () => {
    const user = userEvent.setup();
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "frontend/src/App.tsx",
          status: "modified",
          additions: 4,
          deletions: 1,
        },
      ],
      commits: [
        {
          sha: "abc123def456",
          shortSha: "abc123d",
          message: "Update Codex model catalog",
          author: "Agent",
          date: new Date("2026-04-26T09:00:00Z"),
        },
      ],
      baseRef: "main",
      headRef: "HEAD",
    });
    renderPane("publish", workspace({ mode: "edit" }));

    await waitFor(() => expect(screen.getByTestId("agents-review-changes")).toBeEnabled());
    fireEvent.click(screen.getByTestId("agents-review-changes"));
    await waitFor(() =>
      expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1")
    );
    await user.click(await screen.findByTestId("tab-history"));

    expect(await screen.findByTestId("commit-abc123d")).toHaveTextContent(
      "Update Codex model catalog"
    );
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

    expect(await screen.findByTestId("agents-publish-events")).toBeInTheDocument();
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

    expect(await screen.findByTestId("agents-publish-events")).toBeInTheDocument();
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

    expect(await screen.findByTestId("agents-publish-events")).toBeInTheDocument();
    expect(screen.queryByText("Checking workspace changes")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agents-publish-history-toggle"));
    expect(screen.queryByText("Checking workspace changes")).not.toBeInTheDocument();
    expect(screen.getByText("Pushing agent branch")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-event-icon-event-pushing"))
      .toHaveAttribute("data-state", "active");
  });
});
