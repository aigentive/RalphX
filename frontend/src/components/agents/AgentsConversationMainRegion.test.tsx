import { render, screen } from "@testing-library/react";
import type { ComponentProps, ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AgentsConversationMainRegion } from "./AgentsConversationMainRegion";
import {
  agentProjectFixture,
  agentRuntimeFixture,
  conversationFixture,
  conversationWorkspaceFixture,
} from "./agentsTestFixtures";

const { activePanelMock, startPanelMock } = vi.hoisted(() => ({
  activePanelMock: vi.fn(() => <div data-testid="active-panel" />),
  startPanelMock: vi.fn(() => <div data-testid="start-panel" />),
}));

vi.mock("./AgentsActiveConversationPanel", () => ({
  AgentsActiveConversationPanel: (props: Record<string, unknown>) =>
    activePanelMock(props),
}));

vi.mock("./AgentsStartConversationPanel", () => ({
  AgentsStartConversationPanel: (props: Record<string, unknown>) =>
    startPanelMock(props),
}));

function mainRegionProps(
  overrides: Partial<ComponentProps<typeof AgentsConversationMainRegion>> = {},
): ComponentProps<typeof AgentsConversationMainRegion> {
  return {
    activeConversation: conversationFixture(),
    activeConversationMode: "ideation",
    activeConversationModeLocked: false,
    activeProjectId: "project-1",
    activeProjectOptions: [{ id: "project-1", label: "RalphX" }],
    activeWorkspace: conversationWorkspaceFixture({ mode: "ideation" }),
    activeWorkspaceFreshness: undefined,
    attachedIdeationSessionId: null,
    availableArtifactTabs: [],
    chatFocus: { type: "workspace" },
    chatFocusOptions: [],
    defaultProjectId: "project-1",
    defaultRuntime: agentRuntimeFixture,
    hasAttachedPlanArtifact: false,
    hasAutoOpenArtifacts: false,
    focusedWorkspaceReviewServiceTier: null,
    isLoadingProjects: false,
    modelRegistry: null,
    normalizedActiveRuntime: agentRuntimeFixture,
    onActiveConversationModeChange: vi.fn(),
    onActiveConversationModeMenuOpen: vi.fn(),
    onActiveTeamEnabledChange: vi.fn(),
    onActiveEffortChange: vi.fn(),
    onActiveModelChange: vi.fn(),
    onActiveProviderChange: vi.fn(),
    onAgentUserMessageSent: vi.fn(),
    onConversationModeSwitched: vi.fn(),
    onFocusIdeationSession: vi.fn(),
    onFocusIdeationSessionForConversation: vi.fn(),
    onFocusWorkspaceReview: vi.fn(),
    onFocusVerificationSession: vi.fn(),
    onFocusTaskRuntime: vi.fn(),
    onFocusAutomationRun: vi.fn(),
    onOpenTaskArtifact: vi.fn(),
    onForkConversation: vi.fn(),
    onOpenPlanArtifact: vi.fn(),
    onOpenPublishPane: vi.fn(),
    onOpenPublishFile: vi.fn(),
    onPreloadArtifacts: vi.fn(),
    onPublishWorkspace: vi.fn(),
    onRenameConversation: vi.fn(),
    onRuntimePreferenceChange: vi.fn(),
    onSelectArtifact: vi.fn(),
    onStartAgentConversation: vi.fn(),
    onStartPersonaBuilder: vi.fn(),
    onToggleArtifacts: vi.fn(),
    onSelectChatFocus: vi.fn(),
    projects: [agentProjectFixture],
    publishShortcutLabel: "P",
    publishAttemptsByConversationId: {},
    selectedConversationId: "conversation-1",
    selectedTaskArtifactId: null,
    setTerminalChatDockElement: vi.fn((_: ReactNode) => undefined),
    switchingConversationModeId: null,
    terminalArchivedReason: null,
    terminalUnavailableReason: null,
    ...overrides,
  };
}

describe("AgentsConversationMainRegion", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the active conversation panel for a selected workspace conversation", () => {
    render(<AgentsConversationMainRegion {...mainRegionProps()} />);

    expect(screen.getByTestId("active-panel")).toBeInTheDocument();
    expect(activePanelMock).toHaveBeenCalledWith(
      expect.objectContaining({
        selectedConversationId: "conversation-1",
      }),
    );
  });

  it("renders a selected standalone conversation without an active project", () => {
    render(
      <AgentsConversationMainRegion
        {...mainRegionProps({
          activeConversation: conversationFixture({
            id: "standalone-1",
            contextType: "standalone",
            contextId: "standalone-1",
            projectId: null,
          }),
          activeProjectId: null,
          activeWorkspace: null,
          selectedConversationId: "standalone-1",
        })}
      />,
    );

    expect(screen.getByTestId("active-panel")).toBeInTheDocument();
    expect(screen.queryByTestId("start-panel")).not.toBeInTheDocument();
  });
});
