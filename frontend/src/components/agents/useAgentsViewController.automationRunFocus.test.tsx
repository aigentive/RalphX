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

const { requestAutomationRunOpenMock } = vi.hoisted(() => ({
  requestAutomationRunOpenMock: vi.fn(),
}));

vi.mock("@/components/automations/automationRunNavigation", () => ({
  requestAutomationRunOpen: (...args: unknown[]) =>
    requestAutomationRunOpenMock(...args),
}));

const {
  getAgentConversationWorkspaceMock,
  getWorkspaceReviewContextMock,
  integratedChatPanelRenderMock,
  useConversationMock,
  useProjectAgentConversationsMock,
} = getAgentsViewTestMocks();
const deferredHydrationTimeout = { timeout: 3_000 };
const originalClearAutomationRunFocusRequest =
  useAgentSessionStore.getState().clearAutomationRunFocusRequest;

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

function automationRunConversation(
  overrides: Partial<AgentConversation> = {},
): AgentConversation {
  return conversation({
    id: "run-conversation-1",
    title: "Release automation run",
    agentMode: "automation",
    automationId: "automation-1",
    automationRunId: "run-1",
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

function renderControllerView() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  const makeUi = () => (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <AgentsView projectId="project-1" onCreateProject={vi.fn()} />
      </TooltipProvider>
    </QueryClientProvider>
  );
  const result = render(makeUi());
  return {
    ...result,
    rerenderController: () => result.rerender(makeUi()),
  };
}

function mockHydratedSetupConversation(setup = automationSetupConversation()) {
  mockAgentViewData(setup);
  getAgentConversationWorkspaceMock.mockResolvedValue(
    conversationWorkspace({
      conversationId: setup.id,
      mode: "automation",
    }),
  );
}

function mockDynamicSelectedConversation(
  conversationRef: { current: AgentConversation | null },
) {
  const seed = conversationRef.current ?? automationSetupConversation();
  mockAgentViewData(seed);
  useProjectAgentConversationsMock.mockImplementation(() => ({
    data: conversationRef.current ? [conversationRef.current] : [],
    conversations: conversationRef.current ? [conversationRef.current] : [],
    isLoading: conversationRef.current === null,
    isSuccess: conversationRef.current !== null,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
  }));
  useConversationMock.mockImplementation((conversationId: string | null) => ({
    data:
      conversationRef.current && conversationId === conversationRef.current.id
        ? {
            conversation: conversationRef.current,
            messages: [],
          }
        : null,
    isLoading: conversationRef.current === null,
  }));
}

async function expectRunFocusApplied() {
  await waitFor(() => {
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-conversation-id-override",
      "run-conversation-1",
    );
  });
  expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
    "data-store-context-key-override",
    "project:run-conversation-1",
  );
  expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
    "data-send-conversation-id",
    "run-conversation-1",
  );
}

describe("useAgentsViewController automation run focus", () => {
  beforeEach(() => {
    setupAgentsViewTest();
    requestAutomationRunOpenMock.mockReset();
    useAgentSessionStore.setState({
      clearAutomationRunFocusRequest: originalClearAutomationRunFocusRequest,
    });
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
  });

  afterEach(() => {
    useAgentSessionStore.setState({
      clearAutomationRunFocusRequest: originalClearAutomationRunFocusRequest,
    });
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
  });

  it("auto-focuses a durable workspace fixer child instead of resetting to workspace", async () => {
    const setup = automationSetupConversation({ agentMode: "edit" });
    mockHydratedSetupConversation(setup);
    getWorkspaceReviewContextMock.mockResolvedValue({
      success: true,
      workspace: conversationWorkspace({ conversationId: setup.id, mode: "edit" }),
      events: [],
      target: null,
      monitor: {
        conversationId: setup.id,
        status: "idle",
        reviewConversationId: null,
        reviewFixerConversationId: "fixer-child-1",
      },
      repairRuntimeConversationId: null,
      repairFixerKind: null,
      isCurrent: false,
      isOutdated: false,
      shouldShowTab: false,
    });
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: setup.id,
    });

    renderControllerView();

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        "fixer-child-1",
      );
    });
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-send-conversation-id",
      "fixer-child-1",
    );
  });

  it("holds an automation-run request until the setup conversation hydrates, consumes it once, and clears it", async () => {
    const hydratedSetup = automationSetupConversation();
    const conversationRef: { current: AgentConversation | null } = { current: null };
    mockDynamicSelectedConversation(conversationRef);
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ conversationId: hydratedSetup.id, mode: "automation" }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: hydratedSetup.id,
      automationRunFocusRequestByConversationId: {
        [hydratedSetup.id]: focusRequest(),
      },
    });
    const originalClear =
      useAgentSessionStore.getState().clearAutomationRunFocusRequest;
    const clearSpy = vi.fn(originalClear);
    useAgentSessionStore.setState({
      clearAutomationRunFocusRequest: clearSpy,
    });

    const view = renderControllerView();

    expect(
      useAgentSessionStore.getState().automationRunFocusRequestByConversationId[
        hydratedSetup.id
      ],
    ).toBeDefined();
    expect(
      integratedChatPanelRenderMock.mock.calls.some(
        ([call]) =>
          (call as { conversationIdOverride?: string }).conversationIdOverride ===
          "run-conversation-1",
      ),
    ).toBe(false);

    conversationRef.current = hydratedSetup;
    view.rerenderController();

    await expectRunFocusApplied();
    await waitFor(() => {
      expect(useAgentSessionStore.getState().visibleAgentScope).toEqual({
        workspaceConversationId: hydratedSetup.id,
        visibleConversationId: "run-conversation-1",
        automationRunId: "run-1",
        automationConversationId: "run-conversation-1",
      });
    });
    expect(clearSpy).toHaveBeenCalledTimes(1);
    expect(
      useAgentSessionStore.getState().automationRunFocusRequestByConversationId[
        hydratedSetup.id
      ],
    ).toBeUndefined();

    view.rerenderController();
    expect(clearSpy).toHaveBeenCalledTimes(1);
    view.unmount();
    expect(useAgentSessionStore.getState().visibleAgentScope).toBeNull();
  });

  it("does not re-apply a consumed request after manually switching back to Workspace", async () => {
    const setup = automationSetupConversation();
    mockHydratedSetupConversation(setup);
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: setup.id,
      automationRunFocusRequestByConversationId: {
        [setup.id]: focusRequest(),
      },
    });

    renderControllerView();
    await expectRunFocusApplied();

    fireEvent.click(screen.getByTestId("agents-composer-chat-focus-pill"));
    fireEvent.click(
      screen.getByTestId("agents-composer-chat-focus-option-workspace"),
    );

    await waitFor(() => {
      expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
        "data-conversation-id-override",
        setup.id,
      );
    });
    expect(
      useAgentSessionStore.getState().automationRunFocusRequestByConversationId[
        setup.id
      ],
    ).toBeUndefined();
  });

  it("respects click-site artifact tab seeds and derives the policy default when no seed is present", async () => {
    const setup = automationSetupConversation();
    mockHydratedSetupConversation(setup);
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: setup.id,
      artifactByConversationId: {
        [setup.id]: {
          isOpen: true,
          activeTab: "tasks",
          taskMode: "graph",
        },
      },
      automationRunFocusRequestByConversationId: {
        [setup.id]: focusRequest({ seededTab: "plan" }),
      },
    });
    useAgentArtifactUiStore.getState().setArtifactState(setup.id, {
      isOpen: true,
      activeTab: "tasks",
      taskMode: "graph",
    });

    const first = renderControllerView();
    await expectRunFocusApplied();
    expect(
      await screen.findByTestId(
        "agents-artifact-pane",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveAttribute("data-active-tab", "tasks");
    first.unmount();

    const requestWithoutSeed = {
      ...focusRequest({ requestId: 2, seededTab: "automation" }),
    };
    delete (requestWithoutSeed as Partial<AgentAutomationRunFocusRequest>).seededTab;
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
    mockHydratedSetupConversation(setup);
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: setup.id,
      automationRunFocusRequestByConversationId: {
        [setup.id]:
          requestWithoutSeed as unknown as AgentAutomationRunFocusRequest,
      },
    });

    renderControllerView();
    await expectRunFocusApplied();
    expect(
      await screen.findByTestId(
        "agents-artifact-pane",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveAttribute("data-active-tab", "pr");
  });

  it("reveals a hidden tab targeted by an explicit automation-run focus request", async () => {
    const setup = automationSetupConversation();
    mockHydratedSetupConversation(setup);
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: setup.id,
      artifactByConversationId: {
        [setup.id]: {
          isOpen: true,
          activeTab: "tasks",
          taskMode: "graph",
          hiddenTabs: ["plan"],
        },
      },
      automationRunFocusRequestByConversationId: {
        [setup.id]: focusRequest({ seededTab: "plan" }),
      },
    });
    useAgentArtifactUiStore.getState().setArtifactState(setup.id, {
      isOpen: true,
      activeTab: "tasks",
      taskMode: "graph",
      hiddenTabs: ["plan"],
    });

    renderControllerView();
    await expectRunFocusApplied();

    expect(
      await screen.findByTestId(
        "agents-artifact-pane",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveAttribute("data-active-tab", "plan");
    await waitFor(() => {
      expect(
        useAgentSessionStore.getState().artifactByConversationId[setup.id]
          ?.hiddenTabs,
      ).toEqual([]);
    });
  });

  it("does not unhide Automation during level-triggered setup seeding", async () => {
    const setup = automationSetupConversation();
    mockHydratedSetupConversation(setup);
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: setup.id,
      artifactByConversationId: {
        [setup.id]: {
          isOpen: true,
          activeTab: "plan",
          taskMode: "graph",
          hiddenTabs: ["automation"],
        },
      },
    });
    useAgentArtifactUiStore.getState().setArtifactState(setup.id, {
      isOpen: true,
      activeTab: "plan",
      taskMode: "graph",
      hiddenTabs: ["automation"],
    });

    renderControllerView();

    expect(
      await screen.findByTestId(
        "agents-artifact-pane",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveAttribute("data-active-tab", "plan");
    await waitFor(() => {
      expect(
        useAgentSessionStore.getState().artifactByConversationId[setup.id]
          ?.hiddenTabs,
      ).toEqual(["automation"]);
    });
  });

  it("keeps a newer automation-run request when an older request id is cleared", () => {
    const setup = automationSetupConversation();
    resetAgentSessionState({
      automationRunFocusRequestByConversationId: {
        [setup.id]: focusRequest({ requestId: 2, runId: "run-2" }),
      },
    });

    useAgentSessionStore
      .getState()
      .clearAutomationRunFocusRequest(setup.id, 1);

    expect(
      useAgentSessionStore.getState().automationRunFocusRequestByConversationId[
        setup.id
      ],
    ).toMatchObject({ requestId: 2, runId: "run-2" });
  });

  it("normalizes a directly selected run conversation to setup selection plus run focus without looping", async () => {
    const setup = automationSetupConversation();
    const runConversation = automationRunConversation();
    mockAgentViewData(setup);
    useProjectAgentConversationsMock.mockReturnValue({
      data: [setup, runConversation],
      conversations: [setup, runConversation],
      isLoading: false,
      isSuccess: true,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useConversationMock.mockImplementation((conversationId: string | null) => {
      const match = [setup, runConversation].find(
        (candidate) => candidate.id === conversationId,
      );
      return {
        data: match ? { conversation: match, messages: [] } : null,
        isLoading: false,
      };
    });
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({ conversationId: setup.id, mode: "automation" }),
    );
    requestAutomationRunOpenMock.mockImplementation(
      async (
        _queryClient: unknown,
        target: {
          projectId: string;
          automationId: string;
          runId: string;
          conversationId: string;
        },
      ) => {
        useAgentSessionStore
          .getState()
          .selectConversation(target.projectId, setup.id);
        useAgentSessionStore.getState().requestAutomationRunFocus(setup.id, {
          projectId: target.projectId,
          automationId: target.automationId,
          runId: target.runId,
          conversationId: target.conversationId,
          runStatus: "published",
          judgeState: "none",
          workspaceMode: null,
          hasPlanArtifact: true,
          hasPullRequest: true,
          seededTab: "pr",
        });
        return { applied: true };
      },
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: runConversation.id,
    });

    renderControllerView();

    await expectRunFocusApplied();
    expect(requestAutomationRunOpenMock).toHaveBeenCalledTimes(1);
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(setup.id);
    await waitFor(() => expect(requestAutomationRunOpenMock).toHaveBeenCalledTimes(1));
  });
});
