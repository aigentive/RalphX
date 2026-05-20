import {
  getAgentsViewTestMocks,
  mockAgentViewData,
  renderAgentsView,
  selectSidebarConversationRow,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { useChatStore } from "@/stores/chatStore";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";
import { getAgentConversationStoreKey } from "./agentConversations";

const deferredHydrationTimeout = { timeout: 3_000 };

const {
  getAgentConversationWorkspaceFreshnessMock,
  getAgentConversationWorkspaceMock,
  getAgentRunningStatesMock,
  getWorkspaceDiffMock,
  getWorkspaceReviewMock,
  getWorkspaceStagedChangesMock,
  getWorkspaceUnstagedChangesMock,
  listAgentTasksMock,
  publishAgentConversationWorkspaceMock,
  sendAgentMessageMock,
  toastErrorMock,
} = getAgentsViewTestMocks();

describe("AgentsView publish", () => {
  beforeEach(setupAgentsViewTest);

  it("opens the right-side publish pane from the Commit & Publish header shortcut", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));

    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId("agents-publish-workspace");
    expect(screen.queryByTestId("agents-artifact-pane")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-pane")).toBeInTheDocument()
    );
    expect(publishAgentConversationWorkspaceMock).not.toHaveBeenCalled();
  });

  it("shows a composer workspace changes summary without fetching file diffs", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "src/Foo.tsx",
          status: "modified",
          additions: 10,
          deletions: 2,
          isGenerated: false,
        },
        {
          path: "src/generated.ts",
          status: "added",
          additions: 4,
          deletions: 0,
          isGenerated: true,
        },
      ],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
      supportsWorktreeModes: true,
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId(
      "agents-composer-workspace-changes",
      undefined,
      deferredHydrationTimeout,
    );
    expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Workspace changes");
    expect(screen.getByTestId("agents-composer-workspace-changes-count")).toHaveTextContent(
      "2 files",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-additions")).toHaveTextContent(
      "+14",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-deletions")).toHaveTextContent(
      "−2",
    );

    expect(
      screen.queryByTestId("agents-composer-workspace-changes-list"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-composer-workspace-changes-header"));

    expect(screen.getByTestId("agents-composer-workspace-file-src/Foo.tsx")).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-composer-workspace-file-src/generated.ts"),
    ).toHaveTextContent("Generated");
    expect(getWorkspaceDiffMock).not.toHaveBeenCalled();
  });

  it("prefers unstaged files in the composer workspace summary when dirty files exist", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "src/Workspace.tsx",
          status: "modified",
          additions: 20,
          deletions: 1,
          isGenerated: false,
        },
      ],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
      supportsWorktreeModes: true,
    });
    getWorkspaceStagedChangesMock.mockResolvedValue([
      {
        path: "src/Staged.tsx",
        status: "modified",
        additions: 7,
        deletions: 2,
        isGenerated: false,
      },
    ]);
    getWorkspaceUnstagedChangesMock.mockResolvedValue([
      {
        path: "src/Unstaged.tsx",
        status: "modified",
        additions: 3,
        deletions: 4,
        isGenerated: false,
      },
    ]);

    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId(
      "agents-composer-workspace-changes",
      undefined,
      deferredHydrationTimeout,
    );
    await waitFor(() =>
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Unstaged"),
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-count")).toHaveTextContent(
      "1 file",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-additions")).toHaveTextContent(
      "+3",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-deletions")).toHaveTextContent(
      "−4",
    );

    fireEvent.click(screen.getByTestId("diff-filter-trigger"));

    expect(
      screen.getByTestId("agents-composer-workspace-file-src/Unstaged.tsx"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-composer-workspace-file-src/Workspace.tsx"),
    ).not.toBeInTheDocument();
  });

  it("prefers staged files in the composer workspace summary when no unstaged files exist", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "src/Workspace.tsx",
          status: "modified",
          additions: 20,
          deletions: 1,
          isGenerated: false,
        },
      ],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
      supportsWorktreeModes: true,
    });
    getWorkspaceStagedChangesMock.mockResolvedValue([
      {
        path: "src/Staged.tsx",
        status: "modified",
        additions: 7,
        deletions: 2,
        isGenerated: false,
      },
    ]);
    getWorkspaceUnstagedChangesMock.mockResolvedValue([]);

    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId(
      "agents-composer-workspace-changes",
      undefined,
      deferredHydrationTimeout,
    );
    await waitFor(() =>
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Staged"),
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-count")).toHaveTextContent(
      "1 file",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-additions")).toHaveTextContent(
      "+7",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-deletions")).toHaveTextContent(
      "−2",
    );
  });

  it("switches the composer context tray between tasks and changes", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    listAgentTasksMock.mockResolvedValue([
      {
        taskId: "task-1",
        taskNumber: 1,
        title: "Add runtime task output shim",
        state: "active",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: ["task-2"],
        availability: "ready",
        updatedAt: "2026-05-20T10:00:00Z",
      },
      {
        taskId: "task-2",
        taskNumber: 2,
        title: "Render ledger rows",
        state: "open",
        ownerAgent: null,
        blockedBy: ["task-1"],
        blocks: [],
        availability: "blocked",
        updatedAt: "2026-05-20T10:01:00Z",
      },
    ]);
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "src/Foo.tsx",
          status: "modified",
          additions: 10,
          deletions: 2,
          isGenerated: false,
        },
      ],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
      supportsWorktreeModes: true,
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId(
      "agents-composer-context-tray",
      undefined,
      deferredHydrationTimeout,
    );

    const taskToggle = screen.getByTestId("agents-composer-tasks-toggle");
    const changesToggle = screen.getByTestId("diff-filter-trigger");
    expect(taskToggle.compareDocumentPosition(changesToggle)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(taskToggle).toHaveTextContent("Tasks");
    expect(taskToggle).toHaveTextContent("2");
    expect(changesToggle).toHaveTextContent("Workspace changes");

    fireEvent.click(screen.getByTestId("agents-composer-tasks-toggle"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-composer-task-list")).toHaveTextContent(
        "Add runtime task output shim",
      ),
    );
    expect(screen.getByTestId("agents-composer-task-2")).toHaveTextContent(
      "blocked by #1",
    );
    expect(
      screen.queryByTestId("agents-composer-workspace-changes-list"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("diff-filter-trigger"));

    expect(screen.getByTestId("agents-composer-workspace-changes-list")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-composer-task-list")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("diff-filter-trigger"));

    expect(screen.queryByTestId("agents-composer-context-tray-body")).not.toBeInTheDocument();
  });

  it("auto-expands the composer task ledger for live task updates", async () => {
    const activeConversation = conversation({ agentMode: "edit" });
    mockAgentViewData(activeConversation);
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getAgentRunningStatesMock.mockResolvedValue({ [activeConversation.id]: true });
    listAgentTasksMock.mockResolvedValue([
      {
        taskId: "task-1",
        taskNumber: 1,
        title: "Smoke the task ledger",
        state: "active",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:00:00Z",
      },
    ]);
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
      supportsWorktreeModes: true,
    });

    renderAgentsView();
    selectSidebarConversationRow();
    act(() => {
      useChatStore.setState({
        agentStatus: {
          [getAgentConversationStoreKey(activeConversation)]: "generating",
        },
      });
    });

    await screen.findByTestId(
      "agents-composer-task-list",
      undefined,
      deferredHydrationTimeout,
    );
    expect(screen.getByTestId("agents-composer-tasks-toggle")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByTestId("agents-composer-task-1")).toHaveTextContent(
      "Smoke the task ledger",
    );
    expect(screen.getByTestId("agents-composer-task-1")).toHaveStyle({
      backgroundColor: "var(--bg-hover)",
    });
  });

  it("opens the publish pane with a focused file request from the composer summary", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "src/Foo.tsx",
          status: "modified",
          additions: 10,
          deletions: 2,
          isGenerated: false,
        },
      ],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
      supportsWorktreeModes: true,
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId(
      "agents-composer-workspace-changes",
      undefined,
      deferredHydrationTimeout,
    );
    fireEvent.click(screen.getByTestId("agents-composer-workspace-changes-count"));
    fireEvent.click(screen.getByTestId("agents-composer-workspace-file-src/Foo.tsx"));

    const pane = await screen.findByTestId("agents-artifact-pane");
    expect(pane).toHaveAttribute("data-active-tab", "publish");
    expect(pane).toHaveAttribute("data-publish-focus-path", "src/Foo.tsx");
    expect(pane).toHaveAttribute("data-publish-focus-mode", "uncommitted");
  });

  it("shows Update from base in the header shortcut when the workspace base moved", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
      })
    );
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue({
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

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Update from feature/agent-screen"
      )
    );
  });

  it("shows Base unavailable in the header shortcut when backend blocks the saved base", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "edit",
        baseRef: "feature/deleted-base",
        baseDisplayName: "Current branch (feature/deleted-base)",
      })
    );
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/deleted-base",
      baseDisplayName: "Current branch (feature/deleted-base)",
      targetRef: "",
      capturedBaseCommit: "old-base",
      targetBaseCommit: "",
      isBaseAhead: false,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
      baseStatus: "blocked",
      effectiveBaseRef: null,
      effectiveBaseDisplayName: null,
      baseBlockReason: "Saved base commit is not contained in the default branch",
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Base unavailable"
      )
    );
  });

  it("shows Update from base in the header shortcut for ideation plan-branch workspaces", async () => {
    mockAgentViewData(conversation({ agentMode: "ideation" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "ideation",
        status: "missing",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "Open",
        publicationPushStatus: "pushed",
        baseRef: "main",
        baseDisplayName: "Project default (main)",
      })
    );
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue({
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

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Update from feature/agent-screen"
      )
    );
    expect(screen.getByTestId("agents-publish-workspace")).toBeEnabled();
  });

  it("shows merged terminal state instead of Update from base in the header shortcut", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        publicationPrNumber: 91,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      })
    );

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Merged"
      )
    );
    expect(screen.getByTestId("agents-publish-workspace")).not.toHaveTextContent(
      "Update from feature/agent-screen"
    );
    expect(getAgentConversationWorkspaceFreshnessMock).not.toHaveBeenCalled();
  });

  it("shows Published in the header shortcut when the workspace branch is already current", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "edit",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      })
    );
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue({
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

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Published"
      )
    );
  });

  it("relies on the backend to route fixable publish failures into the workspace agent conversation", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock
      .mockResolvedValueOnce(conversationWorkspace({ mode: "edit" }))
      .mockResolvedValueOnce(
        conversationWorkspace({ mode: "edit", publicationPushStatus: "needs_agent" })
      );
    publishAgentConversationWorkspaceMock.mockRejectedValue(
      "Failed to commit: typecheck failed"
    );
    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId("agents-publish-workspace");
    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    await screen.findByTestId("agents-publish-confirm");
    fireEvent.click(screen.getByTestId("agents-publish-confirm"));

    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalledTimes(3));
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
    expect(toastErrorMock).toHaveBeenCalledWith(
      "Publish failed. Sent the error to the agent to fix."
    );
  });

  it("does not send operational publish failures to the workspace agent", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock
      .mockResolvedValueOnce(conversationWorkspace({ mode: "edit" }))
      .mockResolvedValueOnce(
        conversationWorkspace({ mode: "edit", publicationPushStatus: "failed" })
      );
    publishAgentConversationWorkspaceMock.mockRejectedValue(
      "GitHub integration is not available"
    );
    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId("agents-publish-workspace");
    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    await screen.findByTestId("agents-publish-confirm");
    fireEvent.click(screen.getByTestId("agents-publish-confirm"));

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "GitHub integration is not available"
      )
    );
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
  });

});
