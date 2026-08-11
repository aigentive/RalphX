import type { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AutomationDetail } from "@/api/automations";
import {
  requestAutomationRunOpen,
  resetAutomationRunOpenRequestStateForTests,
} from "./automationRunNavigation";

const {
  activeProjectIdRef,
  artifactByConversationId,
  automationGetMock,
  clearSelectionMock,
  projectSelectMock,
  requestAutomationRunFocusMock,
  selectConversationMock,
  seedAgentArtifactTabMock,
  setActiveConversationMock,
  setArtifactTabMock,
  setCurrentViewMock,
  setFocusedProjectMock,
  toastErrorMock,
} = vi.hoisted(() => ({
  activeProjectIdRef: { current: "project-1" as string | null },
  artifactByConversationId: {} as Record<string, unknown>,
  automationGetMock: vi.fn(),
  clearSelectionMock: vi.fn(),
  projectSelectMock: vi.fn(),
  requestAutomationRunFocusMock: vi.fn(),
  selectConversationMock: vi.fn(),
  seedAgentArtifactTabMock: vi.fn(),
  setActiveConversationMock: vi.fn(),
  setArtifactTabMock: vi.fn(),
  setCurrentViewMock: vi.fn(),
  setFocusedProjectMock: vi.fn(),
  toastErrorMock: vi.fn(),
}));

vi.mock("@/components/agents/agentArtifactState", () => ({
  seedAgentArtifactTab: seedAgentArtifactTabMock,
}));

vi.mock("@/api/automations", () => ({
  automationsApi: {
    get: automationGetMock,
  },
}));

vi.mock("@/stores/agentSessionStore", () => ({
  useAgentSessionStore: {
    getState: () => ({
      artifactByConversationId,
      clearSelection: clearSelectionMock,
      requestAutomationRunFocus: requestAutomationRunFocusMock,
      selectConversation: selectConversationMock,
      setArtifactTab: setArtifactTabMock,
      setFocusedProject: setFocusedProjectMock,
    }),
  },
}));

vi.mock("@/stores/chatStore", () => ({
  useChatStore: {
    getState: () => ({
      setActiveConversation: setActiveConversationMock,
    }),
  },
}));

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: {
    getState: () => ({
      activeProjectId: activeProjectIdRef.current,
      selectProject: projectSelectMock,
    }),
  },
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: {
    getState: () => ({
      setCurrentView: setCurrentViewMock,
    }),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastErrorMock,
  },
}));

function queryClient(): QueryClient {
  return {
    ensureQueryData: vi.fn(({ queryFn }: { queryFn: () => Promise<AutomationDetail> }) =>
      queryFn(),
    ),
  } as unknown as QueryClient;
}

function automationDetail(
  overrides: Partial<AutomationDetail> = {},
): AutomationDetail {
  return {
    automation: {
      id: "automation-1",
      projectId: "project-1",
      name: "Automation",
      status: "active",
      pausedReasonCode: null,
      pausedReasonDetail: null,
      goalPrompt: "Ship it",
      setupConversationId: "setup-conversation-1",
      specArtifactId: null,
      providerHarness: "claude",
      modelId: "sonnet",
      logicalEffort: null,
      runMode: "edit",
      baseRefKind: "project_default",
      baseRef: "",
      baseDisplayName: null,
      baseSourcePullRequestJson: null,
      goalItemsJson: null,
      chainMode: "merged_base",
      completionSignal: "pr_merged",
      planApprovalMode: "manual",
      prMergeMode: "manual",
      planDeepVerification: false,
      maxRuns: 25,
      maxConsecutiveFailures: 3,
      firstRunPrompt: "Start",
      setupAnalysisSummary: null,
      createdAt: "2026-07-09T10:00:00Z",
      updatedAt: "2026-07-09T10:00:00Z",
    },
    runs: [
      {
        id: "run-1",
        automationId: "automation-1",
        runIndex: 1,
        status: "awaiting_plan_approval",
        judgeState: "none",
        judgeLeaseExpiresAt: null,
        planJudgeState: "none",
        planRevisionRound: 0,
        planRevisionPending: false,
        planPhase: false,
        planArtifactId: "plan-artifact-1",
        planApprovedBy: null,
        planApprovedArtifactVersion: null,
        planApprovedAt: null,
        conversationId: "run-conversation-1",
        runPrompt: "Run",
        promptAuthor: "setup_agent",
        baseRefKind: "project_default",
        baseRefUsed: "",
        baseFromRunId: null,
        goalItemId: null,
        branchName: null,
        prNumber: null,
        prUrl: null,
        prTitle: null,
        prHeadRefName: null,
        prBaseRefName: null,
        prMergedAt: null,
        mergeCommitSha: null,
        diffStatsJson: null,
        agentSummary: null,
        judgeVerdictJson: null,
        judgeModelId: null,
        errorCode: null,
        errorDetail: null,
        signalCheckFailures: 0,
        startedAt: null,
        finishedAt: null,
        createdAt: "2026-07-09T10:00:00Z",
        updatedAt: "2026-07-09T10:00:00Z",
      },
    ],
    usage: {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      estimatedUsd: null,
    },
    ...overrides,
  };
}

describe("requestAutomationRunOpen", () => {
  beforeEach(() => {
    activeProjectIdRef.current = "project-1";
    for (const key of Object.keys(artifactByConversationId)) {
      delete artifactByConversationId[key];
    }
    vi.clearAllMocks();
    resetAutomationRunOpenRequestStateForTests();
  });

  it("opens a known setup conversation and seeds the parked run Plan tab", async () => {
    await requestAutomationRunOpen(queryClient(), {
      projectId: "project-1",
      automationId: "automation-1",
      runId: "run-1",
      conversationId: "run-conversation-1",
      setupConversationId: "setup-conversation-1",
      runStatus: "awaiting_plan_approval",
      judgeState: "none",
      planPhase: false,
      planArtifactId: "plan-artifact-1",
      prNumber: null,
    });

    expect(setFocusedProjectMock).toHaveBeenCalledWith("project-1");
    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
    expect(automationGetMock).not.toHaveBeenCalled();
    expect(selectConversationMock).toHaveBeenCalledWith(
      "project-1",
      "setup-conversation-1",
    );
    expect(setActiveConversationMock).toHaveBeenCalledWith(
      "project:project-1",
      "setup-conversation-1",
    );
    expect(seedAgentArtifactTabMock).toHaveBeenCalledWith(
      "setup-conversation-1",
      "plan",
      false,
    );
    expect(requestAutomationRunFocusMock).toHaveBeenCalledWith(
      "setup-conversation-1",
      expect.objectContaining({
        automationId: "automation-1",
        runId: "run-1",
        conversationId: "run-conversation-1",
        runStatus: "awaiting_plan_approval",
        hasPlanArtifact: true,
        hasPullRequest: false,
        seededTab: "plan",
      }),
    );
  });

  it("lets a parked run seed Plan over a stale persisted Automation tab", async () => {
    artifactByConversationId["setup-conversation-1"] = {
      isOpen: true,
      activeTab: "automation",
      taskMode: "graph",
    };

    await requestAutomationRunOpen(queryClient(), {
      projectId: "project-1",
      automationId: "automation-1",
      runId: "run-1",
      conversationId: "run-conversation-1",
      setupConversationId: "setup-conversation-1",
      runStatus: "awaiting_plan_approval",
      judgeState: "none",
      planPhase: false,
      planArtifactId: "plan-artifact-1",
      prNumber: null,
    });

    expect(seedAgentArtifactTabMock).toHaveBeenCalledWith(
      "setup-conversation-1",
      "plan",
      false,
    );
    expect(requestAutomationRunFocusMock).toHaveBeenCalledWith(
      "setup-conversation-1",
      expect.objectContaining({ seededTab: "plan" }),
    );
    expect(setArtifactTabMock).not.toHaveBeenCalled();
  });

  it("honors an explicit notification tab intent over the fast path's synthesized run state", async () => {
    await requestAutomationRunOpen(
      queryClient(),
      {
        projectId: "project-1",
        automationId: "automation-1",
        runId: "run-1",
        conversationId: "run-conversation-1",
        setupConversationId: "setup-conversation-1",
      },
      { tabHint: "plan" },
    );

    expect(automationGetMock).not.toHaveBeenCalled();
    expect(requestAutomationRunFocusMock).toHaveBeenCalledWith(
      "setup-conversation-1",
      expect.objectContaining({ seededTab: "plan" }),
    );
  });

  it("keeps the automation tab as the default when no notification intent is supplied", async () => {
    await requestAutomationRunOpen(queryClient(), {
      projectId: "project-1",
      automationId: "automation-1",
      runId: "run-1",
      conversationId: "run-conversation-1",
      setupConversationId: "setup-conversation-1",
    });

    expect(requestAutomationRunFocusMock).toHaveBeenCalledWith(
      "setup-conversation-1",
      expect.objectContaining({ seededTab: "automation" }),
    );
  });

  it("paints the Agents shell before resolving automation detail for popover targets", async () => {
    let resolveDetail: (detail: AutomationDetail) => void = () => {};
    automationGetMock.mockReturnValue(
      new Promise<AutomationDetail>((resolve) => {
        resolveDetail = resolve;
      }),
    );

    const pending = requestAutomationRunOpen(queryClient(), {
      projectId: "project-1",
      automationId: "automation-1",
      runId: "run-1",
      conversationId: "run-conversation-1",
    });

    expect(setFocusedProjectMock).toHaveBeenCalledWith("project-1");
    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
    expect(automationGetMock).toHaveBeenCalledWith("automation-1");
    expect(selectConversationMock).not.toHaveBeenCalled();

    resolveDetail(automationDetail());
    await pending;

    expect(selectConversationMock).toHaveBeenCalledWith(
      "project-1",
      "setup-conversation-1",
    );
    expect(requestAutomationRunFocusMock).toHaveBeenCalledWith(
      "setup-conversation-1",
      expect.objectContaining({ runId: "run-1" }),
    );
  });

  it("clears selection and toasts when guard rerouting cannot resolve detail", async () => {
    automationGetMock.mockRejectedValue(new Error("gone"));

    await requestAutomationRunOpen(
      queryClient(),
      {
        projectId: "project-1",
        automationId: "automation-1",
        runId: "run-1",
        conversationId: "run-conversation-1",
      },
      { fallback: "clear-selection" },
    );

    expect(clearSelectionMock).toHaveBeenCalled();
    expect(toastErrorMock).toHaveBeenCalledWith("Could not open automation run.");
  });
});
