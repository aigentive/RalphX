import {
  getAgentsViewTestMocks,
  mockAgentViewData,
  renderAgentsView,
  selectSidebarConversationRow,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";

const {
  getAgentConversationWorkspaceFreshnessMock,
  getAgentConversationWorkspaceMock,
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

    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalledTimes(2));
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
