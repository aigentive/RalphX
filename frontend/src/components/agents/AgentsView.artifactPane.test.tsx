import {
  fireAgentViewEvent,
  getAgentsViewTestMocks,
  mockAgentViewData,
  mockAgentSidebarData,
  mockSessionWithData,
  renderAgentsView,
  resetAgentSessionState,
  selectSidebarConversationRow,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { QueryClient } from "@tanstack/react-query";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { ideationKeys } from "@/hooks/useIdeation";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";

const {
  artifactPaneRenderMock,
  getAgentConversationWorkspaceMock,
  getWorkspaceReviewContextMock,
  startWorkspaceReviewMock,
  useConversationMock,
  useProjectAgentConversationsMock,
} = getAgentsViewTestMocks();

const workspaceReviewTarget = {
  scope: "workspace_delta",
  baseRef: "main",
  baseSha: "base-sha",
  headRef: "HEAD",
  headSha: "head-sha",
  diffFingerprint: "fingerprint-1",
  sourcePullRequestNumber: null,
};

function workspaceReviewContext(overrides: {
  conversationId?: string;
  status?: "idle" | "ready" | "reviewing" | "blocked";
  reviewOutcome?: "none" | "passed" | "failed";
  reviewGateStatus?: "none" | "reviewing" | "passed" | "failed";
  isOutdated?: boolean;
  isCurrent?: boolean;
  shouldShowTab?: boolean;
} = {}) {
  const conversationId = overrides.conversationId ?? "conversation-1";
  const reviewArtifactIsCurrent = overrides.isCurrent ?? false;
  const reviewArtifactIsOutdated = overrides.isOutdated ?? true;
  return {
    success: true,
    workspace: conversationWorkspace({ conversationId, mode: "edit" }),
    events: [],
    target: workspaceReviewTarget,
    monitor: {
      conversationId,
      projectId: "project-1",
      status: overrides.status ?? "ready",
      currentTargetScope: "workspace_delta",
      reviewedTargetScope: "workspace_delta",
      reviewOutcome:
        overrides.reviewOutcome ??
        (overrides.reviewGateStatus === "passed" ? "passed" : "none"),
      reviewGateStatus: overrides.reviewGateStatus ?? null,
      reviewConversationId: "review-conversation-1",
      reviewArtifactId: "review-artifact-1",
      reviewArtifactVersion: 1,
      reviewArtifactUpdatedAt: "2026-04-23T09:30:00Z",
      reviewGateBypassedAt: null,
      reviewGateBypassedTargetScope: null,
      reviewGateBypassedDiffFingerprint: null,
      reviewGateBypassedArtifactId: null,
      reviewGateBypassedArtifactVersion: null,
      reviewedHeadSha: "previous-head-sha",
      reviewedDiffFingerprint: "previous-fingerprint",
      selectedSourceBaseRef: null,
      selectedSourceBaseSha: null,
      selectedSourceHeadRef: null,
      selectedSourceHeadSha: null,
      selectedSourcePullRequestNumber: null,
      workspaceBaseRef: "main",
      workspaceBaseSha: "base-sha",
      workspaceHeadRef: "HEAD",
      workspaceHeadSha: "head-sha",
      currentDiffFingerprint: workspaceReviewTarget.diffFingerprint,
      previousVersionId: null,
      lastRunId: null,
      lastError: null,
      createdAt: "2026-04-23T09:00:00Z",
      updatedAt: "2026-04-23T09:30:00Z",
    },
    reviewArtifactIsCurrent,
    reviewArtifactIsOutdated,
    canMutateReviewState: false,
    reviewRuntimeState: "missing_runtime_identity",
    isCurrent: reviewArtifactIsCurrent,
    isOutdated: reviewArtifactIsOutdated,
    shouldShowTab: overrides.shouldShowTab ?? true,
  };
}

describe("AgentsView artifact pane", () => {
  beforeEach(setupAgentsViewTest);

  // Warm the lazily imported pane module so the first pane-mounting test does
  // not pay the one-time dynamic-import transform cost inside findBy's 1s
  // timeout on loaded CI runners.
  beforeAll(async () => {
    await import("./AgentsArtifactPane");
  });

  it("restores persisted artifact width, enforces pane and chat minimums, and resets to default on double click", async () => {
    window.localStorage.setItem("ralphx-agents-artifact-width", "720");
    mockAgentViewData(
      conversation({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
      })
    );
    mockSessionWithData({ planArtifactId: "plan-1" });
    resetAgentSessionState({
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "plan",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();

    const pane = await screen.findByTestId("agents-artifact-resizable-pane");
    expect(pane).toHaveStyle({
      width: "720px",
      minWidth: "600px",
      maxWidth: "calc(100% - 600px)",
    });

    fireEvent.doubleClick(screen.getByTestId("agents-artifact-resize-handle"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-resizable-pane")).toHaveStyle({
        width: "66.666667%",
      })
    );
    expect(window.localStorage.getItem("ralphx-agents-artifact-width")).toBeNull();
  });

  it("resizes the artifact pane when the handle is dragged", async () => {
    mockAgentViewData(
      conversation({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
      })
    );
    mockSessionWithData({ planArtifactId: "plan-1" });
    resetAgentSessionState({
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "plan",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId("agents-artifact-resizable-pane");

    const splitContainer = screen.getByTestId("agents-split-container");
    const rectSpy = vi.spyOn(splitContainer, "getBoundingClientRect").mockReturnValue({
      x: 100,
      y: 0,
      width: 1200,
      height: 800,
      top: 0,
      right: 1300,
      bottom: 800,
      left: 100,
      toJSON: () => ({}),
    });

    fireEvent.mouseDown(screen.getByTestId("agents-artifact-resize-handle"));
    fireEvent.mouseMove(document, { clientX: 940 });
    fireEvent.mouseMove(document, { clientX: 920 });
    fireEvent.mouseMove(document, { clientX: 900 });
    fireEvent.mouseUp(document);

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-resizable-pane")).toHaveStyle({
        width: "600px",
      })
    );
    expect(window.localStorage.getItem("ralphx-agents-artifact-width")).toBe("600");
    expect(rectSpy).toHaveBeenCalledTimes(1);
  });

  it("keeps the artifact pane closed by default when the conversation has nothing to show", async () => {
    mockAgentViewData(
      conversation({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
      })
    );
    mockSessionWithData();
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-content-width-class",
      "max-w-[980px]"
    );
    expect(screen.queryByTestId("agents-artifact-pane")).not.toBeInTheDocument();
  });

  it("restores a persisted artifact pane and active tab on conversation load", async () => {
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

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    const pane = await screen.findByTestId(
      "agents-artifact-pane",
      {},
      { timeout: 5_000 },
    );
    expect(pane).toHaveAttribute("data-active-tab", "publish");
    expect(screen.getByTestId("agents-artifact-resizable-pane")).toBeInTheDocument();
  });

  it("opens the Automation artifact tab by default for automation setup chats", async () => {
    mockAgentViewData(
      conversation({
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: null,
      }),
    );
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    const pane = await screen.findByTestId("agents-artifact-pane");
    await waitFor(() =>
      expect(pane).toHaveAttribute("data-active-tab", "automation"),
    );
    expect(pane).toHaveAttribute("data-automation-id", "automation-1");
  });

  it("auto-opens the Persona artifact tab for persona builder chats", async () => {
    mockAgentViewData(
      conversation({
        agentMode: "persona_builder",
        builderDraftId: null,
        builderResultPersonaId: null,
      }),
    );
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    const pane = await screen.findByTestId("agents-artifact-pane");
    await waitFor(() =>
      expect(pane).toHaveAttribute("data-active-tab", "persona"),
    );
  });

  it("forwards automation-owned conversations to the automation artifact pane action", async () => {
    const onOpenAutomation = vi.fn();
    mockAgentViewData(
      conversation({
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: null,
      })
    );
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "automation",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView({ onOpenAutomation });

    const pane = await screen.findByTestId("agents-artifact-pane");
    expect(pane).toHaveAttribute("data-active-tab", "automation");
    expect(pane).toHaveAttribute("data-automation-id", "automation-1");

    fireEvent.click(screen.getByTestId("mock-open-automation"));

    expect(onOpenAutomation).toHaveBeenCalledWith("automation-1");
  });

  it("keeps Commit & Publish selected when a Workspace Review artifact is created", async () => {
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

    renderAgentsView();

    const pane = await screen.findByTestId("agents-artifact-pane");
    expect(pane).toHaveAttribute("data-active-tab", "publish");

    act(() => {
      fireAgentViewEvent("workspace_review_artifact:created", {
        conversationId: "conversation-1",
        artifact: {
          id: "review-artifact-1",
          name: "Workspace Review",
          version: 1,
        },
      });
    });

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
        "data-active-tab",
        "publish",
      ),
    );
    expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
      "data-publish-sub-tab",
      "review",
    );
  });

  it("keeps local Workspace Review out of flat header shortcuts", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "edit" }),
    );
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({ shouldShowTab: true }),
    );
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

    renderAgentsView();

    await screen.findByTestId("agents-artifact-pane");
    fireEvent.click(screen.getByLabelText("Close panel"));
    await waitFor(() =>
      expect(
        screen.queryByTestId("agents-artifact-pane"),
      ).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("button", { name: "Review" })).not.toBeInTheDocument();
    expect(await screen.findByTestId("agents-publish-workspace")).toBeInTheDocument();
  });

  it("does not query or open Workspace Review for a PLAN workspace", async () => {
    mockAgentViewData(conversation({ agentMode: "plan" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ mode: "plan" }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "plan",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();
    await screen.findByTestId("agents-artifact-pane");
    expect(getWorkspaceReviewContextMock).not.toHaveBeenCalled();

    act(() => {
      fireAgentViewEvent("workspace_review_artifact:created", {
        conversationId: "conversation-1",
        artifact: {
          id: "stale-review-artifact",
          name: "Workspace Review",
          version: 1,
        },
      });
    });

    expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
      "data-active-tab",
      "plan",
    );
  });

  it("promotes Commit & Publish in the header when the open Review is passed and current", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        reviewGateStatus: "passed",
        isCurrent: true,
        isOutdated: false,
        shouldShowTab: true,
      }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "review",
          taskMode: "graph",
        },
      },
    });
    useAgentArtifactUiStore.setState({
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "review",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Commit & Publish",
      ),
    );
  });

  it("hydrates an initial terminal Review snapshot once", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        status: "ready",
        isCurrent: true,
        isOutdated: false,
        shouldShowTab: true,
      }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(getWorkspaceReviewContextMock).toHaveBeenCalledWith(
        "conversation-1",
        expect.objectContaining({ refreshTarget: true }),
      ),
    );
    expect(getWorkspaceReviewContextMock).toHaveBeenCalledTimes(2);
  });

  it("does not refresh an outdated Review from the UI when related runtimes become idle", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({ isOutdated: true, shouldShowTab: true }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(getWorkspaceReviewContextMock).toHaveBeenCalledWith(
        "conversation-1",
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ),
    );
    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();

    act(() => {
      fireAgentViewEvent("agent:run_completed", {
        conversationId: "task-conversation-1",
      });
    });

    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();

    act(() => {
      fireAgentViewEvent("agent:run_completed", {
        conversationId: "task-conversation-1",
      });
    });

    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();
  });

  it("does not invalidate selected workspace Review queries for unrelated child lifecycle events", async () => {
    const invalidateQueriesSpy = vi.spyOn(QueryClient.prototype, "invalidateQueries");
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({ isOutdated: true, shouldShowTab: true }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(getWorkspaceReviewContextMock).toHaveBeenCalledWith(
        "conversation-1",
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ),
    );
    invalidateQueriesSpy.mockClear();

    act(() => {
      fireAgentViewEvent("agent:run_started", {
        conversationId: "task-conversation-1",
        contextId: "task-1",
        taskId: "task-1",
      });
    });

    expect(invalidateQueriesSpy).not.toHaveBeenCalledWith({
      queryKey: agentWorkspaceKeys.workspaceReview("conversation-1"),
    });
    invalidateQueriesSpy.mockRestore();
  });

  it("does not auto-start Review from the UI when retained related runtimes are only waiting for input", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({ isOutdated: true, shouldShowTab: true }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(getWorkspaceReviewContextMock).toHaveBeenCalledWith(
        "conversation-1",
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ),
    );
    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();
  });

  it("still allows manually opening the artifact pane when the conversation has nothing to show", async () => {
    mockAgentViewData(
      conversation({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
        agentMode: "ideation",
      })
    );
    mockSessionWithData();

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(screen.queryByTestId("agents-artifact-pane")).not.toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("Open artifacts"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-pane")).toBeInTheDocument()
    );
  });

  it("opens the artifact pane before persisting the panel state", async () => {
    mockAgentViewData(
      conversation({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
        agentMode: "ideation",
      })
    );
    mockSessionWithData();
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    const setItemSpy = vi.spyOn(Storage.prototype, "setItem");
    setItemSpy.mockClear();

    fireEvent.click(screen.getByLabelText("Open artifacts"));

    expect(screen.getByTestId("agents-artifact-resizable-pane")).toBeInTheDocument();
    expect(setItemSpy).not.toHaveBeenCalled();

    setItemSpy.mockRestore();
  });

  it("closes the artifact pane before persisting the panel state", async () => {
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

    renderAgentsView();

    await screen.findByTestId("agents-artifact-pane");
    const setItemSpy = vi.spyOn(Storage.prototype, "setItem");
    setItemSpy.mockClear();

    fireEvent.click(screen.getByLabelText("Close panel"));

    expect(screen.getByTestId("agents-artifact-resizable-pane")).toHaveStyle({
      width: "0px",
      minWidth: "0px",
      maxWidth: "0px",
      opacity: "0",
      pointerEvents: "none",
    });
    expect(setItemSpy).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.queryByTestId("agents-artifact-pane")).not.toBeInTheDocument()
    );

    setItemSpy.mockRestore();
  });

  it("closes the artifact pane from the pane close action before persisting state", async () => {
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

    renderAgentsView();

    await screen.findByTestId("agents-artifact-pane");
    const setItemSpy = vi.spyOn(Storage.prototype, "setItem");
    setItemSpy.mockClear();

    fireEvent.click(screen.getByTestId("agents-artifact-pane-close"));

    expect(screen.getByTestId("agents-artifact-resizable-pane")).toHaveStyle({
      width: "0px",
      minWidth: "0px",
      maxWidth: "0px",
      opacity: "0",
      pointerEvents: "none",
    });
    expect(setItemSpy).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.queryByTestId("agents-artifact-pane")).not.toBeInTheDocument()
    );

    setItemSpy.mockRestore();
  });

  it("auto-opens the artifact pane when the conversation already has plan data", async () => {
    mockAgentViewData(
      conversation({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
      })
    );
    mockSessionWithData({ planArtifactId: "plan-1" });

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-pane")).toBeInTheDocument()
    );
  });

  it("never renders the previous conversation's focused plan during a conversation switch", async () => {
    const planConversation = conversation({
      id: "conversation-1",
      agentMode: "ideation",
      title: "Plan conversation",
    });
    const unlinkedConversation = conversation({
      id: "conversation-2",
      agentMode: "edit",
      title: "Unlinked conversation",
    });
    const conversations = [planConversation, unlinkedConversation];
    mockAgentViewData(planConversation);
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
          mode: conversationId === "conversation-1" ? "ideation" : "edit",
          linkedIdeationSessionId:
            conversationId === "conversation-1" ? "session-1" : null,
        }),
    );
    mockSessionWithData({ id: "session-1", planArtifactId: "plan-1" });
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "plan",
          taskMode: "graph",
        },
        "conversation-2": {
          isOpen: true,
          activeTab: "plan",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "conversation-1",
      ),
    );
    fireEvent.click(
      await screen.findByTestId("mock-focus-stale-proposals-session"),
    );
    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
        "data-focused-ideation-session-id",
        "session-1",
      ),
    );
    artifactPaneRenderMock.mockClear();

    const unlinkedConversationRow = await screen.findByTestId(
      "agents-session-conversation-2",
    );
    fireEvent.click(
      within(unlinkedConversationRow).getAllByRole("button")[0] ??
        unlinkedConversationRow,
    );

    expect(artifactPaneRenderMock).not.toHaveBeenCalledWith({
      conversationId: "conversation-2",
      focusedIdeationSessionId: "session-1",
    });
  });

  it("opens plan artifacts and header controls when a plan-mode session gains a plan", async () => {
    mockAgentViewData(conversation({ agentMode: "plan" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      })
    );
    mockSessionWithData({ id: "session-1", planArtifactId: null });
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    const { queryClient } = renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(screen.queryByTestId("agents-artifact-pane")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Open artifacts")).toBeInTheDocument();
    expect(screen.getByLabelText("Plan")).toBeInTheDocument();

    mockSessionWithData({ id: "session-1", planArtifactId: "plan-1" });
    await act(async () => {
      await queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionWithData("session-1"),
      });
    });

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
        "data-active-tab",
        "plan"
      )
    );
    expect(screen.getByLabelText("Close panel")).toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("Close panel"));

    await waitFor(() =>
      expect(screen.getByLabelText("Open artifacts")).toBeInTheDocument()
    );
    expect(screen.getByLabelText("Plan")).toBeInTheDocument();
    expect(screen.queryByLabelText("Proposals")).not.toBeInTheDocument();
  });

  it("selects the Plan tab when the selected plan-mode conversation creates a plan", async () => {
    mockAgentViewData(conversation({ agentMode: "plan" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      })
    );
    mockSessionWithData({
      id: "session-1",
      planArtifactId: null,
      sessionFlow: "planning",
    });
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "jira",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();

    const pane = await screen.findByTestId("agents-artifact-pane");
    expect(pane).toHaveAttribute("data-active-tab", "jira");

    act(() => {
      fireAgentViewEvent("plan_artifact:created", {
        sessionId: "session-1",
        artifact: {
          id: "plan-1",
          name: "Plan",
          content: "# Plan",
          version: 1,
        },
      });
    });

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
        "data-active-tab",
        "plan"
      )
    );
  });

  it("keeps the current tab when a created plan belongs to another conversation", async () => {
    mockAgentViewData(conversation({ agentMode: "plan" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      })
    );
    mockSessionWithData({
      id: "session-1",
      planArtifactId: null,
      sessionFlow: "planning",
    });
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "jira",
          taskMode: "graph",
        },
      },
    });

    renderAgentsView();

    const pane = await screen.findByTestId("agents-artifact-pane");
    expect(pane).toHaveAttribute("data-active-tab", "jira");

    act(() => {
      fireAgentViewEvent("plan_artifact:created", {
        sessionId: "other-session",
        artifact: {
          id: "plan-other",
          name: "Other Plan",
          content: "# Other Plan",
          version: 1,
        },
      });
    });

    expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
      "data-active-tab",
      "jira"
    );
  });

  it("keeps Proposals out of header shortcuts after a plan-mode session has proposals", async () => {
    mockAgentViewData(conversation({ agentMode: "plan" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      })
    );
    mockSessionWithData(
      { id: "session-1", planArtifactId: "plan-1" },
      [
        {
          id: "proposal-1",
          sessionId: "session-1",
          title: "Gate embedded proposal access",
          description: "Keep proposals inside the Plan tab.",
          category: "frontend",
          steps: ["Update artifact tabs"],
          acceptanceCriteria: ["Proposals remain available inside Plan"],
          suggestedPriority: "high",
          priorityScore: 90,
          priorityReason: "Avoids empty navigation",
          estimatedComplexity: "simple",
          userPriority: null,
          userModified: false,
          status: "pending",
          createdTaskId: null,
          planArtifactId: "plan-1",
          planVersionAtCreation: 1,
          sortOrder: 0,
          createdAt: "2026-04-23T09:15:00Z",
          updatedAt: "2026-04-23T09:15:00Z",
        },
      ],
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
        "data-active-tab",
        "plan"
      )
    );
    fireEvent.click(screen.getByLabelText("Close panel"));

    await waitFor(() =>
      expect(screen.getByLabelText("Open artifacts")).toBeInTheDocument()
    );
    expect(screen.getByLabelText("Plan")).toBeInTheDocument();
    expect(screen.queryByLabelText("Proposals")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-proposal"),
    ).not.toBeInTheDocument();
  });

});
