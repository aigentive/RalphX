import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import {
  useAgentSessionStore,
  type AgentAutomationRunFocusRequest,
} from "@/stores/agentSessionStore";
import {
  getAgentsViewTestMocks,
  mockAgentViewData,
  resetAgentSessionState,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { AgentsView } from "./AgentsView";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";
import type { AgentConversation } from "./agentConversations";

const { getAgentConversationWorkspaceMock } = getAgentsViewTestMocks();

function automationSetupConversation(
  overrides: Partial<AgentConversation> = {},
): AgentConversation {
  return conversation({
    id: "setup-conversation-1",
    title: "Release automation setup",
    agentMode: "automation",
    automationId: "automation-1",
    automationRunId: null,
    ...overrides,
  });
}

function focusRequest(
  overrides: Partial<AgentAutomationRunFocusRequest> = {},
): AgentAutomationRunFocusRequest {
  return {
    projectId: "project-1",
    automationId: "automation-1",
    runId: "run-1",
    conversationId: "run-conversation-1",
    runStatus: "published",
    judgeState: "none",
    workspaceMode: null,
    hasPlanArtifact: true,
    hasPullRequest: true,
    seededTab: "pr",
    requestId: 1,
    ...overrides,
  };
}

function mockWorkspacesByConversation() {
  getAgentConversationWorkspaceMock.mockImplementation(
    (conversationId: string) =>
      Promise.resolve(
        conversationId === "run-conversation-1"
          ? conversationWorkspace({
              conversationId: "run-conversation-1",
              mode: "edit",
              branchName: "ralphx/run-branch",
            })
          : conversationWorkspace({
              conversationId: "setup-conversation-1",
              mode: "automation",
            }),
      ),
  );
}

function renderControllerView() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <AgentsView projectId="project-1" onCreateProject={vi.fn()} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

function setupRunFocusedView({
  paneOpen = true,
}: { paneOpen?: boolean } = {}) {
  const setup = automationSetupConversation();
  mockAgentViewData(setup);
  mockWorkspacesByConversation();
  resetAgentSessionState({
    selectedProjectId: "project-1",
    selectedConversationId: setup.id,
    ...(paneOpen
      ? {
          artifactByConversationId: {
            [setup.id]: {
              isOpen: true,
              activeTab: "pr",
              taskMode: "graph",
            },
          },
        }
      : {}),
    automationRunFocusRequestByConversationId: {
      [setup.id]: focusRequest(),
    },
  });
  if (paneOpen) {
    useAgentArtifactUiStore.getState().setArtifactState(setup.id, {
      isOpen: true,
      activeTab: "pr",
      taskMode: "graph",
    });
  }
  return setup;
}

async function expectRunFocusApplied() {
  await waitFor(() => {
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-conversation-id-override",
      "run-conversation-1",
    );
  });
}

function expectRunFocusStillApplied() {
  expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
    "data-conversation-id-override",
    "run-conversation-1",
  );
}

describe("useAgentsViewController automation run publish focus", () => {
  beforeEach(() => {
    setupAgentsViewTest();
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
  });

  afterEach(() => {
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
  });

  it("keeps the automation-run chat focus when selecting the publish tab", async () => {
    setupRunFocusedView();
    renderControllerView();
    await expectRunFocusApplied();

    fireEvent.click(
      await screen.findByTestId("mock-select-artifact-tab-publish"),
    );

    await waitFor(() => {
      expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
        "data-active-tab",
        "publish",
      );
    });
    expectRunFocusStillApplied();
    expect(useAgentSessionStore.getState().visibleAgentScope).toMatchObject({
      visibleConversationId: "run-conversation-1",
      automationRunId: "run-1",
      automationConversationId: "run-conversation-1",
    });
  });

  it("keeps the automation-run chat focus when opening the publish pane action", async () => {
    setupRunFocusedView();
    renderControllerView();
    await expectRunFocusApplied();

    fireEvent.click(await screen.findByTestId("mock-open-publish-pane"));

    await waitFor(() => {
      expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
        "data-active-tab",
        "publish",
      );
    });
    expectRunFocusStillApplied();
  });

  it("still returns to the workspace chat when selecting a non-review tab without a run focus", async () => {
    const setup = automationSetupConversation();
    mockAgentViewData(setup);
    mockWorkspacesByConversation();
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: setup.id,
      artifactByConversationId: {
        [setup.id]: {
          isOpen: true,
          activeTab: "pr",
          taskMode: "graph",
        },
      },
    });
    useAgentArtifactUiStore.getState().setArtifactState(setup.id, {
      isOpen: true,
      activeTab: "pr",
      taskMode: "graph",
    });
    renderControllerView();

    fireEvent.click(
      await screen.findByTestId("mock-select-artifact-tab-publish"),
    );

    await waitFor(() => {
      expect(screen.getByTestId("agents-artifact-pane")).toHaveAttribute(
        "data-active-tab",
        "publish",
      );
    });
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-conversation-id-override",
      setup.id,
    );
  });

  it("shows the run workspace's publish shortcut label while a run is focused", async () => {
    setupRunFocusedView({ paneOpen: false });
    const { getAgentConversationWorkspaceFreshnessMock } =
      getAgentsViewTestMocks();
    getAgentConversationWorkspaceFreshnessMock.mockImplementation(
      (conversationId: string) =>
        Promise.resolve(
          conversationId === "run-conversation-1"
            ? {
                conversationId: "run-conversation-1",
                baseRef: "release-base",
                effectiveBaseRef: "release-base",
                isBaseAhead: true,
                hasUncommittedChanges: false,
                unpublishedCommitCount: 0,
                baseStatus: "ok",
                freshnessScope: "full",
                remoteRefreshed: true,
                worktreeStatusChecked: true,
              }
            : null,
        ),
    );
    renderControllerView();
    await expectRunFocusApplied();

    // The run focus auto-opens the artifact pane; close it so the header
    // publish shortcut becomes visible.
    fireEvent.click(await screen.findByTestId("agents-artifact-pane-close"));

    await waitFor(() => {
      expect(
        screen.getByTestId("agents-publish-workspace"),
      ).toHaveAccessibleName(
        "Open workspace publish panel: Update from release-base",
      );
    });
  });

  it("suppresses the publish shortcut when the focused run workspace is unresolved", async () => {
    const setup = automationSetupConversation();
    mockAgentViewData(setup);
    getAgentConversationWorkspaceMock.mockImplementation(
      (conversationId: string) =>
        Promise.resolve(
          conversationId === "run-conversation-1"
            ? null
            : conversationWorkspace({
                conversationId: "setup-conversation-1",
                mode: "automation",
              }),
        ),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: setup.id,
      automationRunFocusRequestByConversationId: {
        [setup.id]: focusRequest(),
      },
    });
    renderControllerView();
    await expectRunFocusApplied();

    await waitFor(() => {
      expect(
        screen.queryByTestId("agents-publish-workspace"),
      ).not.toBeInTheDocument();
    });
  });
});
