import {
  fireAgentViewEvent,
  getAgentsViewTestMocks,
  mockAgentViewData,
  mockSessionWithData,
  renderAgentsView,
  resetAgentSessionState,
  selectSidebarConversationRow,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ideationKeys } from "@/hooks/useIdeation";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";

const { getAgentConversationWorkspaceMock } = getAgentsViewTestMocks();

describe("AgentsView artifact pane", () => {
  beforeEach(setupAgentsViewTest);

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
    const pane = await screen.findByTestId("agents-artifact-pane");
    expect(pane).toHaveAttribute("data-active-tab", "publish");
    expect(screen.getByTestId("agents-artifact-resizable-pane")).toBeInTheDocument();
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

  it("shows the Proposals shortcut after a plan-mode session has proposals", async () => {
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
          title: "Gate proposal tab visibility",
          description: "Show Proposals only when proposal content exists.",
          category: "frontend",
          steps: ["Update artifact tabs"],
          acceptanceCriteria: ["Proposals shortcut appears with content"],
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
    expect(screen.getByLabelText("Proposals")).toBeInTheDocument();
  });

});
