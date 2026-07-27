import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState, type ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { AgentConversationJiraIssue } from "@/api/atlassian";
import type {
  AgentConversationRuntimeStatus,
  AgentWorkspacePrReviewContext,
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
  AgentWorkspaceReviewAutoMergeGuardStatus,
  AgentWorkspaceReviewContext,
  StartAgentWorkspaceReviewResult,
} from "@/api/chat";
import type { AgentConversationGranolaNote } from "@/api/granola";
import type { AgentConversationLinearIssue } from "@/api/linear";
import type { ConversationTicket } from "@/api/ticketing";
import type {
  Automation,
  AutomationDetail,
  AutomationRun,
} from "@/api/automations";
import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";
import { buildStoreKey } from "@/lib/chat-context-registry";
import {
  useAgentSessionStore,
  type AgentArtifactTab,
} from "@/stores/agentSessionStore";
import { usePlanStore } from "@/stores/planStore";
import { useChatStore } from "@/stores/chatStore";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import { createTestQueryClient } from "@/test/store-utils";
import { chatKeys } from "@/hooks/useChat";
import { reviewSettingsKeys } from "@/hooks/useReviewSettings";
import { ticketingKeys } from "@/hooks/useTicketing";
import type { Task } from "@/types/task";
import { AgentsArtifactPane } from "./AgentsArtifactPane";
import { AgentPublishPanel } from "./AgentsPublishPanel";
import { agentProjectFixture } from "./agentsTestFixtures";
import { agentGranolaNoteKeys } from "./agentGranolaNoteQueries";
import { agentJiraIssueKeys } from "./agentJiraIssueQueries";
import { agentLinearIssueKeys } from "./agentLinearIssueQueries";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { agentConversationKeys } from "./useProjectAgentConversations";

const deferredHydrationTimeout = { timeout: 3_000 };
const { approvedPlanRuntime } = vi.hoisted(() => ({
  approvedPlanRuntime: {
    provider: "claude",
    model: "opus",
    effort: "high",
    serviceTier: "provider_default",
    coordinationMode: "solo",
    personaId: null,
  } as const,
}));
const workspaceReviewRuntimeOverride = vi.hoisted(() => ({
  current: null as ManualRoleRuntimeSelection | null,
}));

vi.mock("./useApprovedPlanContinuation", () => ({
  useApprovedPlanContinuation: () => ({
    confirmImplementDirectly: (
      onConfirm: (runtime: typeof approvedPlanRuntime) => Promise<unknown>,
    ) => void onConfirm(approvedPlanRuntime).catch(() => undefined),
    confirmCreateProposals: (
      onConfirm: (runtime: typeof approvedPlanRuntime) => Promise<unknown>,
    ) => void onConfirm(approvedPlanRuntime).catch(() => undefined),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: () => ({
    registry: {
      claude: [
        {
          id: "opus",
          label: "Opus",
          menuLabel: "Opus",
          defaultEffort: "high",
          supportedEfforts: ["high"],
        },
      ],
      codex: [],
    },
  }),
}));
const initialPlanStoreActions = {
  loadActivePlan: usePlanStore.getState().loadActivePlan,
  setActivePlan: usePlanStore.getState().setActivePlan,
  clearActivePlan: usePlanStore.getState().clearActivePlan,
  loadCandidates: usePlanStore.getState().loadCandidates,
};

const defaultReviewSettings = {
  require_human_review: false,
  require_workspace_review: true,
  max_fix_attempts: 3,
  max_revision_cycles: 5,
  ai_review_enabled: true,
  ai_review_auto_fix: true,
  require_fix_approval: false,
  auto_create_followup_agent_conversation: true,
  autofix_workspace_review_blocking_findings: true,
  run_task_validations: true,
};

const {
  getWorkspaceChangesMock,
  getWorkspaceChangeSummaryMock,
  getWorkspaceReviewMock,
  getWorkspaceDiffMock,
  getWorkspaceCommitsMock,
  getWorkspaceCommitChangesMock,
  getWorkspaceCommitDiffMock,
  getWorkspaceRepairSummaryMock,
  getWorkspaceRepairStagedChangesMock,
  getWorkspaceRepairUnstagedChangesMock,
  getWorkspaceRepairConflictDiffMock,
  getWorkspaceRepairStagedDiffMock,
  getWorkspaceRepairUnstagedDiffMock,
  getWorkspacePrAnnotationsMock,
  getConversationWorkspaceMock,
  getPrReviewContextMock,
  getWorkspaceReviewContextMock,
  getAgentConversationRuntimeStatusesMock,
  startWorkspaceReviewMock,
  startWorkspaceReviewFixerMock,
  approveWorkspaceReviewAnywayMock,
  listPublicationEventsMock,
  getWorkspaceFreshnessMock,
  updateWorkspaceFromBaseMock,
  setWorkspaceAutoPublishMock,
  setWorkspacePrSupervisionMock,
  setPrReviewAutoApproveMock,
  setPrReviewMonitoringMock,
  precomputePrDescriptionMock,
  closeWorkspacePrMock,
  sendAgentMessageMock,
  switchAgentConversationModeMock,
  activateAgentPlanDirectImplementationMock,
  activateAgentTaskPipelineMock,
  startAgentTaskPipelineMock,
  listAgentConversationIssuesMock,
  getAutomationMock,
  pauseAutomationMock,
  resumeAutomationMock,
  stopAutomationMock,
  loadBranchBaseOptionsMock,
  getArtifactMock,
  getSessionPlanMock,
  approvePlanArtifactMock,
  getPlanComplexityAssessmentMock,
  confirmVerificationMock,
  getVerificationSpecialistsMock,
  getIdeationSessionMock,
  getIdeationChildrenMock,
  restartImplementationMock,
  pauseExecutionPlanMock,
  resumeExecutionPlanMock,
  stopExecutionPlanMock,
  useTasksMock,
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
  toastDismissMock,
  toastErrorMock,
  toastInfoMock,
  toastLoadingMock,
  toastMessageMock,
  toastSuccessMock,
  tasksEnabledRef,
} = vi.hoisted(() => ({
  getWorkspaceChangesMock: vi.fn(),
  getWorkspaceChangeSummaryMock: vi.fn(),
  getWorkspaceReviewMock: vi.fn(),
  getWorkspaceDiffMock: vi.fn(),
  getWorkspaceCommitsMock: vi.fn(),
  getWorkspaceCommitChangesMock: vi.fn(),
  getWorkspaceCommitDiffMock: vi.fn(),
  getWorkspaceRepairSummaryMock: vi.fn(),
  getWorkspaceRepairStagedChangesMock: vi.fn(),
  getWorkspaceRepairUnstagedChangesMock: vi.fn(),
  getWorkspaceRepairConflictDiffMock: vi.fn(),
  getWorkspaceRepairStagedDiffMock: vi.fn(),
  getWorkspaceRepairUnstagedDiffMock: vi.fn(),
  getWorkspacePrAnnotationsMock: vi.fn(),
  getConversationWorkspaceMock: vi.fn(),
  getPrReviewContextMock: vi.fn(),
  getWorkspaceReviewContextMock: vi.fn(),
  getAgentConversationRuntimeStatusesMock: vi.fn(),
  startWorkspaceReviewMock: vi.fn(),
  startWorkspaceReviewFixerMock: vi.fn(),
  approveWorkspaceReviewAnywayMock: vi.fn(),
  listPublicationEventsMock: vi.fn(),
  getWorkspaceFreshnessMock: vi.fn(),
  updateWorkspaceFromBaseMock: vi.fn(),
  setWorkspaceAutoPublishMock: vi.fn(),
  setWorkspacePrSupervisionMock: vi.fn(),
  setPrReviewAutoApproveMock: vi.fn(),
  setPrReviewMonitoringMock: vi.fn(),
  precomputePrDescriptionMock: vi.fn(),
  closeWorkspacePrMock: vi.fn(),
  sendAgentMessageMock: vi.fn(),
  switchAgentConversationModeMock: vi.fn(),
  activateAgentPlanDirectImplementationMock: vi.fn(),
  activateAgentTaskPipelineMock: vi.fn(),
  startAgentTaskPipelineMock: vi.fn(),
  listAgentConversationIssuesMock: vi.fn(),
  getAutomationMock: vi.fn(),
  pauseAutomationMock: vi.fn(),
  resumeAutomationMock: vi.fn(),
  stopAutomationMock: vi.fn(),
  loadBranchBaseOptionsMock: vi.fn(),
  getArtifactMock: vi.fn(),
  getSessionPlanMock: vi.fn(),
  approvePlanArtifactMock: vi.fn(),
  getPlanComplexityAssessmentMock: vi.fn(),
  confirmVerificationMock: vi.fn(),
  getVerificationSpecialistsMock: vi.fn(),
  getIdeationSessionMock: vi.fn(),
  getIdeationChildrenMock: vi.fn(),
  restartImplementationMock: vi.fn(),
  pauseExecutionPlanMock: vi.fn(),
  resumeExecutionPlanMock: vi.fn(),
  stopExecutionPlanMock: vi.fn(),
  useTasksMock: vi.fn(),
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
  toastDismissMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastMessageMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  tasksEnabledRef: { current: true },
}));

vi.mock("@/hooks/useIdeationSettings", () => ({
  useIdeationSettings: () => ({
    settings: {
      tasksEnabled: tasksEnabledRef.current,
      tasksFeatureState: tasksEnabledRef.current ? "enabled" : "disabled",
      autoVerifyPlans: false,
      autoVerifyDraftPlans: true,
      requireAcceptForFinalize: false,
      requireVerificationForAccept: false,
      externalOverrides: {},
    },
    isLoading: false,
    isError: false,
  }),
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      getAgentConversationWorkspace: (...args: unknown[]) =>
        getConversationWorkspaceMock(...args),
      getAgentWorkspacePrReviewContext: (...args: unknown[]) =>
        getPrReviewContextMock(...args),
      getAgentWorkspaceReviewContext: (...args: unknown[]) =>
        getWorkspaceReviewContextMock(...args),
      getAgentConversationRuntimeStatuses: (...args: unknown[]) =>
        getAgentConversationRuntimeStatusesMock(...args),
      startAgentWorkspaceReview: (...args: unknown[]) =>
        startWorkspaceReviewMock(...args),
      startAgentWorkspaceReviewFixer: (...args: unknown[]) =>
        startWorkspaceReviewFixerMock(...args),
      approveAgentWorkspaceReviewAnyway: (...args: unknown[]) =>
        approveWorkspaceReviewAnywayMock(...args),
      listAgentConversationWorkspacePublicationEvents: (...args: unknown[]) =>
        listPublicationEventsMock(...args),
      getAgentConversationWorkspaceFreshness: (...args: unknown[]) =>
        getWorkspaceFreshnessMock(...args),
      updateAgentConversationWorkspaceFromBase: (...args: unknown[]) =>
        updateWorkspaceFromBaseMock(...args),
      setAgentConversationWorkspaceAutoPublish: (...args: unknown[]) =>
        setWorkspaceAutoPublishMock(...args),
      setAgentConversationWorkspacePrSupervision: (...args: unknown[]) =>
        setWorkspacePrSupervisionMock(...args),
      setAgentWorkspacePrReviewAutoApprove: (...args: unknown[]) =>
        setPrReviewAutoApproveMock(...args),
      setAgentWorkspacePrReviewMonitoring: (...args: unknown[]) =>
        setPrReviewMonitoringMock(...args),
      precomputeAgentConversationWorkspacePrDescription: (...args: unknown[]) =>
        precomputePrDescriptionMock(...args),
      closeAgentWorkspacePr: (...args: unknown[]) =>
        closeWorkspacePrMock(...args),
      sendAgentMessage: (...args: unknown[]) => sendAgentMessageMock(...args),
      switchAgentConversationMode: (...args: unknown[]) =>
        switchAgentConversationModeMock(...args),
      activateAgentPlanDirectImplementation: (...args: unknown[]) =>
        activateAgentPlanDirectImplementationMock(...args),
      activateAgentTaskPipeline: (...args: unknown[]) =>
        activateAgentTaskPipelineMock(...args),
      startAgentTaskPipeline: (...args: unknown[]) =>
        startAgentTaskPipelineMock(...args),
      listAgentConversationIssues: (...args: unknown[]) =>
        listAgentConversationIssuesMock(...args),
    },
  };
});

vi.mock("./useWorkspaceReviewActions", () => ({
  useWorkspaceReviewActions: ({
    onStartReview,
    onStartFixer,
  }: {
    onStartReview: (input: {
      force: boolean;
      confirmation?: {
        targetScope: string;
        diffFingerprint: string;
        headSha: string | null;
        prNumber: number | null;
        willDisableAutoMerge: boolean;
        mergeMethod: string | null;
        restoreAfterPublish: boolean;
      };
      runtimeOverride?: ManualRoleRuntimeSelection;
    }) => Promise<unknown>;
    onStartFixer: (input: {
      confirmation: {
        targetScope: string;
        diffFingerprint: string;
        artifactId: string;
        artifactVersion: number;
        blockingFingerprint: string;
      };
      runtimeOverride: typeof approvedPlanRuntime;
    }) => Promise<unknown>;
  }) => ({
    startReview: (force: boolean) => {
      const runtimeOverride = workspaceReviewRuntimeOverride.current;
      void onStartReview({
        force,
        ...(runtimeOverride
          ? {
              confirmation: {
                targetScope: "workspace_delta",
                diffFingerprint: "fingerprint-1",
                headSha: null,
                prNumber: null,
                willDisableAutoMerge: false,
                mergeMethod: null,
                restoreAfterPublish: false,
              },
              runtimeOverride,
            }
          : {}),
      }).catch(() => undefined);
    },
    startFixer: (context: AgentWorkspaceReviewContext) => {
      const { target, monitor } = context;
      if (
        !target ||
        !monitor.reviewArtifactId ||
        !monitor.reviewArtifactVersion ||
        !monitor.reviewBlockingFingerprint
      ) {
        return;
      }
      void onStartFixer({
        confirmation: {
          targetScope: target.scope,
          diffFingerprint: target.diffFingerprint,
          artifactId: monitor.reviewArtifactId,
          artifactVersion: monitor.reviewArtifactVersion,
          blockingFingerprint: monitor.reviewBlockingFingerprint,
        },
        runtimeOverride: approvedPlanRuntime,
      }).catch(() => undefined);
    },
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

vi.mock("@/api/automations", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/automations")>();
  return {
    ...actual,
    automationsApi: {
      ...actual.automationsApi,
      get: (...args: unknown[]) => getAutomationMock(...args),
      pause: (...args: unknown[]) => pauseAutomationMock(...args),
      resume: (...args: unknown[]) => resumeAutomationMock(...args),
      stop: (...args: unknown[]) => stopAutomationMock(...args),
    },
  };
});

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceFileChanges: (...args: unknown[]) =>
      getWorkspaceChangesMock(...args),
    getAgentConversationWorkspaceChangeSummary: (...args: unknown[]) =>
      getWorkspaceChangeSummaryMock(...args),
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
    getAgentConversationWorkspaceRepairChangeSummary: (...args: unknown[]) =>
      getWorkspaceRepairSummaryMock(...args),
    getAgentConversationWorkspaceRepairStagedFileChanges: (
      ...args: unknown[]
    ) => getWorkspaceRepairStagedChangesMock(...args),
    getAgentConversationWorkspaceRepairUnstagedFileChanges: (
      ...args: unknown[]
    ) => getWorkspaceRepairUnstagedChangesMock(...args),
    getAgentConversationWorkspaceRepairConflictFileDiff: (...args: unknown[]) =>
      getWorkspaceRepairConflictDiffMock(...args),
    getAgentConversationWorkspaceRepairStagedFileDiff: (...args: unknown[]) =>
      getWorkspaceRepairStagedDiffMock(...args),
    getAgentConversationWorkspaceRepairUnstagedFileDiff: (...args: unknown[]) =>
      getWorkspaceRepairUnstagedDiffMock(...args),
    getAgentConversationWorkspacePrAnnotations: (...args: unknown[]) =>
      getWorkspacePrAnnotationsMock(...args),
  },
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: () => () => undefined,
  }),
}));

vi.mock("@/components/shared/branchBaseOptions", async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import("@/components/shared/branchBaseOptions")
    >();
  return {
    ...actual,
    loadBranchBaseOptions: (...args: unknown[]) =>
      loadBranchBaseOptionsMock(...args),
  };
});

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
        restartImplementation: (...args: unknown[]) =>
          restartImplementationMock(...args),
      },
    },
  };
});

vi.mock("@/api/tasks", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/tasks")>();
  return {
    ...actual,
    tasksApi: {
      ...actual.tasksApi,
      pauseExecutionPlan: (...args: unknown[]) =>
        pauseExecutionPlanMock(...args),
      resumeExecutionPlan: (...args: unknown[]) =>
        resumeExecutionPlanMock(...args),
      stopExecutionPlan: (...args: unknown[]) => stopExecutionPlanMock(...args),
    },
  };
});

vi.mock("@/components/Ideation/VerificationPanel", () => ({
  VerificationPanel: ({ session }: { session: { id: string } }) => (
    <div data-testid="mock-verification-panel">{session.id}</div>
  ),
}));

vi.mock("@/components/tasks/TaskBoard", () => ({
  TaskBoard: ({
    onTaskSelect,
    readOnly,
  }: {
    onTaskSelect?: (taskId: string) => void;
    readOnly?: boolean;
  }) => (
    <button
      type="button"
      data-testid="mock-agent-task-card"
      data-read-only={readOnly ? "true" : "false"}
      onClick={() => onTaskSelect?.("task-1")}
    >
      Open task
    </button>
  ),
}));

vi.mock("@/components/TaskGraph", () => ({
  TaskGraphView: ({
    hidePlanSelector,
    readOnly,
  }: {
    hidePlanSelector?: boolean;
    readOnly?: boolean;
  }) => (
    <div
      data-testid="mock-agent-task-graph"
      data-read-only={readOnly ? "true" : "false"}
    >
      <div data-testid="floating-graph-filters">Graph filters</div>
      {!hidePlanSelector && (
        <div data-testid="global-plan-selector">Global plan selector</div>
      )}
    </div>
  ),
}));

vi.mock("@/components/agents/task-details/AgentsTaskDetailOverlay", () => ({
  AgentsTaskDetailOverlay: ({
    onFocusTaskRuntime,
    selectedTaskIdOverride,
    onCloseOverride,
    readOnly,
  }: {
    onFocusTaskRuntime?: (
      taskId: string,
      contextType: "task_execution" | "review" | "merge",
    ) => void;
    selectedTaskIdOverride?: string | null;
    onCloseOverride?: () => void;
    readOnly?: boolean;
  }) =>
    selectedTaskIdOverride ? (
      <div
        data-testid="mock-agent-task-detail"
        data-task-id={selectedTaskIdOverride}
        data-read-only={readOnly ? "true" : "false"}
      >
        <button
          type="button"
          onClick={() => onFocusTaskRuntime?.(selectedTaskIdOverride, "review")}
        >
          Focus review runtime
        </button>
        <button type="button" onClick={onCloseOverride}>
          Close task
        </button>
      </div>
    ) : null,
}));

vi.mock("@/components/pr/PullRequestDetailPanel", () => ({
  PullRequestDetailPanel: ({
    workspace,
  }: {
    workspace: AgentConversationWorkspace | null;
  }) => (
    <div data-testid="mock-pr-detail-panel">
      PR #
      {workspace?.publicationPrNumber ??
        workspace?.sourcePullRequest?.number ??
        "none"}
    </div>
  ),
}));

vi.mock("./AgentPlanStartPanel", () => ({
  AgentPlanStartPanel: ({
    conversationId,
    projectId,
    onPlanSeeded,
  }: {
    conversationId: string;
    projectId: string;
    onPlanSeeded: (result: {
      conversation: {
        id: string;
        contextType: "project";
        contextId: string;
      };
      workspace: {
        conversationId: string;
        projectId: string;
        mode: "plan";
      };
      sessionId: string;
      artifact: {
        id: string;
        name: string;
      };
      blueprintArtifact: {
        id: string;
        name: string;
      } | null;
    }) => void;
  }) => (
    <div
      data-testid="agent-plan-start-panel"
      data-conversation-id={conversationId}
      data-project-id={projectId}
    >
      <button
        type="button"
        onClick={() =>
          onPlanSeeded({
            conversation: {
              id: conversationId,
              contextType: "project",
              contextId: projectId,
            },
            workspace: {
              conversationId,
              projectId,
              mode: "plan",
            },
            sessionId: "seeded-session-1",
            artifact: {
              id: "seeded-plan-1",
              name: "Seeded plan",
            },
            blueprintArtifact: {
              id: "seeded-blueprint-1",
              name: "Seeded Blueprint",
            },
          })
        }
      >
        Seed plan
      </button>
    </div>
  ),
}));

vi.mock("@/api/artifact", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/artifact")>();
  return {
    ...actual,
    artifactApi: {
      ...actual.artifactApi,
      get: (...args: unknown[]) => getArtifactMock(...args),
      getSessionPlan: (...args: unknown[]) => getSessionPlanMock(...args),
      approvePlanArtifact: (...args: unknown[]) =>
        approvePlanArtifactMock(...args),
      getPlanComplexityAssessment: (...args: unknown[]) =>
        getPlanComplexityAssessmentMock(...args),
    },
  };
});

vi.mock("@/api/verification", () => ({
  verificationApi: {
    confirm: (...args: unknown[]) => confirmVerificationMock(...args),
    getSpecialists: (...args: unknown[]) =>
      getVerificationSpecialistsMock(...args),
  },
}));

vi.mock("@/hooks/useChat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/hooks/useChat")>();
  return {
    ...actual,
    useConversationHistoryWindow: (...args: unknown[]) =>
      useConversationMock(...args),
  };
});

vi.mock("@/hooks/useDependencyGraph", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/hooks/useDependencyGraph")>();
  return {
    ...actual,
    useDependencyGraph: (...args: unknown[]) => useDependencyGraphMock(...args),
    useDependencyTiers: () => ({ tierMap: new Map(), maxTier: 0 }),
  };
});

vi.mock("@/hooks/useTasks", () => ({
  useTasks: (...args: unknown[]) => useTasksMock(...args),
  useSessionTaskHistoryAvailability: (
    _projectId: string,
    sessionId: string | null,
  ) => {
    const result = useTasksMock() as {
      data?: Array<{ ideationSessionId?: string | null }>;
      isError?: boolean;
    };
    const taskCount = (result.data ?? []).filter(
      (task) => task.ideationSessionId === sessionId,
    ).length;
    return {
      data: { hasHistory: taskCount > 0, taskCount },
      isError: result.isError ?? false,
    };
  },
  taskKeys: {
    all: ["tasks"],
    lists: () => ["tasks", "list"],
    list: (projectId: string) => ["tasks", "list", projectId],
    sessionHistory: (projectId: string, sessionId: string) => [
      "tasks",
      "session-history",
      projectId,
      sessionId,
    ],
  },
}));

vi.mock("@/hooks/useVerificationStatus", () => ({
  useVerificationStatus: (...args: unknown[]) =>
    useVerificationStatusMock(...args),
  verificationStatusKey: (sessionId: string) =>
    ["verification", sessionId] as const,
}));

vi.mock("@/hooks/useFileDrop", () => ({
  useFileDrop: () => ({
    isDragging: false,
    dropProps: {},
    error: null,
    clearError: vi.fn(),
  }),
}));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGitAuthDiagnostics: (...args: unknown[]) =>
    useGitAuthDiagnosticsMock(...args),
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

vi.mock("@/hooks/useGitHubConnectionStatus", () => ({
  useGitHubConnectionStatus: (...args: unknown[]) =>
    useGhAuthStatusMock(...args),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (...args: unknown[]) => openUrlMock(...args),
}));

vi.mock("sonner", () => ({
  toast: {
    dismiss: (...args: unknown[]) => toastDismissMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    loading: (...args: unknown[]) => toastLoadingMock(...args),
    message: (...args: unknown[]) => toastMessageMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

const workspace = (
  overrides: Partial<AgentConversationWorkspace> = {},
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
  autoPublishEnabled: true,
  autoPublishInitialPrEnabled: false,
  autoPublishPausedPrAutofixEnabled: null,
  autoPublishPausedPrAutoMergeDesired: null,
  status: "active",
  createdAt: "2026-04-23T09:00:00Z",
  updatedAt: "2026-04-23T09:00:00Z",
  ...overrides,
});

const linearIssue = (
  overrides: Partial<AgentConversationLinearIssue> = {},
): AgentConversationLinearIssue => ({
  conversationId: "conversation-1",
  projectId: "project-1",
  provider: "linear",
  issueId: "linear-issue-1",
  issueKey: "LIN-123",
  issueUrl: "https://linear.app/example/issue/LIN-123",
  title: "Keep contextual tabs focused",
  status: "In Progress",
  assignee: null,
  reporter: null,
  updatedAtRemote: "2026-07-16T12:00:00Z",
  descriptionMarkdown: null,
  descriptionText: null,
  comments: [],
  attachments: [],
  lastRefreshedAt: "2026-07-16T12:00:00Z",
  refreshStatus: "loaded",
  refreshError: null,
  assignedAt: "2026-07-16T12:00:00Z",
  assignedFromMessageId: null,
  manuallyAssigned: true,
  createdAt: "2026-07-16T12:00:00Z",
  updatedAt: "2026-07-16T12:00:00Z",
  ...overrides,
});

const jiraIssue = (
  overrides: Partial<AgentConversationJiraIssue> = {},
): AgentConversationJiraIssue => ({
  conversationId: "conversation-1",
  projectId: "project-1",
  provider: "atlassian",
  issueKey: "RX-42",
  issueId: "jira-issue-42",
  issueUrl: "https://jira.example.com/browse/RX-42",
  title: "Keep contextual tabs focused",
  status: "In Progress",
  assignee: null,
  reporter: null,
  updatedAtRemote: "2026-07-16T12:00:00Z",
  descriptionMarkdown: null,
  descriptionText: null,
  acceptanceCriteriaMarkdown: null,
  acceptanceCriteriaText: null,
  comments: [],
  attachments: [],
  lastRefreshedAt: "2026-07-16T12:00:00Z",
  refreshStatus: "loaded",
  refreshError: null,
  assignedAt: "2026-07-16T12:00:00Z",
  assignedFromMessageId: null,
  manuallyAssigned: true,
  createdAt: "2026-07-16T12:00:00Z",
  updatedAt: "2026-07-16T12:00:00Z",
  ...overrides,
});

const granolaNote = (
  overrides: Partial<AgentConversationGranolaNote> = {},
): AgentConversationGranolaNote => ({
  conversationId: "conversation-1",
  projectId: "project-1",
  provider: "granola",
  noteId: "granola-note-1",
  noteUrl: "https://granola.ai/notes/granola-note-1",
  title: "Planning sync",
  summaryMarkdown: "Discussed contextual tabs.",
  transcript: [],
  includeTranscript: true,
  lastRefreshedAt: "2026-07-16T12:00:00Z",
  refreshStatus: "loaded",
  refreshError: null,
  assignedAt: "2026-07-16T12:00:00Z",
  assignedFromMessageId: null,
  manuallyAssigned: true,
  createdAt: "2026-07-16T12:00:00Z",
  updatedAt: "2026-07-16T12:00:00Z",
  ...overrides,
});

const clickUpTicket = (
  overrides: Partial<ConversationTicket> = {},
): ConversationTicket => ({
  ticketRef: { provider: "clickup", id: "clickup-task-1", key: "CU-1" },
  projectId: "project-1",
  title: "Restore rich ClickUp details",
  url: "https://app.clickup.com/t/clickup-task-1",
  ...overrides,
});

type IntegrationAttachments = Partial<{
  jira: AgentConversationJiraIssue | null;
  linear: AgentConversationLinearIssue | null;
  clickup: ConversationTicket | null;
  granola: AgentConversationGranolaNote | null;
}>;

function integrationQueryClient(
  attachments: IntegrationAttachments = {},
): QueryClient {
  const queryClient = createTestQueryClient();
  queryClient.setQueryData(["atlassian", "settings"], {
    enabled: true,
    jiraAvailable: true,
  });
  queryClient.setQueryData(["linear", "settings"], {
    enabled: true,
    issueSearchAvailable: true,
  });
  queryClient.setQueryData(["clickup-integration", "settings"], {
    enabled: true,
    hasApiToken: true,
    validationStatus: "valid",
    taskSearchAvailable: true,
  });
  queryClient.setQueryData(["granola", "settings"], {
    enabled: true,
    validationStatus: "valid",
  });
  queryClient.setQueryData(
    agentJiraIssueKeys.issue("conversation-1"),
    attachments.jira ?? null,
  );
  queryClient.setQueryData(
    agentLinearIssueKeys.issue("conversation-1"),
    attachments.linear ?? null,
  );
  queryClient.setQueryData(
    ticketingKeys.conversationTicket("conversation-1"),
    attachments.clickup ?? null,
  );
  queryClient.setQueryData(
    agentGranolaNoteKeys.note("conversation-1"),
    attachments.granola ?? null,
  );
  return queryClient;
}

const integrationTabCases = [
  {
    label: "Jira",
    tab: "jira" as const,
    attachments: { jira: jiraIssue() },
  },
  {
    label: "Linear",
    tab: "linear" as const,
    attachments: { linear: linearIssue() },
  },
  {
    label: "ClickUp",
    tab: "clickup" as const,
    attachments: { clickup: clickUpTicket() },
  },
  {
    label: "Granola",
    tab: "granola" as const,
    attachments: { granola: granolaNote() },
  },
];

const publishedPrSupervisionWorkspace = (
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace =>
  workspace({
    mode: "edit",
    publicationPrNumber: 90,
    publicationPrUrl: "https://github.com/mock/project/pull/90",
    publicationPrStatus: "open",
    publicationPushStatus: "pushed",
    prAutofixEnabled: true,
    prAutoMergeDesired: true,
    prAutoMergeCurrent: true,
    prSupervisionStatus: "monitoring",
    updatedAt: "2026-04-23T09:00:00Z",
    ...overrides,
  });

const workspaceFreshness = (
  overrides: Partial<AgentConversationWorkspaceFreshness> = {},
): AgentConversationWorkspaceFreshness => ({
  conversationId: "conversation-1",
  freshnessScope: "local",
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
  baseStatus: "valid",
  effectiveBaseRef: null,
  effectiveBaseDisplayName: null,
  baseBlockReason: null,
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

const automationFixture = (
  overrides: Partial<Automation> = {},
): Automation => ({
  id: "automation-1",
  projectId: "project-1",
  name: "Release automation",
  status: "active",
  pausedReasonCode: null,
  pausedReasonDetail: null,
  goalPrompt: "Ship the remaining release tasks.",
  setupConversationId: "conversation-setup",
  providerHarness: "codex",
  modelId: "gpt-5.4",
  logicalEffort: "medium",
  runMode: "edit",
  baseRefKind: "project_default",
  baseRef: "",
  baseDisplayName: "Project default (main)",
  baseSourcePullRequestJson: null,
  goalItemsJson: null,
  chainMode: "merged_base",
  completionSignal: "pr_merged",
  specArtifactId: null,
  planApprovalMode: "manual",
  prMergeMode: "manual",
  planDeepVerification: false,
  maxRuns: 25,
  maxConsecutiveFailures: 3,
  firstRunPrompt: null,
  setupAnalysisSummary: null,
  createdAt: "2026-07-05T10:00:00Z",
  updatedAt: "2026-07-05T10:00:00Z",
  ...overrides,
});

const automationRunFixture = (
  overrides: Partial<AutomationRun> = {},
): AutomationRun => ({
  id: "run-1",
  automationId: "automation-1",
  runIndex: 3,
  status: "published",
  judgeState: "none",
  judgeLeaseExpiresAt: null,
  planJudgeState: "none",
  planRevisionRound: 0,
  planRevisionPending: false,
  planPhase: false,
  planArtifactId: null,
  planApprovedBy: null,
  planApprovedArtifactVersion: null,
  planApprovedAt: null,
  conversationId: "conversation-1",
  runPrompt: "Continue the release automation.",
  promptAuthor: "judge",
  baseRefKind: "project_default",
  baseRefUsed: "main",
  baseFromRunId: "run-0",
  goalItemId: null,
  branchName: "ralphx/release/agent-1",
  prNumber: 593,
  prUrl: "https://github.com/aigentive/ralphx.app/pull/593",
  prTitle: "Release automation task",
  prHeadRefName: "ralphx/release/agent-1",
  prBaseRefName: "main",
  prMergedAt: null,
  mergeCommitSha: null,
  diffStatsJson: null,
  agentSummary: null,
  judgeVerdictJson: null,
  judgeModelId: null,
  errorCode: null,
  errorDetail: null,
  signalCheckFailures: 0,
  startedAt: "2026-07-05T10:00:00Z",
  finishedAt: null,
  createdAt: "2026-07-05T10:00:00Z",
  updatedAt: "2026-07-05T10:00:00Z",
  ...overrides,
});

const automationDetailFixture = (
  overrides: Partial<AutomationDetail> = {},
): AutomationDetail => ({
  automation: automationFixture(),
  runs: [automationRunFixture()],
  usage: {
    inputTokens: 0,
    outputTokens: 0,
    cacheCreationTokens: 0,
    cacheReadTokens: 0,
    estimatedUsd: null,
  },
  ...overrides,
});

function conversationRuntimeStatus(
  overrides: Partial<AgentConversationRuntimeStatus> = {},
): AgentConversationRuntimeStatus {
  const agentStatus = overrides.agentStatus ?? "generating";
  return {
    conversationId: "conversation-1",
    isRunning: agentStatus !== "idle",
    agentStatus,
    primarySource: "workspace",
    summaryLabel:
      agentStatus === "waiting_for_input" ? "Runtime waiting" : "Agent running",
    items: [
      {
        source: "workspace",
        contextType: "project",
        contextId: "conversation-1",
        label:
          agentStatus === "waiting_for_input"
            ? "Workspace waiting"
            : "Workspace running",
        title: "Workspace chat",
        agentStatus,
        taskId: null,
        internalStatus: null,
        runningProcess: null,
        ideationSession: null,
        parentSessionId: null,
        childSessionId: null,
        conversationId: "conversation-1",
      },
    ],
    ...overrides,
  };
}

const workspaceReviewTarget = {
  scope: "selected_source",
  baseRef: "base-sha",
  baseSha: "base-sha",
  headRef: "refs/ralphx/pr-heads/351",
  headSha: "head-sha",
  diffFingerprint: "fingerprint-351",
  sourcePullRequestNumber: 351,
};

function workspaceReviewContext(
  overrides: {
    conversationId?: string;
    target?: typeof workspaceReviewTarget | null;
    status?: "idle" | "ready" | "reviewing" | "blocked";
    reviewOutcome?:
      "none" | "passed" | "blocking" | "no_changes" | "run_failed";
    reviewGateStatus?:
      | "not_required"
      | "required"
      | "reviewing"
      | "passed"
      | "blocking"
      | "failed";
    reviewArtifactId?: string | null;
    reviewArtifactVersion?: number | null;
    reviewRequestedChangesArtifactId?: string | null;
    reviewRequestedChangesArtifactVersion?: number | null;
    reviewConversationId?: string | null;
    reviewBlockingSummary?: string | null;
    reviewBlockingFingerprint?: string | null;
    reviewFixerStatus?: string | null;
    reviewFixerRunId?: string | null;
    reviewFixerConversationId?: string | null;
    autoMergeGuardStatus?: AgentWorkspaceReviewAutoMergeGuardStatus | null;
    autoMergeGuardPrNumber?: number | null;
    autoMergeGuardMethod?: string | null;
    autoMergeGuardLastError?: string | null;
    reviewGateBypassedAt?: string | null;
    reviewGateBypassedTargetScope?:
      "selected_source" | "workspace_delta" | null;
    reviewGateBypassedDiffFingerprint?: string | null;
    reviewGateBypassedArtifactId?: string | null;
    reviewGateBypassedArtifactVersion?: number | null;
    isCurrent?: boolean;
    isOutdated?: boolean;
    shouldShowTab?: boolean;
    lastError?: string | null;
  } = {},
): AgentWorkspaceReviewContext {
  const target =
    overrides.target === undefined ? workspaceReviewTarget : overrides.target;
  const reviewArtifactId = overrides.reviewArtifactId ?? null;
  const conversationId = overrides.conversationId ?? "conversation-1";
  const reviewArtifactIsCurrent = overrides.isCurrent ?? false;
  const reviewArtifactIsOutdated = overrides.isOutdated ?? false;

  return {
    success: true,
    workspace: workspace({ conversationId, mode: "edit" }),
    events: [],
    target,
    monitor: {
      conversationId,
      projectId: "project-1",
      status: overrides.status ?? "idle",
      reviewOutcome: overrides.reviewOutcome ?? "none",
      reviewGateStatus: overrides.reviewGateStatus ?? "not_required",
      currentTargetScope: target?.scope ?? null,
      reviewedTargetScope:
        overrides.reviewOutcome === "passed" ? (target?.scope ?? null) : null,
      reviewConversationId: overrides.reviewConversationId ?? null,
      reviewArtifactId,
      reviewArtifactVersion: overrides.reviewArtifactVersion ?? null,
      reviewArtifactUpdatedAt: reviewArtifactId ? "2026-04-23T09:30:00Z" : null,
      reviewRequestedChangesArtifactId:
        overrides.reviewRequestedChangesArtifactId ?? null,
      reviewRequestedChangesArtifactVersion:
        overrides.reviewRequestedChangesArtifactVersion ?? null,
      reviewRequestedChangesArtifactUpdatedAt:
        overrides.reviewRequestedChangesArtifactId
          ? "2026-04-23T09:30:00Z"
          : null,
      reviewedHeadSha:
        overrides.reviewOutcome === "passed" ? (target?.headSha ?? null) : null,
      reviewedDiffFingerprint:
        overrides.reviewOutcome === "passed"
          ? (target?.diffFingerprint ?? null)
          : null,
      selectedSourceBaseRef:
        target?.scope === "selected_source" ? target.baseRef : null,
      selectedSourceBaseSha:
        target?.scope === "selected_source" ? target.baseSha : null,
      selectedSourceHeadRef:
        target?.scope === "selected_source" ? target.headRef : null,
      selectedSourceHeadSha:
        target?.scope === "selected_source" ? target.headSha : null,
      selectedSourcePullRequestNumber:
        target?.scope === "selected_source"
          ? (target.sourcePullRequestNumber ?? null)
          : null,
      workspaceBaseRef:
        target?.scope === "workspace_delta" ? target.baseRef : null,
      workspaceBaseSha:
        target?.scope === "workspace_delta" ? target.baseSha : null,
      workspaceHeadRef:
        target?.scope === "workspace_delta" ? target.headRef : null,
      workspaceHeadSha:
        target?.scope === "workspace_delta" ? target.headSha : null,
      currentDiffFingerprint: target?.diffFingerprint ?? null,
      previousVersionId: null,
      reviewRequestedChangesPreviousVersionId: null,
      reviewGateBypassedAt: overrides.reviewGateBypassedAt ?? null,
      reviewGateBypassedTargetScope:
        overrides.reviewGateBypassedTargetScope ?? null,
      reviewGateBypassedDiffFingerprint:
        overrides.reviewGateBypassedDiffFingerprint ?? null,
      reviewGateBypassedArtifactId:
        overrides.reviewGateBypassedArtifactId ?? null,
      reviewGateBypassedArtifactVersion:
        overrides.reviewGateBypassedArtifactVersion ?? null,
      reviewBlockingSummary: overrides.reviewBlockingSummary ?? null,
      reviewBlockingFingerprint:
        overrides.reviewBlockingFingerprint ??
        (overrides.reviewOutcome === "blocking"
          ? "blocking-fingerprint-1"
          : null),
      reviewFixerStatus: overrides.reviewFixerStatus ?? null,
      reviewFixerRunId: overrides.reviewFixerRunId ?? null,
      reviewFixerConversationId: overrides.reviewFixerConversationId ?? null,
      lastRunId: null,
      autoMergeGuardStatus: overrides.autoMergeGuardStatus ?? null,
      autoMergeGuardPrNumber: overrides.autoMergeGuardPrNumber ?? null,
      autoMergeGuardMethod: overrides.autoMergeGuardMethod ?? null,
      autoMergeGuardTargetScope:
        overrides.autoMergeGuardStatus == null ? null : "workspace_delta",
      autoMergeGuardDiffFingerprint:
        overrides.autoMergeGuardStatus == null ? null : "fingerprint-351",
      autoMergeGuardHeadSha:
        overrides.autoMergeGuardStatus == null ? null : "head-sha",
      autoMergeGuardLastError: overrides.autoMergeGuardLastError ?? null,
      lastError: overrides.lastError ?? null,
      createdAt: "2026-04-23T09:00:00Z",
      updatedAt: "2026-04-23T09:30:00Z",
    },
    reviewArtifactIsCurrent,
    reviewArtifactIsOutdated,
    canMutateReviewState: false,
    reviewRuntimeState: "missing_runtime_identity",
    isCurrent: reviewArtifactIsCurrent,
    isOutdated: reviewArtifactIsOutdated,
    shouldShowTab:
      overrides.shouldShowTab ?? Boolean(target || reviewArtifactId),
  };
}

function workspaceReviewArtifact(version = 2) {
  return {
    id: `review-artifact-${version}`,
    type: "workspace_review",
    name: "Workspace Review",
    content: {
      type: "inline",
      text: "# Workspace Review\n\nNo blocking findings.",
    },
    metadata: {
      createdAt: "2026-04-23T09:30:00Z",
      createdBy: "ralphx-workspace-reviewer",
      version,
    },
    derivedFrom: [],
    bucketId: "prd-library",
  };
}

function workspaceReviewRequestedChangesArtifact(version = 2) {
  return {
    ...workspaceReviewArtifact(version),
    id: `review-requested-changes-${version}`,
    name: "Workspace Review — Requested Changes",
    content: {
      type: "inline",
      text: "## Step 1\n\nImplement the exact repair.",
    },
  };
}

function primeWorkspaceReviewArtifactPair(
  queryClient: QueryClient,
  overview: ReturnType<typeof workspaceReviewArtifact>,
  requestedChanges: ReturnType<
    typeof workspaceReviewRequestedChangesArtifact
  >,
) {
  queryClient.setQueryData(["agents", "artifact", overview.id], overview);
  queryClient.setQueryData(
    ["agents", "artifact", requestedChanges.id],
    requestedChanges,
  );
}

function task(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    category: "feature",
    title: "Implement plan task",
    description: null,
    priority: 1,
    internalStatus: "ready",
    needsReviewPoint: false,
    ideationSessionId: "session-1",
    executionPlanId: undefined,
    createdAt: "2026-04-23T09:00:00Z",
    updatedAt: "2026-04-23T09:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    ...overrides,
  };
}

function taskProposal(overrides: Record<string, unknown> = {}) {
  return {
    id: "proposal-1",
    sessionId: "session-1",
    title: "Implement plan task",
    description: "Ship the accepted implementation work.",
    category: "frontend",
    steps: ["Implement"],
    acceptanceCriteria: ["The task is visible"],
    suggestedPriority: "high",
    priorityScore: 90,
    priorityReason: "Required for the plan",
    estimatedComplexity: "simple",
    userPriority: null,
    userModified: false,
    status: "accepted",
    createdTaskId: "task-current",
    planArtifactId: "artifact-1",
    planVersionAtCreation: 1,
    sortOrder: 0,
    createdAt: "2026-04-23T09:15:00Z",
    updatedAt: "2026-04-23T09:15:00Z",
    ...overrides,
  };
}

function ideationSessionResponse(
  sessionOverrides: Record<string, unknown> = {},
  proposals: Array<Record<string, unknown>> = [],
) {
  return {
    session: {
      id: "session-1",
      projectId: "project-1",
      title: "Planning session",
      titleSource: "auto",
      status: "active",
      planArtifactId: "artifact-1",
      seedTaskId: null,
      parentSessionId: null,
      createdAt: "2026-04-23T09:00:00Z",
      updatedAt: "2026-04-23T09:00:00Z",
      archivedAt: null,
      convertedAt: null,
      verificationStatus: "unverified",
      verificationInProgress: false,
      gapScore: null,
      inheritedPlanArtifactId: null,
      sessionPurpose: "general",
      sessionFlow: "planning",
      acceptanceStatus: null,
      ...sessionOverrides,
    },
    proposals,
    messages: [],
  };
}

function approvedPlanArtifact() {
  return {
    id: "artifact-1",
    type: "specification",
    name: "Implementation Plan",
    content: {
      type: "inline",
      text: "# Implementation Plan\n\nDo the work.",
    },
    metadata: {
      createdAt: "2026-04-23T09:00:00Z",
      createdBy: "orchestrator",
      version: 1,
    },
    derivedFrom: [],
    bucketId: "prd-library",
    planApproval: {
      status: "approved",
      approvedArtifactId: "artifact-1",
      approvedVersion: 1,
      approvedAt: "2026-04-23T09:30:00Z",
    },
  };
}

function approvedPlanBundleArtifact() {
  return {
    ...approvedPlanArtifact(),
    planContractVersion: 2,
    blueprint: {
      id: "blueprint-1",
      type: "specification",
      name: "Implementation Blueprint",
      content: {
        type: "inline",
        text: "# Implementation Blueprint\n\nFollow these detailed steps.",
      },
      metadata: {
        createdAt: "2026-04-23T09:01:00Z",
        createdBy: "orchestrator",
        version: 2,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    },
  };
}

function draftPlanArtifact() {
  return {
    ...approvedPlanArtifact(),
    planApproval: {
      status: "draft",
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function prReviewContext(
  conversationId: string,
  reviewArtifactId: string | null,
): AgentWorkspacePrReviewContext {
  return {
    success: true,
    workspace: workspace({ conversationId, mode: "review_pr" }),
    events: [],
    prNumber: 78,
    prUrl: "https://github.com/mock/project/pull/78",
    currentHeadSha: "head-sha",
    pendingActionHeadStatus: "current",
    health: null,
    reviewFeedback: null,
    monitor: {
      conversationId,
      projectId: "project-1",
      prNumber: 78,
      status: "watching",
      monitorEnabled: true,
      autoApproveEnabled: true,
      firstReviewCompleted: Boolean(reviewArtifactId),
      firstActionResolved: Boolean(reviewArtifactId),
      lastSeenHeadSha: "head-sha",
      lastReviewedHeadSha: reviewArtifactId ? "head-sha" : null,
      lastReviewRunId: reviewArtifactId ? "run-1" : null,
      lastReviewOutcome: reviewArtifactId ? "approved" : null,
      lastSubmittedReviewId: null,
      reviewArtifactId,
      reviewArtifactHeadSha: reviewArtifactId ? "head-sha" : null,
      reviewArtifactVersion: reviewArtifactId ? 1 : null,
      reviewArtifactUpdatedAt: reviewArtifactId ? "2026-04-23T09:30:00Z" : null,
      lastError: null,
      createdAt: "2026-04-23T09:00:00Z",
      updatedAt: "2026-04-23T09:30:00Z",
    },
    pendingAction: null,
    recentActions: [],
    issueCommentEvidence: [],
  };
}

function renderPane(
  activeTab: AgentArtifactTab = "tasks",
  paneWorkspace: AgentConversationWorkspace | null = workspace(),
  onPublishWorkspace = vi.fn(),
  isPublishingWorkspace = false,
  paneConversation = null,
  paneProps: Partial<ComponentProps<typeof AgentsArtifactPane>> = {},
  queryClient: QueryClient = createTestQueryClient(),
) {
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
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
    </QueryClientProvider>,
  );
}

function renderPublishPanelForWorkspaceRerender(
  initialWorkspace: AgentConversationWorkspace | null,
  queryClient: QueryClient = createTestQueryClient(),
  initialReviewContext: AgentWorkspaceReviewContext | null = null,
) {
  const pane = (
    paneWorkspace: AgentConversationWorkspace | null,
    reviewContext: AgentWorkspaceReviewContext | null,
  ) => (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <div className="h-[480px]">
          <AgentPublishPanel
            workspace={paneWorkspace}
            conversationTitle="Agent conversation"
            onPublishWorkspace={vi.fn()}
            isPublishingWorkspace={false}
            reviewContext={reviewContext}
            activeSubTab="automation"
            showReviewTab
            onSubTabChange={() => {}}
            reviewContent={null}
          />
        </div>
      </TooltipProvider>
    </QueryClientProvider>
  );
  const result = render(pane(initialWorkspace, initialReviewContext));
  return {
    ...result,
    rerenderWorkspace: (
      nextWorkspace: AgentConversationWorkspace | null,
      nextReviewContext: AgentWorkspaceReviewContext | null = initialReviewContext,
    ) => {
      result.rerender(pane(nextWorkspace, nextReviewContext));
    },
  };
}

function artifactTabIds(tabRow: HTMLElement): string[] {
  return Array.from(
    tabRow.querySelectorAll("[data-testid^='agents-artifact-tab-']"),
  ).map((tab) => tab.getAttribute("data-testid") ?? "");
}

async function openAutomationTab() {
  fireEvent.mouseDown(
    await screen.findByTestId("agents-publish-tab-automation"),
    { button: 0 },
  );
  await screen.findByTestId("agents-publish-content-automation");
}

async function openHistoryTab() {
  fireEvent.mouseDown(await screen.findByTestId("agents-publish-tab-history"), {
    button: 0,
  });
  await screen.findByTestId("agents-publish-content-history");
}

function renderControlledPane(
  initialTab: AgentArtifactTab,
  paneWorkspace: AgentConversationWorkspace | null = workspace(),
  paneConversation = conversation(),
  paneProps: Partial<ComponentProps<typeof AgentsArtifactPane>> = {},
) {
  function ControlledPane() {
    const [activeTab, setActiveTab] = useState<AgentArtifactTab>(initialTab);

    return (
      <QueryClientProvider client={createTestQueryClient()}>
        <TooltipProvider delayDuration={0}>
          <div className="h-[480px]">
            <AgentsArtifactPane
              conversation={paneConversation}
              workspace={paneWorkspace}
              activeTab={activeTab}
              taskMode="graph"
              onTabChange={setActiveTab}
              onTaskModeChange={() => {}}
              onPublishWorkspace={vi.fn()}
              isPublishingWorkspace={false}
              onClose={() => {}}
              {...paneProps}
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    );
  }

  return render(<ControlledPane />);
}

describe("AgentsArtifactPane", () => {
  beforeEach(() => {
    workspaceReviewRuntimeOverride.current = null;
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(defaultReviewSettings);
    tasksEnabledRef.current = true;
    useProjectStore.getState().setProjects([agentProjectFixture]);
    useProjectStore.getState().selectProject(agentProjectFixture.id);
    useChatStore.setState({ activeConversationIds: {} });
    getWorkspaceChangesMock.mockResolvedValue([
      {
        path: "frontend/src/App.tsx",
        status: "modified",
        additions: 4,
        deletions: 1,
      },
    ]);
    getWorkspaceChangeSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 0, additions: 0, deletions: 0 },
    });
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
            {
              kind: "deletion",
              content: "old",
              oldLineNum: 1,
              newLineNum: null,
            },
            {
              kind: "addition",
              content: "new",
              oldLineNum: null,
              newLineNum: 1,
            },
          ],
        },
      ],
      oldTotalLines: 1,
      newTotalLines: 1,
      isBinary: false,
    });
    getWorkspaceCommitsMock.mockResolvedValue([]);
    getWorkspaceCommitChangesMock.mockResolvedValue([
      {
        path: "frontend/src/App.tsx",
        status: "modified",
        additions: 4,
        deletions: 1,
      },
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
            {
              kind: "deletion",
              content: "old",
              oldLineNum: 1,
              newLineNum: null,
            },
            {
              kind: "addition",
              content: "new",
              oldLineNum: null,
              newLineNum: 1,
            },
          ],
        },
      ],
      oldTotalLines: 1,
      newTotalLines: 1,
      isBinary: false,
    });
    getWorkspaceRepairSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 1, additions: 4, deletions: 1 },
      unstaged: { fileCount: 1, additions: 2, deletions: 0 },
      conflicted: { fileCount: 1, files: ["frontend/src/App.tsx"] },
      repairState: {
        expectedBranch: "ralphx/demo/agent-conversation-1",
        checkedOutBranch: "HEAD",
        rebaseInProgress: true,
        mergeInProgress: false,
      },
    });
    getWorkspaceRepairStagedChangesMock.mockResolvedValue([
      {
        path: "frontend/src/Staged.tsx",
        status: "modified",
        additions: 4,
        deletions: 1,
        isGenerated: false,
      },
    ]);
    getWorkspaceRepairUnstagedChangesMock.mockResolvedValue([
      {
        path: "frontend/src/App.tsx",
        status: "modified",
        additions: 2,
        deletions: 0,
        isGenerated: false,
      },
    ]);
    getWorkspaceRepairConflictDiffMock.mockResolvedValue({
      filePath: "frontend/src/App.tsx",
      baseContent: "base\n",
      oursContent: "ours\n",
      theirsContent: "theirs\n",
      mergedWithMarkers:
        "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n",
      language: "typescript",
    });
    getWorkspaceRepairStagedDiffMock.mockResolvedValue({
      filePath: "frontend/src/Staged.tsx",
      language: "typescript",
      hunks: [],
      oldTotalLines: 1,
      newTotalLines: 1,
      isBinary: false,
    });
    getWorkspaceRepairUnstagedDiffMock.mockResolvedValue({
      filePath: "frontend/src/App.tsx",
      language: "typescript",
      hunks: [],
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
    getConversationWorkspaceMock.mockResolvedValue(null);
    getPrReviewContextMock.mockResolvedValue({
      success: true,
      workspace: workspace({ mode: "review_pr" }),
      events: [],
      prNumber: 78,
      prUrl: "https://github.com/mock/project/pull/78",
      currentHeadSha: "head-sha",
      pendingActionHeadStatus: "current",
      health: null,
      reviewFeedback: null,
      monitor: null,
      pendingAction: null,
      recentActions: [],
      issueCommentEvidence: [],
    });
    getWorkspaceReviewContextMock.mockResolvedValue({
      success: true,
      workspace: workspace({ mode: "edit" }),
      events: [],
      target: null,
      monitor: {
        status: "idle",
        reviewArtifactId: null,
        reviewArtifactVersion: null,
      },
      isCurrent: false,
      isOutdated: false,
      shouldShowTab: false,
    });
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({});
    startWorkspaceReviewMock.mockClear();
    startWorkspaceReviewMock.mockResolvedValue({
      success: true,
      target: null,
      monitor: {
        status: "idle",
        reviewArtifactId: null,
        reviewArtifactVersion: null,
      },
      isCurrent: false,
      isOutdated: false,
      shouldShowTab: false,
      started: false,
      skippedReason: "no_reviewable_changes",
      wasQueued: false,
    });
    startWorkspaceReviewFixerMock.mockClear();
    startWorkspaceReviewFixerMock.mockResolvedValue({
      success: true,
      target: null,
      monitor: {
        conversationId: "conversation-1",
        status: "idle",
        reviewArtifactId: null,
        reviewArtifactVersion: null,
        reviewFixerStatus: null,
        reviewFixerRunId: null,
        reviewFixerConversationId: null,
      },
      isCurrent: false,
      isOutdated: false,
      shouldShowTab: false,
      started: false,
      skippedReason: null,
    });
    approveWorkspaceReviewAnywayMock.mockReset();
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
    setWorkspacePrSupervisionMock.mockImplementation(
      async (
        conversationId: string,
        input: { autoFixEnabled: boolean; autoMergeDesired: boolean },
      ) =>
        workspace({
          mode: "edit",
          conversationId,
          publicationPrNumber: 90,
          publicationPrUrl: "https://github.com/mock/project/pull/90",
          publicationPrStatus: "open",
          publicationPushStatus: "pushed",
          prAutofixEnabled: input.autoFixEnabled,
          prAutoMergeDesired: input.autoMergeDesired,
          prAutoMergeMethod: "squash",
          prSupervisionStatus:
            input.autoFixEnabled || input.autoMergeDesired
              ? "monitoring"
              : "disabled",
        }),
    );
    setPrReviewAutoApproveMock.mockReset();
    setPrReviewMonitoringMock.mockReset();
    setWorkspaceAutoPublishMock.mockImplementation(
      async (conversationId: string, input: { autoPublishEnabled: boolean }) =>
        workspace({
          mode: "edit",
          conversationId,
          publicationPrNumber: 90,
          publicationPrUrl: "https://github.com/mock/project/pull/90",
          publicationPrStatus: "open",
          publicationPushStatus: "pushed",
          autoPublishEnabled: input.autoPublishEnabled,
          autoPublishInitialPrEnabled: input.autoPublishEnabled,
          prSupervisionStatus: input.autoPublishEnabled
            ? "monitoring"
            : "paused",
        }),
    );
    precomputePrDescriptionMock.mockClear();
    precomputePrDescriptionMock.mockResolvedValue({
      conversationId: "conversation-1",
      status: "ready",
      cacheStatus: "miss",
      reason: null,
    });
    loadBranchBaseOptionsMock.mockResolvedValue({
      options: [
        {
          key: "project_default:main",
          label: "Project default (main)",
          detail: "Configured project base branch",
          source: "project",
          selection: {
            kind: "project_default",
            ref: "main",
            displayName: "Project default (main)",
          },
        },
        {
          key: "local_branch:release/0.8",
          label: "release/0.8",
          detail: "Local branch",
          source: "local",
          selection: {
            kind: "local_branch",
            ref: "release/0.8",
            displayName: "release/0.8",
          },
        },
      ],
      selectedKey: "project_default:main",
    });
    closeWorkspacePrMock.mockResolvedValue(
      workspace({
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "closed",
        publicationPushStatus: "pushed",
      }),
    );
    sendAgentMessageMock.mockResolvedValue({
      conversationId: "ideation-conversation-1",
      agentRunId: "agent-run-1",
      isNewConversation: true,
      wasQueued: false,
      queuedMessageId: null,
      queuedAsPending: false,
    });
    switchAgentConversationModeMock.mockResolvedValue({
      conversation: conversation(),
      workspace: workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
      }),
    });
    activateAgentPlanDirectImplementationMock.mockResolvedValue({
      workspace: workspace({
        mode: "edit",
        linkedIdeationSessionId: "session-1",
      }),
      artifactReferences: [
        {
          artifactId: "artifact-1",
          kind: "plan",
          title: "Implementation Plan",
          sessionId: "session-1",
          version: 1,
          status: "approved",
        },
        {
          artifactId: "blueprint-1",
          kind: "plan",
          title: "Implementation Blueprint",
          sessionId: "session-1",
          version: 2,
          status: "approved",
        },
      ],
    });
    activateAgentTaskPipelineMock.mockResolvedValue(
      workspace({
        mode: "tasks",
        linkedIdeationSessionId: "session-1",
        taskPipelineSessionId: "session-1",
        taskPipelineAvailable: true,
      }),
    );
    startAgentTaskPipelineMock.mockResolvedValue({
      tasksCreated: 1,
      executionPlanId: "execution-plan-1",
    });
    listAgentConversationIssuesMock.mockResolvedValue([]);
    getAutomationMock.mockResolvedValue(automationDetailFixture());
    pauseAutomationMock.mockResolvedValue(
      automationFixture({ status: "paused" }),
    );
    resumeAutomationMock.mockResolvedValue(
      automationFixture({ status: "active" }),
    );
    stopAutomationMock.mockResolvedValue(
      automationFixture({ status: "stopped" }),
    );
    getArtifactMock.mockResolvedValue(null);
    getSessionPlanMock.mockResolvedValue(null);
    approvePlanArtifactMock.mockResolvedValue(null);
    getPlanComplexityAssessmentMock.mockResolvedValue(null);
    confirmVerificationMock.mockResolvedValue({ status: "ok" });
    getVerificationSpecialistsMock.mockResolvedValue({ specialists: [] });
    getIdeationSessionMock.mockResolvedValue(null);
    getIdeationChildrenMock.mockResolvedValue([]);
    restartImplementationMock.mockResolvedValue({
      sessionId: "session-1",
      oldExecutionPlanId: "exec-old",
      executionPlanId: "exec-new",
      archivedTaskCount: 1,
      createdTaskIds: ["task-new"],
    });
    pauseExecutionPlanMock.mockResolvedValue({
      executionPlanId: "exec-current",
      affectedCount: 1,
    });
    resumeExecutionPlanMock.mockResolvedValue({
      executionPlanId: "exec-current",
      affectedCount: 1,
    });
    stopExecutionPlanMock.mockResolvedValue({
      executionPlanId: "exec-current",
      affectedCount: 1,
    });
    useTasksMock.mockReturnValue({
      data: [],
      isLoading: false,
      isFetching: false,
    });
    usePlanStore.setState({
      activePlanByProject: {},
      activeExecutionPlanIdByProject: {},
      activePlanLoadedByProject: {},
      planCandidates: [],
      isLoading: false,
      error: null,
      ...initialPlanStoreActions,
    });
    useConversationMock.mockReturnValue({
      data: null,
      isLoading: false,
    });
    useDependencyGraphMock.mockReturnValue({
      data: null,
      isLoading: false,
    });
    useVerificationStatusMock.mockReturnValue({
      data: {
        status: "unverified",
        inProgress: false,
      },
      isLoading: false,
      isFetching: false,
    });
    useGitAuthDiagnosticsMock.mockReturnValue({
      data: {
        fetchUrl: "git@github.com:mock/project.git",
        pushUrl: "git@github.com:mock/project.git",
        fetchKind: "SSH",
        pushKind: "SSH",
        mixedAuthModes: false,
        githubHttpsCredentialHelperConfigured: false,
        canSwitchToSsh: false,
        suggestedSshUrl: null,
      },
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });
    useGhAuthStatusMock.mockReturnValue({
      data: {
        state: "authenticated",
        diagnostic: null,
        ghInstalled: true,
        authenticated: true,
        host: "github.com",
        account: "octocat",
      },
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });
    openUrlMock.mockResolvedValue(undefined);
    toastDismissMock.mockClear();
    toastErrorMock.mockClear();
    toastInfoMock.mockClear();
    toastLoadingMock.mockClear();
    toastMessageMock.mockClear();
    toastSuccessMock.mockClear();
    useUiStore.setState({ activeModal: null, modalContext: undefined });
    useAgentSessionStore.setState({
      focusedProjectId: null,
      selectedProjectId: null,
      selectedConversationId: null,
      startConversationDraft: null,
    });
    useChatStore.getState().setActiveConversation("project:project-1", null);
  });

  it("keeps one workspace toolbar fixed between the tabs and scrolling content", async () => {
    const user = userEvent.setup();
    renderControlledPane("publish", workspace({ mode: "edit" }));

    const tabRow = screen.getByTestId("agents-artifact-tab-row");
    const toolbar = screen.getByTestId("agents-workspace-toolbar");
    const publishContent = screen.getByTestId(
      "agents-artifact-content-publish",
    );
    expect(tabRow.nextElementSibling).toBe(toolbar);
    expect(toolbar.nextElementSibling).toBe(publishContent);
    expect(
      screen.queryByTestId("agents-publish-metadata-strip"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-artifact-tab-plan"));

    expect(screen.getByTestId("agents-workspace-toolbar")).toBe(toolbar);
    expect(toolbar.nextElementSibling).toBe(
      screen.getByTestId("agents-artifact-content-plan"),
    );
  });

  it.each([
    ["alpha", "beta"],
    ["beta", "alpha"],
  ])(
    "does not render %s approval state after switching to %s mid-approve",
    async (fromId, toId) => {
      const queryClient = createTestQueryClient();
      let resolveApproval:
        ((value: Record<string, string | number | null>) => void) | undefined;
      const approval = new Promise<Record<string, string | number | null>>(
        (resolve) => {
          resolveApproval = resolve;
        },
      );
      const rawPersona = (id: string, status: "draft" | "active") => ({
        id: status === "draft" ? `draft-${id}` : `persona-${id}`,
        artifact_id: null,
        project_id: "project-1",
        slug: `${id}-voice`,
        name: `${id.toUpperCase()} Voice`,
        description: `${id} description`,
        content: `${id.toUpperCase()} persona content`,
        status,
        version: 1,
        content_hash: `${id}-hash`,
        source_session_id: `conversation-${id}`,
        source_persona_id: null,
        source_content_hash: null,
        created_at: "2026-07-17T08:00:00Z",
        updated_at: "2026-07-17T08:00:00Z",
      });
      vi.mocked(invoke).mockImplementation(async (command, args) => {
        if (command === "get_persona") {
          const id = (args as { input: { id: string } }).input.id.replace(
            "draft-",
            "",
          );
          return rawPersona(id, "draft");
        }
        if (command === "approve_persona") {
          return approval;
        }
        return defaultReviewSettings;
      });
      const pane = (id: string) => (
        <QueryClientProvider client={queryClient}>
          <TooltipProvider delayDuration={0}>
            <div className="h-[480px]">
              <AgentsArtifactPane
                conversation={{
                  ...conversation(),
                  id: `conversation-${id}`,
                  agentMode: "persona_builder",
                  builderDraftId: `draft-${id}`,
                  builderResultPersonaId: null,
                }}
                workspace={null}
                activeTab="persona"
                taskMode="graph"
                onTabChange={() => {}}
                onTaskModeChange={() => {}}
                onPublishWorkspace={vi.fn()}
                isPublishingWorkspace={false}
                onClose={() => {}}
              />
            </div>
          </TooltipProvider>
        </QueryClientProvider>
      );
      const { rerender } = render(pane(fromId));
      const approveButton = await screen.findByRole("button", {
        name: "Approve Persona",
      });
      const oldScrollContainer = approveButton.closest(".overflow-y-auto");
      expect(oldScrollContainer).not.toBeNull();
      oldScrollContainer!.scrollTop = 48;
      approveButton.focus();
      fireEvent.click(approveButton);
      await waitFor(() =>
        expect(invoke).toHaveBeenCalledWith("approve_persona", {
          input: { id: `draft-${fromId}` },
        }),
      );

      rerender(pane(toId));
      expect(
        await screen.findByText(`${toId.toUpperCase()} persona content`),
      ).toBeInTheDocument();
      const incomingApprove = screen.getByRole("button", {
        name: "Approve Persona",
      });
      const newScrollContainer = incomingApprove.closest(".overflow-y-auto");
      expect(newScrollContainer).not.toBe(oldScrollContainer);
      expect(newScrollContainer).toHaveProperty("scrollTop", 0);
      expect(approveButton).not.toBeInTheDocument();

      await act(async () => {
        resolveApproval?.(rawPersona(fromId, "active"));
        await approval;
      });

      expect(
        screen.getByText(`${toId.toUpperCase()} persona content`),
      ).toBeInTheDocument();
      expect(
        screen.queryByText(`${fromId.toUpperCase()} persona content`),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Approve Persona" }),
      ).toBeInTheDocument();
    },
  );

  it("filters hidden tabs and exposes the right-click hide action", async () => {
    const onHideTab = vi.fn();
    renderPane(
      "plan",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      { hiddenTabs: ["verification"], onHideTab },
    );

    const planTab = await screen.findByTestId("agents-artifact-tab-plan");
    expect(
      screen.queryByTestId("agents-artifact-tab-verification"),
    ).not.toBeInTheDocument();

    fireEvent.contextMenu(planTab);
    await userEvent.click(await screen.findByText("Hide “Plan”"));
    expect(onHideTab).toHaveBeenCalledWith("plan", expect.any(Array));
  });

  it("keeps Persona Builder conversations focused on the Persona artifact", async () => {
    renderPane(
      "plan",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      {
        ...conversation(),
        agentMode: "persona_builder",
        builderDraftId: null,
        builderResultPersonaId: null,
      },
      { hiddenTabs: ["persona"] },
    );

    const tabRow = screen.getByTestId("agents-artifact-tab-row");
    expect(artifactTabIds(tabRow)).toEqual(["agents-artifact-tab-persona"]);
    expect(
      screen.getByTestId("agents-artifact-content-persona"),
    ).toBeInTheDocument();
    expect(
      screen.queryByLabelText("Customize artifact tabs"),
    ).not.toBeInTheDocument();

    fireEvent.contextMenu(screen.getByTestId("agents-artifact-tab-persona"));
    expect(screen.queryByText("Hide “Persona”")).not.toBeInTheDocument();
  });

  it("shows a recoverable empty state when every available tab is hidden", async () => {
    renderPane(
      "plan",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {
        hiddenTabs: [
          "issues",
          "plan",
          "verification",
          "tasks",
          "automation",
          "pr",
          "jira",
          "linear",
          "granola",
          "review",
          "publish",
        ],
      },
    );

    expect(await screen.findByText("All tabs are hidden")).toBeVisible();
    expect(
      screen.getAllByRole("button", { name: "Customize tabs" }),
    ).not.toHaveLength(0);
  });

  it("defaults to Plan and Commit & Publish when no contextual artifact is attached", async () => {
    renderPane(
      "plan",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      integrationQueryClient(),
    );

    await screen.findByTestId("agents-artifact-tab-plan");
    expect(
      artifactTabIds(screen.getByTestId("agents-artifact-tab-row")),
    ).toEqual(["agents-artifact-tab-plan", "agents-artifact-tab-publish"]);
  });

  it.each(integrationTabCases)(
    "shows $label when the integration is enabled and its resource is attached",
    async ({ tab, attachments }) => {
      renderPane(
        "plan",
        workspace({ mode: "edit" }),
        vi.fn(),
        false,
        conversation(),
        {},
        integrationQueryClient(attachments),
      );

      expect(
        await screen.findByTestId(`agents-artifact-tab-${tab}`),
      ).toBeVisible();
    },
  );

  it.each(integrationTabCases)(
    "keeps an explicitly hidden $label tab hidden after its resource is attached",
    async ({ tab, attachments }) => {
      renderPane(
        "plan",
        workspace({ mode: "edit" }),
        vi.fn(),
        false,
        conversation(),
        { hiddenTabs: [tab] },
        integrationQueryClient(attachments),
      );

      expect(
        await screen.findByTestId("agents-artifact-tab-plan"),
      ).toBeVisible();
      expect(
        screen.queryByTestId(`agents-artifact-tab-${tab}`),
      ).not.toBeInTheDocument();
    },
  );

  it("hides the Issues tab when a project conversation has no open issues", async () => {
    listAgentConversationIssuesMock.mockResolvedValue([]);

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    await waitFor(() =>
      expect(listAgentConversationIssuesMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    expect(
      screen.queryByTestId("agents-artifact-tab-issues"),
    ).not.toBeInTheDocument();
  });

  it("shows the Issues tab when a project conversation has open issues", async () => {
    listAgentConversationIssuesMock.mockResolvedValue([{ id: "issue-1" }]);

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId("agents-artifact-tab-issues"),
    ).toBeInTheDocument();
  });

  it("hydrates plan artifacts for an ideation conversation without a workspace link", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: null,
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "draft",
      },
    });

    renderPane("plan", null, vi.fn(), false, {
      ...conversation(),
      contextType: "ideation",
      contextId: "session-1",
      agentMode: "ideation",
    });

    await waitFor(() =>
      expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"),
    );
    await waitFor(() =>
      expect(getSessionPlanMock).toHaveBeenCalledWith("session-1"),
    );
    expect(screen.queryByText("No plan yet")).not.toBeInTheDocument();
  });

  it("anchors the active tab border to the bottom edge of the tab bar", async () => {
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [task({ id: "task-current", executionPlanId: "exec-current" })],
      isLoading: false,
      isFetching: false,
    });
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
      "border-color: var(--overlay-faint);",
    );
    expect(activeTab.parentElement?.className).toContain("self-stretch");
    expect(activeTab.className).toContain("self-stretch");
    expect(activeTab.getAttribute("data-theme-button-skip")).toBe("true");
    expect(inactiveTab.getAttribute("data-theme-button-skip")).toBe("true");
    expect(activeTab.className).not.toContain("border-b-2");
    expect(
      activeTab.querySelector(
        "span[style='background: var(--accent-primary);']",
      ),
    ).not.toBeNull();
    expect(
      inactiveTab.querySelector(
        "span[style='background: var(--accent-primary);']",
      ),
    ).toBeNull();
  });

  it("opens task details inside the Agents tasks artifact surface", async () => {
    const onTaskArtifactSelectionChange = vi.fn();
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [task({ id: "task-1", executionPlanId: "exec-current" })],
      isLoading: false,
      isFetching: false,
    });
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
      { taskMode: "kanban", onTaskArtifactSelectionChange },
    );

    fireEvent.click(await screen.findByTestId("mock-agent-task-card"));

    expect(await screen.findByTestId("mock-agent-task-detail")).toHaveAttribute(
      "data-task-id",
      "task-1",
    );
    expect(onTaskArtifactSelectionChange).toHaveBeenCalledWith("task-1");
  });

  it("keeps durable task history visible and read-only while Tasks are off", async () => {
    tasksEnabledRef.current = false;
    useTasksMock.mockReturnValue({
      data: [task({ id: "task-1", ideationSessionId: "session-1" })],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
      }),
    );

    renderPane(
      "tasks",
      workspace({ mode: "ideation", linkedIdeationSessionId: "session-1" }),
      vi.fn(),
      false,
      conversation(),
      { taskMode: "kanban" },
    );

    expect(
      await screen.findByTestId("agents-artifact-tab-tasks"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("tasks-read-only-banner")).toHaveTextContent(
      "Tasks are off",
    );
    const card = screen.getByTestId("mock-agent-task-card");
    expect(card).toHaveAttribute("data-read-only", "true");
    fireEvent.click(card);
    expect(await screen.findByTestId("mock-agent-task-detail")).toHaveAttribute(
      "data-read-only",
      "true",
    );
  });

  it("keeps a read-only Tasks shell visible when history availability fails", async () => {
    useTasksMock.mockReturnValue({
      data: [],
      isLoading: false,
      isFetching: false,
      isError: true,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
      }),
    );

    renderPane(
      "tasks",
      workspace({ mode: "ideation", linkedIdeationSessionId: "session-1" }),
      vi.fn(),
      false,
      conversation(),
      { taskMode: "graph" },
    );

    expect(
      await screen.findByTestId("agents-artifact-tab-tasks"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("tasks-read-only-banner")).toHaveTextContent(
      "Task history could not be checked",
    );
    expect(screen.getByTestId("mock-agent-task-graph")).toHaveAttribute(
      "data-read-only",
      "true",
    );
  });

  it("shows graph filters without the global plan selector in the Tasks artifact", async () => {
    renderPane(
      "tasks",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
      { taskMode: "graph" },
    );

    expect(
      await screen.findByTestId("floating-graph-filters"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("global-plan-selector"),
    ).not.toBeInTheDocument();
  });

  it("passes task runtime focus requests from task details to the host chat", async () => {
    const onFocusTaskRuntime = vi.fn();
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [task({ id: "task-1", executionPlanId: "exec-current" })],
      isLoading: false,
      isFetching: false,
    });
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
      { taskMode: "kanban", onFocusTaskRuntime },
    );

    fireEvent.click(await screen.findByTestId("mock-agent-task-card"));
    fireEvent.click(
      await screen.findByRole("button", { name: "Focus review runtime" }),
    );

    expect(onFocusTaskRuntime).toHaveBeenCalledWith("task-1", "review");
  });

  it("shows the automation artifact tab and opens the automation detail route", async () => {
    const onOpenAutomation = vi.fn();
    const automationConversation = {
      ...conversation(),
      agentMode: "automation" as const,
      automationId: "automation-1",
      automationRunId: "run-1",
    };

    renderPane(
      "automation",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      automationConversation,
      { onOpenAutomation },
    );

    expect(
      screen.getByTestId("agents-artifact-tab-automation"),
    ).toBeInTheDocument();
    expect(
      await screen.findByTestId("agents-automation-panel-loading"),
    ).toBeInTheDocument();
    await waitFor(
      () => expect(getAutomationMock).toHaveBeenCalledWith("automation-1"),
      deferredHydrationTimeout,
    );
  });

  it("applies the automation run tab policy instead of generic workspace tabs", async () => {
    const queryClient = createTestQueryClient();
    queryClient.setQueryData(["atlassian", "settings"], {
      enabled: true,
      jiraAvailable: true,
    });
    queryClient.setQueryData(["linear", "settings"], {
      enabled: true,
      issueSearchAvailable: true,
    });
    queryClient.setQueryData(["granola", "settings"], {
      enabled: true,
      validationStatus: "valid",
    });
    getAutomationMock.mockResolvedValue(
      automationDetailFixture({
        runs: [
          automationRunFixture({
            status: "published",
            planArtifactId: null,
            prNumber: 593,
            prUrl: "https://github.com/aigentive/ralphx.app/pull/593",
          }),
        ],
      }),
    );

    renderPane(
      "jira",
      workspace({
        mode: "edit",
        publicationPrNumber: 593,
        publicationPrUrl: "https://github.com/aigentive/ralphx.app/pull/593",
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
      }),
      vi.fn(),
      false,
      {
        ...conversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: "run-1",
      },
      {},
      queryClient,
    );

    await waitFor(() =>
      expect(getAutomationMock).toHaveBeenCalledWith("automation-1"),
    );

    expect(
      screen.getByTestId("agents-artifact-tab-automation"),
    ).toBeInTheDocument();
    expect(
      await screen.findByTestId("agents-artifact-tab-pr"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-artifact-tab-plan")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(
      screen.getByTestId("agents-artifact-tab-publish"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-jira"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-linear"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-granola"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByTestId("agents-artifact-content-pr"),
    ).toBeInTheDocument();
  });

  it("shows recovery instead of hidden Automation content when only disabled Plan remains", async () => {
    getAutomationMock.mockResolvedValue(
      automationDetailFixture({
        runs: [
          automationRunFixture({
            status: "running",
            planArtifactId: null,
            prNumber: null,
            prUrl: null,
          }),
        ],
      }),
    );

    renderPane(
      "automation",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      {
        ...conversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: "run-1",
      },
      { hiddenTabs: ["automation", "publish"] },
    );

    await waitFor(() =>
      expect(getAutomationMock).toHaveBeenCalledWith("automation-1"),
    );
    expect(screen.getByTestId("agents-artifact-tab-plan")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(
      await screen.findByTestId("agents-artifact-content-hidden"),
    ).toBeVisible();
    expect(
      screen.queryByTestId("agents-artifact-content-automation"),
    ).not.toBeInTheDocument();

    await userEvent.click(
      screen.getAllByRole("button", { name: "Customize tabs" })[0]!,
    );
    expect(
      screen.getByText("No run plan has been authored yet."),
    ).toBeVisible();
  });

  it("uses the automation tab as the setup-conversation fallback", async () => {
    const automationConversation = {
      ...conversation(),
      agentMode: "automation" as const,
      automationId: "automation-1",
      automationRunId: null,
    };

    renderPane(
      "plan",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      automationConversation,
    );

    expect(
      await screen.findByTestId("agents-automation-panel-loading"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-artifact-tab-automation"),
    ).toBeInTheDocument();
  });

  it("shows the versioned setup spec as a Plan tab", async () => {
    const queryClient = createTestQueryClient();
    getAutomationMock.mockResolvedValue(
      automationDetailFixture({
        automation: automationFixture({ specArtifactId: "spec-artifact-2" }),
      }),
    );
    const setupPlan = {
      ...draftPlanArtifact(),
      id: "spec-artifact-2",
      name: "Automation setup plan",
      content: {
        type: "inline" as const,
        text: "# Automation setup plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "automation-setup-agent",
        version: 2,
      },
    };
    getArtifactMock.mockResolvedValue(setupPlan);
    queryClient.setQueryData(
      ["agents", "artifact", "spec-artifact-2"],
      setupPlan,
    );

    renderPane(
      "plan",
      workspace({ mode: "automation" }),
      vi.fn(),
      false,
      {
        ...conversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: null,
      },
      {},
      queryClient,
    );

    expect(
      await screen.findByTestId("agents-artifact-tab-plan"),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("Automation setup plan"),
    ).toBeInTheDocument();
  });

  it("reuses Commit & Publish for the automation setup workspace", async () => {
    renderPane("publish", workspace({ mode: "automation" }), vi.fn(), false, {
      ...conversation(),
      agentMode: "automation",
      automationId: "automation-1",
      automationRunId: null,
    });

    expect(
      await screen.findByTestId("agents-artifact-tab-publish"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
  });

  it("selects task details from an external task focus request", async () => {
    const onTaskArtifactSelectionChange = vi.fn();
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [task({ id: "task-42", executionPlanId: "exec-current" })],
      isLoading: false,
      isFetching: false,
    });
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
      {
        taskFocusRequest: { taskId: "task-42", requestId: 1 },
        taskMode: "kanban",
        onTaskArtifactSelectionChange,
      },
    );

    expect(await screen.findByTestId("mock-agent-task-detail")).toHaveAttribute(
      "data-task-id",
      "task-42",
    );
    expect(onTaskArtifactSelectionChange).toHaveBeenCalledWith("task-42");
  });

  it("renders the Plan start panel for edit workspaces before an ideation run is attached", () => {
    renderPane(
      "plan",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(screen.getByTestId("agents-artifact-tab-plan")).toBeInTheDocument();
    expect(screen.getByTestId("agent-plan-start-panel")).toBeInTheDocument();
    expect(
      screen.queryByText("No ideation run attached"),
    ).not.toBeInTheDocument();
  });

  it("does not render the Plan start panel for automation run conversations", async () => {
    getAutomationMock.mockResolvedValue(
      automationDetailFixture({
        runs: [
          automationRunFixture({
            status: "awaiting_plan_approval",
            planArtifactId: "plan-artifact-1",
            prNumber: null,
            prUrl: null,
          }),
        ],
      }),
    );

    renderPane("plan", workspace({ mode: "edit" }), vi.fn(), false, {
      ...conversation(),
      agentMode: "automation",
      automationId: "automation-1",
      automationRunId: "run-1",
    });

    expect(
      await screen.findByTestId("agents-artifact-tab-plan"),
    ).toBeInTheDocument();
    expect(
      await screen.findByTestId("agents-artifact-content-plan"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agent-plan-start-panel"),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByText("No ideation run attached"),
    ).toBeInTheDocument();
  });

  it("keeps a focused automation run scoped to its Plan tab from the setup conversation", async () => {
    getAutomationMock.mockResolvedValue(
      automationDetailFixture({
        runs: [
          automationRunFixture({
            id: "run-3",
            status: "awaiting_plan_approval",
            planArtifactId: "plan-artifact-1",
            conversationId: "conversation-run-3",
            prNumber: null,
            prUrl: null,
          }),
        ],
      }),
    );
    getConversationWorkspaceMock.mockImplementation(
      async (conversationId: string) =>
        conversationId === "conversation-run-3"
          ? workspace({
              conversationId,
              mode: "plan",
              linkedIdeationSessionId: "session-1",
            })
          : null,
    );
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({ planArtifactId: "plan-artifact-1" }),
    );
    getSessionPlanMock.mockResolvedValue({
      ...draftPlanArtifact(),
      id: "plan-artifact-1",
    });

    renderPane(
      "plan",
      workspace({
        conversationId: "conversation-setup",
        mode: "automation",
      }),
      vi.fn(),
      false,
      {
        ...conversation(),
        id: "conversation-setup",
        title: "Ticket Attachment MCP Tools",
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: null,
      },
      {
        automationRunFocusTarget: {
          type: "automation_run",
          automationId: "automation-1",
          runId: "run-3",
          conversationId: "conversation-run-3",
        },
      },
    );

    await waitFor(() =>
      expect(
        screen.getByTestId("agents-artifact-tab-plan"),
      ).not.toHaveAttribute("aria-disabled"),
    );
    expect(
      await screen.findByTestId("agents-artifact-content-plan"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(getConversationWorkspaceMock).toHaveBeenCalledWith(
        "conversation-run-3",
      ),
    );
    expect(
      screen.queryByTestId("agent-plan-start-panel"),
    ).not.toBeInTheDocument();
  });

  it("fails closed when a focused automation run has no workspace", async () => {
    getConversationWorkspaceMock.mockResolvedValue(null);

    renderPane(
      "plan",
      workspace({
        conversationId: "conversation-setup",
        branchName: "parent-workspace-branch",
        mode: "automation",
      }),
      vi.fn(),
      false,
      {
        ...conversation(),
        id: "conversation-setup",
        agentMode: "automation",
        automationId: "automation-1",
      },
      {
        automationRunFocusTarget: {
          type: "automation_run",
          automationId: "automation-1",
          runId: "run-without-workspace",
          conversationId: "conversation-run-without-workspace",
        },
      },
    );

    expect(
      await screen.findByText("Workspace status unavailable"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("parent-workspace-branch"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Loading workspace status…"),
    ).not.toBeInTheDocument();
  });

  it("does not render cached focused workspace metadata after its refresh fails", async () => {
    const queryClient = createTestQueryClient();
    queryClient.setQueryData(
      agentWorkspaceKeys.workspace("conversation-run-stale"),
      workspace({
        conversationId: "conversation-run-stale",
        branchName: "stale-focused-branch",
        mode: "plan",
      }),
      { updatedAt: 0 },
    );
    getConversationWorkspaceMock.mockRejectedValue(
      new Error("Focused workspace refresh failed"),
    );

    renderPane(
      "plan",
      workspace({
        conversationId: "conversation-setup",
        branchName: "parent-workspace-branch",
        mode: "automation",
      }),
      vi.fn(),
      false,
      {
        ...conversation(),
        id: "conversation-setup",
        agentMode: "automation",
        automationId: "automation-1",
      },
      {
        automationRunFocusTarget: {
          type: "automation_run",
          automationId: "automation-1",
          runId: "run-stale",
          conversationId: "conversation-run-stale",
        },
      },
      queryClient,
    );

    expect(
      await screen.findByText("Workspace status unavailable"),
    ).toBeInTheDocument();
    expect(screen.queryByText("stale-focused-branch")).not.toBeInTheDocument();
    expect(
      screen.queryByText("parent-workspace-branch"),
    ).not.toBeInTheDocument();
  });

  it("keeps the empty Plan tab visible when Workspace Review is available under Publish", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "plan",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByTestId("agents-artifact-tab-publish")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-review")).not.toBeInTheDocument();
    expect(screen.getByTestId("agent-plan-start-panel")).toBeInTheDocument();
    expect(screen.queryByText("Review not run")).not.toBeInTheDocument();
  });

  it("updates workspace and plan caches when the Plan start panel seeds a plan", async () => {
    const user = userEvent.setup();
    const queryClient = createTestQueryClient();
    const setQueryDataSpy = vi.spyOn(queryClient, "setQueryData");
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    renderPane(
      "plan",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      queryClient,
    );

    await user.click(screen.getByRole("button", { name: "Seed plan" }));

    await waitFor(() =>
      expect(setQueryDataSpy).toHaveBeenCalledWith(
        agentWorkspaceKeys.workspace("conversation-1"),
        expect.objectContaining({
          conversationId: "conversation-1",
          projectId: "project-1",
          mode: "plan",
        }),
      ),
    );
    expect(setQueryDataSpy).toHaveBeenCalledWith(
      ["agents", "artifact", "seeded-plan-1"],
      expect.objectContaining({
        id: "seeded-plan-1",
        name: "Seeded plan",
      }),
    );
    expect(setQueryDataSpy).toHaveBeenCalledWith(
      ["agents", "session-plan", "seeded-session-1", "seeded-plan-1"],
      expect.objectContaining({
        id: "seeded-plan-1",
      }),
    );
    expect(setQueryDataSpy).toHaveBeenCalledWith(
      ["agents", "artifact", "seeded-blueprint-1"],
      expect.objectContaining({
        id: "seeded-blueprint-1",
        name: "Seeded Blueprint",
      }),
    );
    expect(setQueryDataSpy).toHaveBeenCalledWith(
      [
        "agents",
        "session-plan",
        "seeded-session-1",
        "seeded-blueprint-1",
      ],
      expect.objectContaining({
        id: "seeded-blueprint-1",
      }),
    );
    expect(setQueryDataSpy).toHaveBeenCalledWith(
      ["agents", "plan-approval", "seeded-session-1"],
      expect.objectContaining({
        id: "seeded-plan-1",
      }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: agentConversationKeys.project("project-1"),
    });
  });

  it("keeps non-plan ideation tabs hidden for edit workspaces without plan data", () => {
    renderPane("publish", workspace({ mode: "edit" }));

    expect(
      screen.getByTestId("agents-artifact-tab-publish"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(screen.getByTestId("agents-artifact-tab-plan")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-verification"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-proposal"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-tasks"),
    ).not.toBeInTheDocument();
  });

  it("renders the publish tab for blank plan workspaces", () => {
    renderPane(
      "publish",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      }),
    );

    expect(
      screen.getByTestId("agents-artifact-tab-publish"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-publish-tab-review"),
    ).not.toBeInTheDocument();
    expect(getWorkspaceReviewContextMock).not.toHaveBeenCalled();
    expect(
      screen.queryByTestId("agents-artifact-tab-pr"),
    ).not.toBeInTheDocument();
  });

  it("keeps local Review for automation-owned publish workspaces", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      {
        ...conversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: null,
      },
    );

    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(
      await screen.findByTestId("agents-publish-tab-review"),
    ).toBeInTheDocument();
    expect(getWorkspaceReviewContextMock).toHaveBeenCalled();
  });

  it("renders PR and publish tabs for PR-backed plan workspaces", () => {
    renderPane(
      "publish",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
        publicationPrNumber: 648,
        publicationPrUrl: "https://github.com/mock/project/pull/648",
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
      }),
    );

    expect(screen.getByTestId("agents-artifact-tab-pr")).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-artifact-tab-publish"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
  });

  it("hides local Review and falls back from a persisted Review tab in PLAN mode", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        reviewArtifactId: "stale-review-artifact",
        shouldShowTab: true,
      }),
    );

    renderPane(
      "review",
      workspace({ mode: "plan", linkedIdeationSessionId: null }),
    );

    expect(
      await screen.findByTestId("agents-artifact-content-plan"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-review"),
    ).not.toBeInTheDocument();
    expect(getWorkspaceReviewContextMock).not.toHaveBeenCalled();
  });

  it("returns to Changes when the current workspace becomes ineligible for Review", async () => {
    const queryClient = createTestQueryClient();
    const pane = (mode: "edit" | "plan") => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={0}>
          <div className="h-[480px]">
            <AgentsArtifactPane
              conversation={conversation()}
              workspace={workspace({ mode })}
              activeTab="publish"
              taskMode="graph"
              onTabChange={() => {}}
              onTaskModeChange={() => {}}
              onPublishWorkspace={vi.fn()}
              isPublishingWorkspace={false}
              onClose={() => {}}
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    );
    const result = render(pane("edit"));

    fireEvent.mouseDown(await screen.findByTestId("agents-publish-tab-review"), {
      button: 0,
    });
    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-tab-review")).toHaveAttribute(
        "data-state",
        "active",
      ),
    );

    result.rerender(pane("plan"));

    expect(
      screen.queryByTestId("agents-publish-tab-review"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-tab-changes")).toHaveAttribute(
      "data-state",
      "active",
    );
    expect(screen.getByTestId("agents-publish-content-changes")).toBeVisible();
  });

  it("shows the PR artifact tab for DB-backed workspace pull requests", async () => {
    renderPane(
      "pr",
      workspace({
        mode: "edit",
        publicationPrNumber: 42,
        publicationPrUrl: "https://github.com/acme/app/pull/42",
        publicationPrStatus: "open",
      }),
    );

    expect(screen.getByTestId("agents-artifact-tab-pr")).toBeInTheDocument();
    expect(await screen.findByTestId("mock-pr-detail-panel")).toHaveTextContent(
      "PR #42",
    );
  });

  it("renders Workspace Review inside Commit & Publish for editable workspaces", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 351,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(screen.queryByTestId("agents-artifact-tab-review")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-artifact-tab-publish")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-tab-changes")).toHaveAttribute(
      "data-state",
      "active",
    );
    expect(screen.getByTestId("agents-publish-tab-review")).toBeInTheDocument();

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-review"), {
      button: 0,
    });

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-tab-review")).toHaveAttribute(
        "data-state",
        "active",
      ),
    );
    expect(await screen.findByText("Review not run")).toBeInTheDocument();
  });

  it("normalizes a stored local Review tab to Commit & Publish Review", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByTestId("agents-artifact-content-publish")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-review")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-tab-review")).toHaveAttribute(
      "data-state",
      "active",
    );
    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Run review" }),
    ).toBeInTheDocument();
    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();
  });

  it("keeps the no-changes publish guard active when Review mounts first", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
    });

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByTestId("agents-artifact-content-publish")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-tab-review")).toHaveAttribute(
      "data-state",
      "active",
    );
    expect(
      await screen.findByRole(
        "heading",
        { name: "No changes to publish" },
        deferredHydrationTimeout,
      ),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-confirm")).toBeDisabled();
  });

  it("falls back to Commit & Publish when the workspace review needs attention", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "blocked",
        reviewOutcome: "blocking",
        reviewGateStatus: "blocking",
        reviewArtifactId: "review-artifact-1",
        shouldShowTab: true,
      }),
    );

    renderPane(
      "pr",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId(
        "agents-artifact-content-publish",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toBeInTheDocument();
  });

  it("does not auto-start Review when the user opens the Review tab", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );

    renderControlledPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 351,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      }),
    );

    fireEvent.mouseDown(await screen.findByTestId("agents-publish-tab-review"), {
      button: 0,
    });

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();
  });

  it("does not re-open generic Publish when switching to its Review sub-tab", async () => {
    const onTabChange = vi.fn();
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      { onTabChange },
    );

    fireEvent.mouseDown(await screen.findByTestId("agents-publish-tab-review"), {
      button: 0,
    });

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(onTabChange).not.toHaveBeenCalledWith("publish");
  });

  it("renders History and Automation as lazy publish destinations without Checks", async () => {
    listPublicationEventsMock.mockResolvedValue([
      {
        id: "event-history",
        conversationId: "conversation-1",
        step: "published",
        status: "succeeded",
        summary: "Published pull request",
        classification: null,
        createdAt: "2026-07-23T15:00:00Z",
      },
    ]);
    renderControlledPane("publish", workspace({ mode: "edit" }));

    const tabs = await screen.findByTestId("agents-publish-tabs");
    expect(
      Array.from(tabs.querySelectorAll('[role="tab"]')).map((tab) =>
        tab.getAttribute("data-testid"),
      ),
    ).toEqual([
      "agents-publish-tab-changes",
      "agents-publish-tab-review",
      "agents-publish-tab-history",
      "agents-publish-tab-automation",
    ]);
    expect(screen.queryByTestId("agents-publish-events")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-pr-supervision-controls"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Checks")).not.toBeInTheDocument();

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-history"), {
      button: 0,
    });

    expect(
      await screen.findByTestId("agents-publish-content-history"),
    ).toBeInTheDocument();
    expect(
      await screen.findByTestId("agents-publish-events"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-publish-content-changes"),
    ).toHaveAttribute("data-state", "inactive");

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-automation"), {
      button: 0,
    });

    expect(
      await screen.findByTestId("agents-publish-content-automation"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-pr-supervision-controls"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-tab-automation")).toHaveClass(
      "ml-auto",
    );
  });

  it("keeps History and Automation selected when the host re-opens the publish pane", async () => {
    // Production wires `onOpenPublish` to the Agents view helper that always
    // requests the `changes` sub-tab. Selecting History/Automation while the
    // publish artifact tab is already active must not round-trip through that
    // helper, otherwise the request snaps the pane straight back to Changes.
    listPublicationEventsMock.mockResolvedValue([
      {
        id: "event-history",
        conversationId: "conversation-1",
        step: "published",
        status: "succeeded",
        summary: "Published pull request",
        classification: null,
        createdAt: "2026-07-23T15:00:00Z",
      },
    ]);

    function HostControlledPane() {
      const [activeTab, setActiveTab] = useState<AgentArtifactTab>("publish");
      const [publishSubTabRequest, setPublishSubTabRequest] = useState<{
        conversationId: string;
        requestId: number;
        tab: "changes";
      } | null>(null);

      return (
        <QueryClientProvider client={createTestQueryClient()}>
          <TooltipProvider delayDuration={0}>
            <div className="h-[480px]">
              <AgentsArtifactPane
                conversation={conversation()}
                workspace={workspace({ mode: "edit" })}
                activeTab={activeTab}
                taskMode="graph"
                onTabChange={setActiveTab}
                onTaskModeChange={() => {}}
                onPublishWorkspace={vi.fn()}
                isPublishingWorkspace={false}
                onClose={() => {}}
                publishSubTabRequest={publishSubTabRequest}
                onOpenPublish={() => {
                  setActiveTab("publish");
                  setPublishSubTabRequest((current) => ({
                    conversationId: "conversation-1",
                    requestId: (current?.requestId ?? 0) + 1,
                    tab: "changes",
                  }));
                }}
              />
            </div>
          </TooltipProvider>
        </QueryClientProvider>
      );
    }

    render(<HostControlledPane />);

    fireEvent.mouseDown(await screen.findByTestId("agents-publish-tab-history"), {
      button: 0,
    });

    expect(
      await screen.findByTestId("agents-publish-content-history"),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByTestId("agents-publish-tab-history")).toHaveAttribute(
        "data-state",
        "active",
      );
    });
    expect(screen.getByTestId("agents-publish-tab-changes")).toHaveAttribute(
      "data-state",
      "inactive",
    );

    fireEvent.mouseDown(
      screen.getByTestId("agents-publish-tab-automation"),
      { button: 0 },
    );

    expect(
      await screen.findByTestId("agents-publish-content-automation"),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(
        screen.getByTestId("agents-publish-tab-automation"),
      ).toHaveAttribute("data-state", "active");
    });
    expect(screen.getByTestId("agents-publish-tab-changes")).toHaveAttribute(
      "data-state",
      "inactive",
    );
  });

  it("accepts fresh History requests without creating Review focus", async () => {
    const onFocusWorkspaceReview = vi.fn();

    renderControlledPane(
      "publish",
      workspace({ mode: "edit" }),
      conversation(),
      {
        onFocusWorkspaceReview,
        publishSubTabRequest: {
          conversationId: "conversation-1",
          requestId: 1,
          tab: "history",
        },
      },
    );

    expect(
      await screen.findByTestId("agents-publish-tab-history"),
    ).toHaveAttribute("data-state", "active");
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();
  });

  it("accepts a fresh Checks request for a URL-only published workspace", async () => {
    const onFocusWorkspaceReview = vi.fn();

    renderControlledPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrUrl: "https://github.com/acme/app/pull/351",
      }),
      conversation(),
      {
        onFocusWorkspaceReview,
        publishSubTabRequest: {
          conversationId: "conversation-1",
          requestId: 1,
          tab: "checks",
        },
      },
    );

    expect(
      await screen.findByTestId("agents-publish-tab-checks"),
    ).toHaveAttribute("data-state", "active");
    expect(
      await screen.findByTestId("agents-publish-checks-shell"),
    ).toBeInTheDocument();
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();
  });

  it("falls back to Changes when requested Checks is unavailable", async () => {
    renderControlledPane(
      "publish",
      workspace({
        mode: "edit",
        sourcePullRequest: {
          number: 351,
          url: "https://github.com/acme/app/pull/351",
          title: "Source PR",
          headRefName: "source/pr",
          baseRefName: "main",
          headRefOid: null,
        },
      }),
      conversation(),
      {
        publishSubTabRequest: {
          conversationId: "conversation-1",
          requestId: 1,
          tab: "checks",
        },
      },
    );

    expect(
      await screen.findByTestId("agents-publish-tab-changes"),
    ).toHaveAttribute("data-state", "active");
    expect(
      screen.queryByTestId("agents-publish-tab-checks"),
    ).not.toBeInTheDocument();
  });

  it("returns to Changes when the active workspace loses Checks eligibility", async () => {
    const queryClient = createTestQueryClient();
    const pane = (paneWorkspace: AgentConversationWorkspace) => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={0}>
          <div className="h-[480px]">
            <AgentsArtifactPane
              conversation={conversation()}
              workspace={paneWorkspace}
              activeTab="publish"
              taskMode="graph"
              onTabChange={() => {}}
              onTaskModeChange={() => {}}
              onPublishWorkspace={vi.fn()}
              isPublishingWorkspace={false}
              onClose={() => {}}
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    );
    const result = render(
      pane(
        workspace({
          mode: "edit",
          publicationPrNumber: 351,
          publicationPrUrl: "https://github.com/acme/app/pull/351",
        }),
      ),
    );

    fireEvent.mouseDown(await screen.findByTestId("agents-publish-tab-checks"), {
      button: 0,
    });
    expect(screen.getByTestId("agents-publish-tab-checks")).toHaveAttribute(
      "data-state",
      "active",
    );

    result.rerender(pane(workspace({ mode: "edit" })));

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-tab-changes")).toHaveAttribute(
        "data-state",
        "active",
      ),
    );
    expect(
      screen.queryByTestId("agents-publish-tab-checks"),
    ).not.toBeInTheDocument();
  });

  it("falls back to Changes when requested Automation is unavailable", async () => {
    renderControlledPane(
      "publish",
      workspace({ mode: "plan" }),
      conversation({ agentMode: "plan" }),
      {
        publishSubTabRequest: {
          conversationId: "conversation-1",
          requestId: 1,
          tab: "automation",
        },
      },
    );

    expect(
      await screen.findByTestId("agents-publish-tab-changes"),
    ).toHaveAttribute("data-state", "active");
    expect(
      screen.queryByTestId("agents-publish-tab-automation"),
    ).not.toBeInTheDocument();
  });

  it("focuses the workspace Review chat when the user opens the Review tab", async () => {
    const onFocusWorkspaceReview = vi.fn();
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        reviewConversationId: "review-conversation-1",
        shouldShowTab: true,
      }),
    );

    renderControlledPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 351,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      }),
      conversation(),
      { onFocusWorkspaceReview },
    );

    fireEvent.mouseDown(await screen.findByTestId("agents-publish-tab-review"), {
      button: 0,
    });

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(onFocusWorkspaceReview).toHaveBeenCalledWith(
      "review-conversation-1",
    );
  });

  it("focuses the reviewer child after a programmatic Review tab request hydrates", async () => {
    const onFocusWorkspaceReview = vi.fn();
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        reviewConversationId: "review-conversation-1",
        shouldShowTab: true,
      }),
    );

    renderControlledPane(
      "publish",
      workspace({ mode: "edit" }),
      conversation(),
      {
        onFocusWorkspaceReview,
        publishSubTabRequest: {
          conversationId: "conversation-1",
          requestId: 1,
          tab: "review",
        },
      },
    );

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(onFocusWorkspaceReview).toHaveBeenCalledWith(
      "review-conversation-1",
    );
  });

  it("does not focus a late reviewer child after the user returns to Changes", async () => {
    const onFocusWorkspaceReview = vi.fn();
    const context = deferred<AgentWorkspaceReviewContext>();
    getWorkspaceReviewContextMock.mockReturnValue(context.promise);

    renderControlledPane(
      "publish",
      workspace({ mode: "edit" }),
      conversation(),
      { onFocusWorkspaceReview },
    );

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-review"), {
      button: 0,
    });
    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-changes"), {
      button: 0,
    });

    await act(async () => {
      context.resolve(
        workspaceReviewContext({
          target: workspaceReviewTarget,
          reviewConversationId: "review-conversation-late",
          shouldShowTab: true,
        }),
      );
      await context.promise;
    });
    await screen.findByText("Review not run");

    expect(screen.getByTestId("agents-publish-tab-changes")).toHaveAttribute(
      "data-state",
      "active",
    );
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();
  });

  it("consumes a nested-tab request once across conversation switches", async () => {
    const queryClient = createTestQueryClient();
    const staleRequest = {
      conversationId: "conversation-1",
      requestId: 1,
      tab: "changes" as const,
    };
    getWorkspaceReviewContextMock.mockImplementation((conversationId: string) =>
      Promise.resolve(
        workspaceReviewContext({
          conversationId,
          target: workspaceReviewTarget,
          shouldShowTab: true,
        }),
      ),
    );
    const pane = (conversationId: string) => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={0}>
          <div className="h-[480px]">
            <AgentsArtifactPane
              conversation={{ ...conversation(), id: conversationId }}
              workspace={workspace({ conversationId, mode: "edit" })}
              activeTab="publish"
              taskMode="graph"
              onTabChange={() => {}}
              onTaskModeChange={() => {}}
              onPublishWorkspace={vi.fn()}
              isPublishingWorkspace={false}
              onClose={() => {}}
              publishSubTabRequest={staleRequest}
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    );
    const result = render(pane("conversation-1"));

    fireEvent.mouseDown(await screen.findByTestId("agents-publish-tab-review"), {
      button: 0,
    });
    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-tab-review")).toHaveAttribute(
        "data-state",
        "active",
      ),
    );

    result.rerender(pane("conversation-2"));
    result.rerender(pane("conversation-1"));

    expect(screen.getByTestId("agents-publish-tab-review")).toHaveAttribute(
      "data-state",
      "active",
    );
  });

  it("opens the exact Workspace Review transcript without starting review mutations", async () => {
    const onFocusWorkspaceReview = vi.fn();
    const onPublishWorkspace = vi.fn().mockResolvedValue(undefined);
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewConversationId: "review-conversation-current",
        shouldShowTab: true,
      }),
    );

    renderControlledPane(
      "review",
      workspace({ mode: "edit" }),
      conversation(),
      {
        onFocusWorkspaceReview,
        onPublishWorkspace,
      },
    );

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "View transcript" }));

    expect(onFocusWorkspaceReview).toHaveBeenCalledOnce();
    expect(onFocusWorkspaceReview).toHaveBeenCalledWith(
      "review-conversation-current",
    );
    expect(screen.getByTestId("agents-publish-tab-review")).toHaveAttribute(
      "data-state",
      "active",
    );
    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();
    expect(startWorkspaceReviewFixerMock).not.toHaveBeenCalled();
    expect(approveWorkspaceReviewAnywayMock).not.toHaveBeenCalled();
    expect(onPublishWorkspace).not.toHaveBeenCalled();
  });

  it("does not focus Review chat when the Review tab has no child conversation", async () => {
    const onFocusWorkspaceReview = vi.fn();
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        reviewConversationId: null,
        shouldShowTab: true,
      }),
    );

    renderControlledPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 351,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      }),
      conversation(),
      { onFocusWorkspaceReview },
    );

    fireEvent.mouseDown(await screen.findByTestId("agents-publish-tab-review"), {
      button: 0,
    });

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "View transcript" }),
    ).not.toBeInTheDocument();
  });

  it("opens Review and focuses the Review chat from the publish Review CTA", async () => {
    const onFocusWorkspaceReview = vi.fn();
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        reviewConversationId: "review-conversation-1",
        reviewGateStatus: "required",
        shouldShowTab: true,
      }),
    );

    renderControlledPane(
      "publish",
      workspace({ mode: "edit" }),
      conversation(),
      { onFocusWorkspaceReview },
    );

    fireEvent.click(
      await screen.findByTestId("agents-publish-review-required"),
    );

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(onFocusWorkspaceReview).toHaveBeenCalledWith(
      "review-conversation-1",
    );
  });

  it("does not block publishing on a required Review gate when policy is disabled", async () => {
    const queryClient = createTestQueryClient();
    const disabledReviewSettings = {
      require_human_review: false,
      require_workspace_review: false,
      max_fix_attempts: 3,
      max_revision_cycles: 5,
      ai_review_enabled: true,
      ai_review_auto_fix: true,
      require_fix_approval: false,
      auto_create_followup_agent_conversation: true,
      autofix_workspace_review_blocking_findings: true,
      run_task_validations: true,
    };
    queryClient.setQueryData(reviewSettingsKeys.all, disabledReviewSettings);
    vi.mocked(invoke).mockResolvedValue(disabledReviewSettings);
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        reviewConversationId: "review-conversation-1",
        reviewGateStatus: "required",
        shouldShowTab: true,
      }),
    );

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      queryClient,
    );

    await waitFor(() =>
      expect(getWorkspaceReviewContextMock).toHaveBeenCalledWith(
        "conversation-1",
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ),
    );
    await screen.findByTestId("agents-publish-tab-review");
    expect(
      screen.queryByTestId("agents-publish-review-required"),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByTestId("agents-publish-confirm"),
    ).toBeInTheDocument();
  });

  it("labels the internal Review tab when review is required", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        reviewGateStatus: "required",
        shouldShowTab: true,
      }),
    );

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    const reviewTab = await screen.findByTestId("agents-publish-tab-review");

    expect(await within(reviewTab).findByText("Required")).toBeInTheDocument();
  });

  it("labels and animates the internal Review tab while review is running", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "reviewing",
        reviewGateStatus: "reviewing",
        shouldShowTab: true,
      }),
    );

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    const reviewTab = await screen.findByTestId("agents-publish-tab-review");

    expect(await within(reviewTab).findByText("Running")).toBeInTheDocument();
    expect(reviewTab.querySelector("svg")).toHaveClass("animate-pulse");
  });

  it("colors the Review tab as passed only after the review gate passes", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewOutcome: "passed",
        reviewGateStatus: "passed",
        reviewArtifactId: "review-artifact-1",
        isCurrent: true,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    const reviewTab = await screen.findByTestId("agents-publish-tab-review");
    await within(reviewTab).findByText("Passed");
    const reviewIcon = reviewTab.querySelector("svg");

    expect(reviewIcon).toHaveStyle({ color: "var(--status-success)" });
  });

  it("colors an approved-anyway blocking Review tab as a warning", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewOutcome: "blocking",
        reviewGateStatus: "passed",
        reviewArtifactId: "review-artifact-1",
        reviewArtifactVersion: 2,
        reviewGateBypassedAt: "2026-07-10T00:05:00.000Z",
        reviewGateBypassedTargetScope: "selected_source",
        reviewGateBypassedDiffFingerprint: "fingerprint-351",
        reviewGateBypassedArtifactId: "review-artifact-1",
        reviewGateBypassedArtifactVersion: 2,
        isCurrent: true,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    const reviewTab = await screen.findByTestId("agents-publish-tab-review");
    await within(reviewTab).findByText("Approved");
    const reviewIcon = reviewTab.querySelector("svg");

    expect(reviewIcon).toHaveStyle({ color: "var(--status-warning)" });
    expect(reviewIcon).not.toHaveStyle({ color: "var(--status-success)" });
  });

  it("starts an initial Review only from the Run review action", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );
    startWorkspaceReviewMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "reviewing",
        shouldShowTab: true,
      }),
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Run review" }));

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: false,
      }),
    );
    expect(toastMessageMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("uses the parent Review owner while a reviewer child conversation is selected", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        conversationId: "parent-conversation",
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "review",
      workspace({ conversationId: "parent-conversation", mode: "edit" }),
      vi.fn(),
      false,
      {
        ...conversation(),
        id: "review-child-conversation",
        parentConversationId: "parent-conversation",
      },
    );

    await waitFor(() =>
      expect(getWorkspaceReviewContextMock).toHaveBeenCalledWith(
        "parent-conversation",
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ),
    );
    expect(getWorkspaceReviewContextMock).not.toHaveBeenCalledWith(
      "review-child-conversation",
      expect.anything(),
    );
  });

  it("keeps a failed Review start visible after the mutation settles", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );
    startWorkspaceReviewMock.mockRejectedValue(
      new Error("could not disable GitHub auto-merge before workspace Review"),
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );
    expect(await screen.findByText("Review not run")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Run review" }));

    expect(
      await screen.findByText(
        "could not disable GitHub auto-merge before workspace Review",
      ),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("agents-publish-tab-review")).getByText(
        "Failed",
      ),
    ).toBeInTheDocument();
  });

  it("hydrates the shared Review context, focuses the child chat, and invalidates the transcript after starting", async () => {
    const queryClient = createTestQueryClient();
    const onFocusWorkspaceReview = vi.fn();
    const initialContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      shouldShowTab: true,
    });
    const startedContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      status: "reviewing",
      reviewGateStatus: "reviewing",
      reviewConversationId: "review-conversation-1",
      shouldShowTab: true,
    });
    getWorkspaceReviewContextMock
      .mockResolvedValueOnce(initialContext)
      .mockResolvedValue(startedContext);
    startWorkspaceReviewMock.mockResolvedValue(startedContext);
    const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      { onFocusWorkspaceReview },
      queryClient,
    );

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Run review" }));

    await waitFor(() =>
      expect(
        queryClient.getQueryData(
          agentWorkspaceKeys.workspaceReview("conversation-1"),
        ),
      ).toEqual(startedContext),
    );
    expect(invalidateQueriesSpy).toHaveBeenCalledWith({
      queryKey: chatKeys.conversationTimeline("review-conversation-1"),
    });
    expect(onFocusWorkspaceReview).toHaveBeenCalledWith(
      "review-conversation-1",
    );
  });

  it("carries the committed reviewer runtime only to a newly started child focus", async () => {
    const queryClient = createTestQueryClient();
    const onFocusWorkspaceReview = vi.fn();
    workspaceReviewRuntimeOverride.current = {
      provider: "codex",
      model: "gpt-5.5",
      effort: "high",
      serviceTier: "standard",
      coordinationMode: "solo",
      personaId: null,
    };
    const initialContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      shouldShowTab: true,
    });
    const startedContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      status: "reviewing",
      reviewGateStatus: "reviewing",
      reviewConversationId: "review-conversation-1",
      shouldShowTab: true,
    });
    getWorkspaceReviewContextMock
      .mockResolvedValueOnce(initialContext)
      .mockResolvedValue(startedContext);
    startWorkspaceReviewMock.mockResolvedValue({ ...startedContext, started: true });
    const sessionStateBefore = useAgentSessionStore.getState();
    const runtimeStateBefore = {
      runtimeByConversationId: { ...sessionStateBefore.runtimeByConversationId },
      lastRuntimeByProjectId: { ...sessionStateBefore.lastRuntimeByProjectId },
      lastModelEffortByProvider: {
        ...sessionStateBefore.lastModelEffortByProvider,
      },
    };

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      { onFocusWorkspaceReview },
      queryClient,
    );

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Run review" }));

    await waitFor(() =>
      expect(onFocusWorkspaceReview).toHaveBeenCalledWith(
        "review-conversation-1",
        {
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "high",
        },
      ),
    );
    const sessionStateAfter = useAgentSessionStore.getState();
    expect({
      runtimeByConversationId: sessionStateAfter.runtimeByConversationId,
      lastRuntimeByProjectId: sessionStateAfter.lastRuntimeByProjectId,
      lastModelEffortByProvider: sessionStateAfter.lastModelEffortByProvider,
    }).toEqual(runtimeStateBefore);
  });

  it.each([
    {
      label: "already-reviewing review",
      result: {
        started: false,
        wasQueued: false,
        skippedReason: "already_reviewing",
      },
    },
    {
      label: "queued review",
      result: { started: true, wasQueued: true },
    },
  ])("focuses a $label without carrying a runtime hint", async ({ result }) => {
    const queryClient = createTestQueryClient();
    const onFocusWorkspaceReview = vi.fn();
    workspaceReviewRuntimeOverride.current = {
      provider: "codex",
      model: "gpt-5.5",
      effort: "high",
      serviceTier: "standard",
      coordinationMode: "solo",
      personaId: null,
    };
    const initialContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      shouldShowTab: true,
    });
    const reviewContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      reviewConversationId: "review-conversation-1",
      shouldShowTab: true,
    });
    getWorkspaceReviewContextMock
      .mockResolvedValueOnce(initialContext)
      .mockResolvedValue(reviewContext);
    startWorkspaceReviewMock.mockResolvedValue({
      ...reviewContext,
      ...result,
    });

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      { onFocusWorkspaceReview },
      queryClient,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Run review" }));

    await waitFor(() =>
      expect(onFocusWorkspaceReview).toHaveBeenCalledWith(
        "review-conversation-1",
      ),
    );
  });

  it("does not focus or carry a hint for a successful start without a child conversation", async () => {
    const queryClient = createTestQueryClient();
    const onFocusWorkspaceReview = vi.fn();
    workspaceReviewRuntimeOverride.current = {
      provider: "codex",
      model: "gpt-5.5",
      effort: "high",
      serviceTier: "standard",
      coordinationMode: "solo",
      personaId: null,
    };
    const childlessContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      reviewConversationId: null,
      shouldShowTab: true,
    });
    getWorkspaceReviewContextMock.mockResolvedValue(childlessContext);
    startWorkspaceReviewMock.mockResolvedValue({
      ...childlessContext,
      started: true,
      wasQueued: false,
    });

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      { onFocusWorkspaceReview },
      queryClient,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Run review" }));

    await waitFor(() => expect(startWorkspaceReviewMock).toHaveBeenCalled());
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();
  });

  it("does not focus or carry a hint when review start is rejected", async () => {
    const queryClient = createTestQueryClient();
    const onFocusWorkspaceReview = vi.fn();
    workspaceReviewRuntimeOverride.current = {
      provider: "codex",
      model: "gpt-5.5",
      effort: "high",
      serviceTier: "standard",
      coordinationMode: "solo",
      personaId: null,
    };
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );
    startWorkspaceReviewMock.mockRejectedValue(new Error("review conflict"));
    const runtimeBefore =
      useAgentSessionStore.getState().runtimeByConversationId[
        "review-conversation-1"
      ];

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      { onFocusWorkspaceReview },
      queryClient,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Run review" }));

    await waitFor(() => expect(startWorkspaceReviewMock).toHaveBeenCalled());
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();
    expect(
      useAgentSessionStore.getState().runtimeByConversationId[
        "review-conversation-1"
      ],
    ).toBe(runtimeBefore);
  });

  it("starts the workspace fixer from Fix Issues for a current blocking Review", async () => {
    const queryClient = createTestQueryClient();
    const initialContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      status: "ready",
      reviewOutcome: "blocking",
      reviewGateStatus: "blocking",
      reviewArtifactId: "review-artifact-1",
      reviewArtifactVersion: 2,
      reviewBlockingSummary: "Fix the failing review assertion.",
      isCurrent: true,
      shouldShowTab: true,
    });
    const startedContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      status: "ready",
      reviewOutcome: "blocking",
      reviewGateStatus: "blocking",
      reviewArtifactId: "review-artifact-1",
      reviewArtifactVersion: 2,
      reviewBlockingSummary: "Fix the failing review assertion.",
      reviewFixerStatus: "running",
      reviewFixerRunId: "fixer-run-1",
      reviewFixerConversationId: "conversation-1",
      isCurrent: true,
      shouldShowTab: true,
    });
    getWorkspaceReviewContextMock
      .mockResolvedValueOnce(initialContext)
      .mockResolvedValue(startedContext);
    startWorkspaceReviewFixerMock.mockResolvedValue(startedContext);
    const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      queryClient,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Fix Issues" }),
    );

    await waitFor(() =>
      expect(startWorkspaceReviewFixerMock).toHaveBeenCalledWith(
        "conversation-1",
        {
          confirmation: {
            targetScope: "selected_source",
            diffFingerprint: "fingerprint-351",
            artifactId: "review-artifact-1",
            artifactVersion: 2,
            blockingFingerprint: "blocking-fingerprint-1",
          },
          runtimeOverride: approvedPlanRuntime,
        },
      ),
    );
    await waitFor(() =>
      expect(
        queryClient.getQueryData(
          agentWorkspaceKeys.workspaceReview("conversation-1"),
        ),
      ).toEqual(startedContext),
    );
    expect(invalidateQueriesSpy).toHaveBeenCalledWith({
      queryKey: chatKeys.conversationTimeline("conversation-1"),
    });
  });

  it("approves the exact blocking Review snapshot and updates the shared cache", async () => {
    const user = userEvent.setup();
    const queryClient = createTestQueryClient();
    const initialContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      status: "ready",
      reviewOutcome: "blocking",
      reviewGateStatus: "blocking",
      reviewArtifactId: "review-artifact-1",
      reviewArtifactVersion: 2,
      reviewBlockingSummary: "A human must accept this blocker.",
      isCurrent: true,
      shouldShowTab: true,
    });
    const approvedContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      status: "ready",
      reviewOutcome: "blocking",
      reviewGateStatus: "passed",
      reviewArtifactId: "review-artifact-1",
      reviewArtifactVersion: 2,
      reviewBlockingSummary: "A human must accept this blocker.",
      reviewGateBypassedAt: "2026-07-10T00:05:00.000Z",
      reviewGateBypassedTargetScope: "selected_source",
      reviewGateBypassedDiffFingerprint: "fingerprint-351",
      reviewGateBypassedArtifactId: "review-artifact-1",
      reviewGateBypassedArtifactVersion: 2,
      isCurrent: true,
      shouldShowTab: true,
    });
    getWorkspaceReviewContextMock
      .mockResolvedValueOnce(initialContext)
      .mockResolvedValue(approvedContext);
    approveWorkspaceReviewAnywayMock.mockResolvedValue({
      success: true,
      monitor: approvedContext.monitor,
    });

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      queryClient,
    );

    await user.click(
      await screen.findByRole("button", { name: "Review actions" }),
    );
    await user.click(screen.getByTestId("agents-review-approve-anyway"));
    await user.click(screen.getByRole("button", { name: "Approve anyway" }));

    await waitFor(() =>
      expect(approveWorkspaceReviewAnywayMock).toHaveBeenCalledWith(
        "conversation-1",
        {
          targetScope: "selected_source",
          diffFingerprint: "fingerprint-351",
          artifactId: "review-artifact-1",
          artifactVersion: 2,
        },
      ),
    );
    await waitFor(() =>
      expect(
        queryClient.getQueryData<AgentWorkspaceReviewContext>(
          agentWorkspaceKeys.workspaceReview("conversation-1"),
        )?.monitor.reviewGateBypassedDiffFingerprint,
      ).toBe("fingerprint-351"),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Review approved anyway for the current changes",
    );
  });

  it("shows Fixing instead of Fix Issues while the review fixer is active", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewOutcome: "blocking",
        reviewGateStatus: "blocking",
        reviewArtifactId: "review-artifact-2",
        reviewArtifactVersion: 2,
        reviewBlockingSummary: "Fix active issues.",
        reviewFixerStatus: "queued",
        isCurrent: true,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByTestId("agents-review-fixing")).toHaveTextContent(
      "Fixing...",
    );
    expect(
      screen.queryByRole("button", { name: "Fix Issues" }),
    ).not.toBeInTheDocument();
    expect(startWorkspaceReviewFixerMock).not.toHaveBeenCalled();
  });

  it("does not show Fix Issues for outdated blocking Review findings", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewOutcome: "blocking",
        reviewGateStatus: "blocking",
        reviewArtifactId: "review-artifact-2",
        reviewArtifactVersion: 2,
        reviewBlockingSummary: "Stale blocker.",
        isCurrent: false,
        isOutdated: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockResolvedValue(workspaceReviewArtifact(2));

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    const updateReviewButton = await screen.findByRole("button", {
      name: "Update review",
    });
    expect(
      screen.queryByRole("button", { name: "Fix Issues" }),
    ).not.toBeInTheDocument();
    expect(updateReviewButton).toBeInTheDocument();
  });

  it("runs a forced update for an outdated Review artifact", async () => {
    const queryClient = createTestQueryClient();
    primeWorkspaceReviewArtifactPair(
      queryClient,
      { ...workspaceReviewArtifact(2), id: "review-artifact-1" },
      workspaceReviewRequestedChangesArtifact(2),
    );
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-1",
        reviewArtifactVersion: 2,
        reviewRequestedChangesArtifactId: "review-requested-changes-2",
        reviewRequestedChangesArtifactVersion: 2,
        isOutdated: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockImplementation(async (artifactId: string) =>
      artifactId === "review-requested-changes-2"
        ? workspaceReviewRequestedChangesArtifact(2)
        : {
            ...workspaceReviewArtifact(2),
            id: "review-artifact-1",
          },
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      queryClient,
    );

    expect(await screen.findByText("Review is outdated")).toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole(
        "button",
        { name: "Update review" },
        deferredHydrationTimeout,
      ),
    );

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: true,
      }),
    );
  });

  it("loads and renders both Workspace Review documents", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-2",
        reviewArtifactVersion: 2,
        reviewRequestedChangesArtifactId: "review-requested-changes-2",
        reviewRequestedChangesArtifactVersion: 2,
        reviewGateStatus: "blocking",
        reviewOutcome: "blocking",
        isCurrent: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockImplementation(async (artifactId: string) =>
      artifactId === "review-requested-changes-2"
        ? workspaceReviewRequestedChangesArtifact(2)
        : workspaceReviewArtifact(2),
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByRole("tab", { name: "Overview" }),
    ).toBeInTheDocument();
    const requestedChangesTab = screen.getByRole("tab", {
      name: "Requested Changes",
    });
    fireEvent.mouseDown(requestedChangesTab, { button: 0 });
    fireEvent.click(requestedChangesTab);

    expect(
      await screen.findByText("Implement the exact repair."),
    ).toBeInTheDocument();
    expect(getArtifactMock).toHaveBeenCalledWith("review-artifact-2");
    expect(getArtifactMock).toHaveBeenCalledWith(
      "review-requested-changes-2",
    );
  });

  it("disables the Review update action while a related runtime is generating", async () => {
    const queryClient = createTestQueryClient();
    primeWorkspaceReviewArtifactPair(
      queryClient,
      workspaceReviewArtifact(2),
      workspaceReviewRequestedChangesArtifact(2),
    );
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-2",
        reviewArtifactVersion: 2,
        reviewRequestedChangesArtifactId: "review-requested-changes-2",
        reviewRequestedChangesArtifactVersion: 2,
        isOutdated: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockImplementation(async (artifactId: string) =>
      artifactId === "review-requested-changes-2"
        ? workspaceReviewRequestedChangesArtifact(2)
        : workspaceReviewArtifact(2),
    );
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": conversationRuntimeStatus(),
    });

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      queryClient,
    );

    expect(await screen.findByText("Review is outdated")).toBeInTheDocument();

    const updateReviewButton = await screen.findByRole(
      "button",
      { name: "Update review" },
      deferredHydrationTimeout,
    );
    await waitFor(() => expect(updateReviewButton).toBeDisabled());
    expect(
      screen.queryByTestId("agents-review-action-disabled-reason"),
    ).not.toBeInTheDocument();
    expect(updateReviewButton).not.toHaveAttribute("aria-describedby");

    fireEvent.click(updateReviewButton);

    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();
  });

  it("does not mirror review-tab child runtime status into the visible workspace chat key", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-2",
        reviewArtifactVersion: 2,
        reviewRequestedChangesArtifactId: "review-requested-changes-2",
        reviewRequestedChangesArtifactVersion: 2,
        isOutdated: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockImplementation(async (artifactId: string) =>
      artifactId === "review-requested-changes-2"
        ? workspaceReviewRequestedChangesArtifact(2)
        : workspaceReviewArtifact(2),
    );
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": conversationRuntimeStatus({
        primarySource: "workspace_review",
        summaryLabel: "Reviewing",
        items: [
          {
            ...conversationRuntimeStatus().items[0]!,
            source: "workspace_review",
            contextType: "project",
            contextId: "review-conversation-1",
            label: "Reviewing",
            title: "Review workspace",
            conversationId: "review-conversation-1",
          },
        ],
      }),
    });

    const storeKey = buildStoreKey("project", "conversation-1");
    useChatStore.getState().setAgentStatus(storeKey, "generating");
    useChatStore.getState().setAgentActivityLabel(storeKey, "running");

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByText("Review is outdated")).toBeInTheDocument();
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Update review" }),
      ).toBeDisabled();
    });

    expect(useChatStore.getState().agentStatus[storeKey]).toBe("generating");
    expect(useChatStore.getState().agentActivityLabels[storeKey]).toBe(
      "running",
    );
  });

  it("keeps the Review update action enabled while a related runtime is waiting for input", async () => {
    const queryClient = createTestQueryClient();
    primeWorkspaceReviewArtifactPair(
      queryClient,
      workspaceReviewArtifact(2),
      workspaceReviewRequestedChangesArtifact(2),
    );
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-2",
        reviewArtifactVersion: 2,
        reviewRequestedChangesArtifactId: "review-requested-changes-2",
        reviewRequestedChangesArtifactVersion: 2,
        isOutdated: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockImplementation(async (artifactId: string) =>
      artifactId === "review-requested-changes-2"
        ? workspaceReviewRequestedChangesArtifact(2)
        : workspaceReviewArtifact(2),
    );
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": conversationRuntimeStatus({
        agentStatus: "waiting_for_input",
        items: [
          {
            ...conversationRuntimeStatus().items[0]!,
            agentStatus: "waiting_for_input",
            label: "Workspace waiting",
          },
        ],
      }),
    });

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      queryClient,
    );

    expect(await screen.findByText("Review is outdated")).toBeInTheDocument();

    const updateReviewButton = await screen.findByRole(
      "button",
      { name: "Update review" },
      deferredHydrationTimeout,
    );
    await waitFor(() => expect(updateReviewButton).toBeEnabled());
    fireEvent.click(updateReviewButton);

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: true,
      }),
    );
  });

  it("shows running Review state in the panel without repeating the tab title", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "reviewing",
        shouldShowTab: true,
      }),
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    const content = await screen.findByTestId("agents-publish-content-review");

    expect(await within(content).findByText("Reviewing")).toBeInTheDocument();
    expect(
      within(content).queryByRole("heading", { name: "Review" }),
    ).not.toBeInTheDocument();
    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();
  });

  it("ignores Review state returned for another conversation", async () => {
    const queryClient = createTestQueryClient();
    queryClient.setQueryData(
      agentWorkspaceKeys.workspaceReview("conversation-2"),
      workspaceReviewContext({
        conversationId: "conversation-1",
        target: workspaceReviewTarget,
        status: "reviewing",
        shouldShowTab: true,
      }),
    );

    renderPane(
      "publish",
      workspace({ conversationId: "conversation-2", mode: "edit" }),
      vi.fn(),
      false,
      { ...conversation(), id: "conversation-2" },
      {},
      queryClient,
    );

    expect(
      screen.queryByTestId("agents-artifact-tab-review"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Reviewing")).not.toBeInTheDocument();
  });

  it("offers a forced rerun for a current Review artifact without success toasts", async () => {
    const rerun = deferred<StartAgentWorkspaceReviewResult>();
    const queryClient = createTestQueryClient();
    primeWorkspaceReviewArtifactPair(
      queryClient,
      { ...workspaceReviewArtifact(2), id: "review-artifact-1" },
      workspaceReviewRequestedChangesArtifact(2),
    );
    startWorkspaceReviewMock.mockReturnValue(rerun.promise);
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-1",
        reviewArtifactVersion: 2,
        reviewRequestedChangesArtifactId: "review-requested-changes-2",
        reviewRequestedChangesArtifactVersion: 2,
        reviewGateStatus: "passed",
        isCurrent: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockImplementation(async (artifactId: string) =>
      artifactId === "review-requested-changes-2"
        ? workspaceReviewRequestedChangesArtifact(2)
        : {
            ...workspaceReviewArtifact(2),
            id: "review-artifact-1",
          },
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      queryClient,
    );

    expect(await screen.findByText("Review passed")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-review-open-publish"),
    ).not.toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole(
        "button",
        { name: "Run again" },
        deferredHydrationTimeout,
      ),
    );

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: true,
      }),
    );
    expect(screen.getByTestId("agents-publish-reviewing")).toHaveTextContent(
      "Reviewing",
    );
    expect(
      screen.queryByTestId("agents-publish-confirm"),
    ).not.toBeInTheDocument();
    expect(toastMessageMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("does not let stale completed start data override the current Review context", async () => {
    const queryClient = createTestQueryClient();
    primeWorkspaceReviewArtifactPair(
      queryClient,
      { ...workspaceReviewArtifact(2), id: "review-artifact-v2" },
      {
        ...workspaceReviewRequestedChangesArtifact(2),
        id: "review-requested-changes-v2",
      },
    );
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-v2",
        reviewArtifactVersion: 2,
        reviewRequestedChangesArtifactId: "review-requested-changes-v2",
        reviewRequestedChangesArtifactVersion: 2,
        reviewGateStatus: "passed",
        isCurrent: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockImplementation(async (artifactId: string) =>
      artifactId === "review-requested-changes-v2"
        ? {
            ...workspaceReviewRequestedChangesArtifact(2),
            id: "review-requested-changes-v2",
          }
        : {
            ...workspaceReviewArtifact(2),
            id: "review-artifact-v2",
            content: {
              type: "inline",
              text: "# Workspace Review\n\nCurrent v2 findings.",
            },
            metadata: {
              createdAt: "2026-04-23T09:35:00Z",
              createdBy: "ralphx-workspace-reviewer",
              version: 2,
            },
          },
    );
    startWorkspaceReviewMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "reviewing",
        reviewArtifactId: "review-artifact-v1",
        reviewArtifactVersion: 1,
        isOutdated: true,
        shouldShowTab: true,
      }),
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {},
      queryClient,
    );

    expect(await screen.findByText("Review passed")).toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole(
        "button",
        { name: "Run again" },
        deferredHydrationTimeout,
      ),
    );

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: true,
      }),
    );
    expect(screen.getByText("Review passed")).toBeInTheDocument();
    expect(screen.queryByText("Reviewing")).not.toBeInTheDocument();
    expect(screen.queryByText("Review is outdated")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/The Review below is still available/),
    ).not.toBeInTheDocument();
  });

  it("offers a forced retry when Review is blocked", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "blocked",
        lastError: "Reviewer child chat failed",
        shouldShowTab: true,
      }),
    );

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByText("Review failed")).toBeInTheDocument();
    expect(screen.getByText("Reviewer child chat failed")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry review" }));

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: true,
      }),
    );
  });

  it("keeps the artifact pane as a passive Review observer", async () => {
    vi.useFakeTimers();
    try {
      getWorkspaceReviewContextMock.mockResolvedValue(
        workspaceReviewContext({
          target: workspaceReviewTarget,
          status: "reviewing",
          shouldShowTab: true,
        }),
      );

      renderPane(
        "review",
        workspace({ mode: "edit" }),
        vi.fn(),
        false,
        conversation(),
      );

      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      await act(async () => {});

      expect(
        within(screen.getByTestId("agents-publish-content-review")).getByText(
          "Reviewing",
        ),
      ).toBeInTheDocument();
      expect(getWorkspaceReviewContextMock).toHaveBeenCalledTimes(1);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2_000);
      });

      expect(getWorkspaceReviewContextMock).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not fall back to publish for generic edit workspace pane opens", () => {
    renderPane("plan", workspace({ mode: "edit" }));

    expect(
      screen.getByTestId("agents-artifact-tab-publish"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("agents-publish-pane")).not.toBeInTheDocument();
  });

  it("shows pre-PR Auto Publish with independent PR automation controls", async () => {
    renderPane("publish", workspace({ mode: "edit" }));
    await openAutomationTab();

    expect(
      await screen.findByTestId("agents-auto-publish-switch"),
    ).not.toBeChecked();
    expect(screen.getByTestId("agents-pr-autofix-switch")).toBeEnabled();
    expect(screen.getByTestId("agents-pr-auto-merge-switch")).toBeEnabled();
  });

  it("renders the Review tab for Review PR workspaces without plan tabs", async () => {
    const onTabChange = vi.fn();
    getPrReviewContextMock.mockResolvedValue({
      success: true,
      workspace: workspace({ mode: "review_pr" }),
      events: [],
      prNumber: 78,
      prUrl: "https://github.com/mock/project/pull/78",
      currentHeadSha: "head-sha",
      pendingActionHeadStatus: "current",
      health: null,
      reviewFeedback: null,
      monitor: {
        conversationId: "conversation-1",
        projectId: "project-1",
        prNumber: 78,
        status: "watching",
        monitorEnabled: true,
        autoApproveEnabled: true,
        firstReviewCompleted: true,
        firstActionResolved: true,
        lastSeenHeadSha: "head-sha",
        lastReviewedHeadSha: "head-sha",
        lastReviewRunId: "run-1",
        lastReviewOutcome: "approved",
        lastSubmittedReviewId: null,
        reviewArtifactId: "review-artifact-1",
        reviewArtifactHeadSha: "head-sha",
        reviewArtifactVersion: 1,
        reviewArtifactUpdatedAt: "2026-04-23T09:30:00Z",
        lastError: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:30:00Z",
      },
      pendingAction: null,
      recentActions: [],
      issueCommentEvidence: [],
    });
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-1",
      type: "pr_review",
      name: "PR #78 Review",
      content: {
        type: "inline",
        text: "# PR Review\n\nNo blocking findings.",
      },
      metadata: {
        createdAt: "2026-04-23T09:30:00Z",
        createdBy: "ralphx-pr-reviewer",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    renderPane(
      "review",
      workspace({ mode: "review_pr" }),
      vi.fn(),
      false,
      {
        ...conversation(),
        agentMode: "review_pr",
      },
      { onTabChange },
    );

    const tabRow = screen.getByTestId("agents-artifact-tab-row");
    await screen.findByTestId("agents-artifact-tab-review");

    expect(artifactTabIds(tabRow)).toContain("agents-artifact-tab-review");
    expect(
      screen.queryByTestId("agents-artifact-tab-plan"),
    ).not.toBeInTheDocument();
    expect(await screen.findByText("PR Review")).toBeInTheDocument();
    expect(getPrReviewContextMock).toHaveBeenCalledWith("conversation-1");
    expect(getWorkspaceReviewContextMock).not.toHaveBeenCalled();
    expect(getArtifactMock).toHaveBeenCalledWith("review-artifact-1");

    onTabChange.mockClear();
    fireEvent.click(screen.getByTestId("agents-artifact-tab-review"));
    expect(onTabChange).not.toHaveBeenCalledWith("publish");
  });

  it("does not leak stale Workspace Review actions into Review PR mode", async () => {
    const queryClient = createTestQueryClient();
    const startedContext = workspaceReviewContext({
      target: workspaceReviewTarget,
      status: "ready",
      reviewOutcome: "blocking",
      reviewGateStatus: "blocking",
      reviewArtifactId: "review-artifact-2",
      reviewArtifactVersion: 2,
      reviewBlockingSummary: "Workspace review blocker.",
      isCurrent: true,
      shouldShowTab: true,
    });
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );
    startWorkspaceReviewMock.mockResolvedValue(startedContext);
    getPrReviewContextMock.mockResolvedValue(
      prReviewContext("conversation-1", "review-artifact-1"),
    );
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-1",
      type: "pr_review",
      name: "PR #78 Review",
      content: { type: "inline", text: "# PR Review" },
      metadata: {
        createdAt: "2026-04-23T09:30:00Z",
        createdBy: "ralphx-pr-reviewer",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    const pane = (mode: "edit" | "review_pr") => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={0}>
          <div className="h-[480px]">
            <AgentsArtifactPane
              conversation={conversation({
                agentMode: mode,
              })}
              workspace={workspace({ mode })}
              activeTab="review"
              taskMode="graph"
              onTabChange={() => {}}
              onTaskModeChange={() => {}}
              onPublishWorkspace={vi.fn()}
              isPublishingWorkspace={false}
              onClose={() => {}}
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    );

    const { rerender } = render(pane("edit"));

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Run review" }));
    await waitFor(() => expect(startWorkspaceReviewMock).toHaveBeenCalled());

    rerender(pane("review_pr"));

    expect(await screen.findByText("PR Review")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Fix Issues" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Update review" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Run review" }),
    ).not.toBeInTheDocument();
    expect(startWorkspaceReviewFixerMock).not.toHaveBeenCalled();
  });

  it("persists the Review PR Auto Approve switch through the authoritative API", async () => {
    const user = userEvent.setup();
    const context = prReviewContext("conversation-1", "review-artifact-1");
    getPrReviewContextMock.mockResolvedValue(context);
    setPrReviewAutoApproveMock.mockResolvedValue({
      success: true,
      monitor: {
        ...context.monitor!,
        autoApproveEnabled: false,
      },
    });
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-1",
      type: "pr_review",
      name: "PR #78 Review",
      content: { type: "inline", text: "# PR Review" },
      metadata: {
        createdAt: "2026-04-23T09:30:00Z",
        createdBy: "ralphx-pr-reviewer",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    renderPane("review", workspace({ mode: "review_pr" }), vi.fn(), false, {
      ...conversation(),
      agentMode: "review_pr",
    });

    const toggle = await screen.findByRole("switch", { name: "Auto Approve" });
    expect(toggle).toBeChecked();
    await user.click(toggle);

    await waitFor(() =>
      expect(setPrReviewAutoApproveMock).toHaveBeenCalledWith(
        "conversation-1",
        false,
      ),
    );
    expect(toggle).not.toBeChecked();
  });

  it("pauses and restarts Review PR monitoring from the Review tab", async () => {
    const user = userEvent.setup();
    const context = prReviewContext("conversation-1", "review-artifact-1");
    const pausedContext = {
      ...context,
      monitor: {
        ...context.monitor!,
        monitorEnabled: false,
        status: "paused" as const,
      },
    };
    getPrReviewContextMock
      .mockResolvedValueOnce(context)
      .mockResolvedValue(pausedContext);
    setPrReviewMonitoringMock
      .mockResolvedValueOnce({
        success: true,
        monitor: pausedContext.monitor!,
      })
      .mockResolvedValueOnce({ success: true, monitor: context.monitor! });
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-1",
      type: "pr_review",
      name: "PR #78 Review",
      content: { type: "inline", text: "# PR Review" },
      metadata: {
        createdAt: "2026-04-23T09:30:00Z",
        createdBy: "ralphx-pr-reviewer",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    renderPane("review", workspace({ mode: "review_pr" }), vi.fn(), false, {
      ...conversation(),
      agentMode: "review_pr",
    });

    await user.click(
      await screen.findByRole("button", { name: "Stop Monitoring" }),
    );

    await waitFor(() =>
      expect(setPrReviewMonitoringMock).toHaveBeenCalledWith(
        "conversation-1",
        false,
      ),
    );
    await user.click(
      await screen.findByRole("button", { name: "Restart Monitoring" }),
    );
    await waitFor(() =>
      expect(setPrReviewMonitoringMock).toHaveBeenLastCalledWith(
        "conversation-1",
        true,
      ),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "New-head PR reviews paused; lifecycle monitoring continues",
    );
  });

  it("asks whether to finish or cancel an active PR review before stopping", async () => {
    const user = userEvent.setup();
    const context = prReviewContext("conversation-1", "review-artifact-1");
    context.monitor = { ...context.monitor!, status: "reviewing" };
    getPrReviewContextMock.mockResolvedValue(context);
    setPrReviewMonitoringMock.mockResolvedValue({
      success: true,
      monitor: {
        ...context.monitor!,
        monitorEnabled: false,
        status: "paused",
      },
    });
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-1",
      type: "pr_review",
      name: "PR #78 Review",
      content: { type: "inline", text: "# PR Review" },
      metadata: {
        createdAt: "2026-04-23T09:30:00Z",
        createdBy: "ralphx-pr-reviewer",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    renderPane("review", workspace({ mode: "review_pr" }), vi.fn(), false, {
      ...conversation(),
      agentMode: "review_pr",
    });

    await user.click(
      await screen.findByRole("button", { name: "Stop Monitoring" }),
    );
    expect(
      await screen.findByRole("heading", {
        name: "Stop PR review monitoring?",
      }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Stop After Review" }));

    await waitFor(() =>
      expect(setPrReviewMonitoringMock).toHaveBeenCalledWith(
        "conversation-1",
        false,
        "finish_current",
      ),
    );
  });

  it("restores the Review PR Auto Approve preference when saving fails", async () => {
    const user = userEvent.setup();
    const context = prReviewContext("conversation-1", "review-artifact-1");
    getPrReviewContextMock.mockResolvedValue(context);
    setPrReviewAutoApproveMock.mockRejectedValue(
      new Error("Auto Approve is unavailable"),
    );
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-1",
      type: "pr_review",
      name: "PR #78 Review",
      content: { type: "inline", text: "# PR Review" },
      metadata: {
        createdAt: "2026-04-23T09:30:00Z",
        createdBy: "ralphx-pr-reviewer",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    renderPane("review", workspace({ mode: "review_pr" }), vi.fn(), false, {
      ...conversation(),
      agentMode: "review_pr",
    });

    const toggle = await screen.findByRole("switch", { name: "Auto Approve" });
    await user.click(toggle);

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "Auto Approve is unavailable",
      ),
    );
    expect(toggle).toBeChecked();
  });

  it("drops placeholder PR review context when switching conversations", async () => {
    const queryClient = createTestQueryClient();
    queryClient.setDefaultOptions({
      queries: {
        retry: false,
        placeholderData: (previousData: unknown) => previousData,
      },
      mutations: { retry: false },
    });
    getPrReviewContextMock.mockResolvedValueOnce(
      prReviewContext("conversation-1", "review-artifact-1"),
    );
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-1",
      type: "pr_review",
      name: "PR #78 Review",
      content: {
        type: "inline",
        text: "# PR Review\n\nNo blocking findings.",
      },
      metadata: {
        createdAt: "2026-04-23T09:30:00Z",
        createdBy: "ralphx-pr-reviewer",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    const pane = (conversationId: string) => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={0}>
          <div className="h-[480px]">
            <AgentsArtifactPane
              conversation={conversation({
                id: conversationId,
                agentMode: "review_pr",
              })}
              workspace={workspace({ conversationId, mode: "review_pr" })}
              activeTab="review"
              taskMode="graph"
              onTabChange={() => {}}
              onTaskModeChange={() => {}}
              onPublishWorkspace={vi.fn()}
              isPublishingWorkspace={false}
              onClose={() => {}}
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    );

    const { rerender } = render(pane("conversation-1"));

    expect(
      await screen.findByTestId("agents-artifact-tab-review"),
    ).toBeInTheDocument();
    expect(await screen.findByText("PR Review")).toBeInTheDocument();

    getPrReviewContextMock.mockReturnValue(
      deferred<AgentWorkspacePrReviewContext>().promise,
    );
    rerender(pane("conversation-2"));

    expect(
      screen.queryByTestId("agents-artifact-tab-review"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("PR Review")).not.toBeInTheDocument();
  });

  it("persists pre-PR autofix preference while initial Auto Publish is off", async () => {
    renderPane("publish", workspace({ mode: "edit" }));
    await openAutomationTab();

    expect(
      await screen.findByTestId("agents-auto-publish-switch"),
    ).not.toBeChecked();

    fireEvent.click(screen.getByTestId("agents-pr-autofix-switch"));

    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: false,
          autoMergeMethod: "squash",
        },
      ),
    );
  });

  it("persists pre-PR auto-merge preference while initial Auto Publish is off", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        prAutofixEnabled: true,
      }),
    );
    await openAutomationTab();

    expect(
      await screen.findByTestId("agents-auto-publish-switch"),
    ).not.toBeChecked();

    fireEvent.click(screen.getByTestId("agents-pr-auto-merge-switch"));

    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: true,
          autoMergeMethod: "squash",
        },
      ),
    );
  });

  it("confirms enabling pre-PR Auto Publish from the publish pane", async () => {
    setWorkspaceAutoPublishMock.mockImplementationOnce(
      async (conversationId: string, input: { autoPublishEnabled: boolean }) =>
        workspace({
          mode: "edit",
          conversationId,
          autoPublishInitialPrEnabled: input.autoPublishEnabled,
        }),
    );
    renderPane("publish", workspace({ mode: "edit" }));
    await openAutomationTab();

    fireEvent.click(await screen.findByTestId("agents-auto-publish-switch"));

    expect(setWorkspaceAutoPublishMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Enable Auto Publish",
      }),
    );

    await waitFor(() =>
      expect(setWorkspaceAutoPublishMock).toHaveBeenCalledWith(
        "conversation-1",
        {
          autoPublishEnabled: true,
        },
      ),
    );
  });

  it("persists PR supervision switches from the publish pane", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
      }),
    );
    await openAutomationTab();

    fireEvent.click(await screen.findByTestId("agents-pr-autofix-switch"));

    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: false,
          autoMergeMethod: "squash",
        },
      ),
    );
  });

  it("keeps the GitHub auto-merge switch off after a successful disable settles", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
        prAutofixEnabled: true,
        prAutoMergeDesired: true,
        prAutoMergeCurrent: true,
        prSupervisionStatus: "monitoring",
      }),
    );
    await openAutomationTab();

    const autoMergeSwitch = await screen.findByRole("switch", {
      name: "GitHub auto-merge",
    });
    expect(autoMergeSwitch).toBeChecked();

    fireEvent.click(autoMergeSwitch);

    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: false,
          autoMergeMethod: "squash",
        },
      ),
    );
    await waitFor(() =>
      expect(
        screen.queryByText("Saving PR supervision"),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("switch", { name: "GitHub auto-merge" }),
    ).not.toBeChecked();
  });

  it("clears the PR supervision result override when refreshed workspace data matches it", async () => {
    const initialWorkspace = publishedPrSupervisionWorkspace();
    const settledWorkspace = publishedPrSupervisionWorkspace({
      prAutoMergeDesired: false,
      prAutoMergeCurrent: false,
      prSupervisionStatus: "waiting_for_checks",
    });
    setWorkspacePrSupervisionMock.mockResolvedValueOnce(settledWorkspace);
    const { rerenderWorkspace } =
      renderPublishPanelForWorkspaceRerender(initialWorkspace);

    fireEvent.click(
      await screen.findByRole("switch", { name: "GitHub auto-merge" }),
    );

    await waitFor(() =>
      expect(
        screen.getByTestId("agents-pr-supervision-status"),
      ).toHaveTextContent("Waiting for checks"),
    );
    expect(
      screen.getByRole("switch", { name: "GitHub auto-merge" }),
    ).not.toBeChecked();

    rerenderWorkspace(settledWorkspace);
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "GitHub auto-merge" }),
      ).not.toBeChecked(),
    );

    rerenderWorkspace(initialWorkspace);
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "GitHub auto-merge" }),
      ).toBeChecked(),
    );
    expect(
      screen.getByTestId("agents-pr-supervision-status"),
    ).toHaveTextContent("Monitoring PR");
  });

  it("clears the PR supervision result override when refreshed workspace data advances", async () => {
    const initialWorkspace = publishedPrSupervisionWorkspace();
    const settledWorkspace = publishedPrSupervisionWorkspace({
      prAutoMergeDesired: false,
      prAutoMergeCurrent: false,
      prSupervisionStatus: "waiting_for_checks",
    });
    setWorkspacePrSupervisionMock.mockResolvedValueOnce(settledWorkspace);
    const { rerenderWorkspace } =
      renderPublishPanelForWorkspaceRerender(initialWorkspace);

    fireEvent.click(
      await screen.findByRole("switch", { name: "GitHub auto-merge" }),
    );

    await waitFor(() =>
      expect(
        screen.getByTestId("agents-pr-supervision-status"),
      ).toHaveTextContent("Waiting for checks"),
    );
    expect(
      screen.getByRole("switch", { name: "GitHub auto-merge" }),
    ).not.toBeChecked();

    rerenderWorkspace(
      publishedPrSupervisionWorkspace({
        updatedAt: "2026-04-23T09:01:00Z",
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "GitHub auto-merge" }),
      ).toBeChecked(),
    );
    expect(
      screen.getByTestId("agents-pr-supervision-status"),
    ).toHaveTextContent("Monitoring PR");
  });

  it("clears the PR supervision result override when switching workspaces", async () => {
    const initialWorkspace = publishedPrSupervisionWorkspace();
    const settledWorkspace = publishedPrSupervisionWorkspace({
      prAutoMergeDesired: false,
      prAutoMergeCurrent: false,
      prSupervisionStatus: "waiting_for_checks",
    });
    setWorkspacePrSupervisionMock.mockResolvedValueOnce(settledWorkspace);
    const { rerenderWorkspace } =
      renderPublishPanelForWorkspaceRerender(initialWorkspace);

    fireEvent.click(
      await screen.findByRole("switch", { name: "GitHub auto-merge" }),
    );

    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "GitHub auto-merge" }),
      ).not.toBeChecked(),
    );

    rerenderWorkspace(
      publishedPrSupervisionWorkspace({
        conversationId: "conversation-2",
        publicationPrNumber: 91,
        publicationPrUrl: "https://github.com/mock/project/pull/91",
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "GitHub auto-merge" }),
      ).toBeChecked(),
    );
    expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
      "conversation-1",
      {
        autoFixEnabled: true,
        autoMergeDesired: false,
        autoMergeMethod: "squash",
      },
    );
  });

  it("surfaces object-shaped backend reasons when PR supervision updates fail", async () => {
    setWorkspacePrSupervisionMock.mockRejectedValueOnce({
      message: "Branch is checked out by an active merge worktree",
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
        prAutofixEnabled: true,
        prAutoMergeDesired: true,
        prSupervisionStatus: "monitoring",
      }),
    );
    await openAutomationTab();

    fireEvent.click(
      await screen.findByRole("switch", { name: "GitHub auto-merge" }),
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "Branch is checked out by an active merge worktree",
      ),
    );
  });

  it("surfaces string backend reasons when PR supervision updates fail", async () => {
    setWorkspacePrSupervisionMock.mockRejectedValueOnce(
      "GitHub auto-merge disable could not be confirmed",
    );

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
        prAutofixEnabled: true,
        prAutoMergeDesired: true,
        prSupervisionStatus: "monitoring",
      }),
    );
    await openAutomationTab();

    fireEvent.click(
      await screen.findByRole("switch", { name: "GitHub auto-merge" }),
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "GitHub auto-merge disable could not be confirmed",
      ),
    );
  });

  it("opens Workspace settings from PR automation tooltip actions", async () => {
    const user = userEvent.setup();
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
      }),
    );
    await openAutomationTab();

    await user.hover(
      await screen.findByRole("button", {
        name: "About Autofix CI and Reviews",
      }),
    );
    const settingsActions = await screen.findAllByTestId(
      "agents-tooltip-settings-workspace",
    );
    await user.click(settingsActions[0]);

    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({
      section: "workspace",
    });
  });

  it("confirms pausing Auto Publish from the publish pane", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
        autoPublishEnabled: true,
        prAutofixEnabled: true,
      }),
    );
    await openAutomationTab();

    fireEvent.click(await screen.findByTestId("agents-auto-publish-switch"));

    expect(setWorkspaceAutoPublishMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Pause Auto Publish",
      }),
    );

    await waitFor(() =>
      expect(setWorkspaceAutoPublishMock).toHaveBeenCalledWith(
        "conversation-1",
        {
          autoPublishEnabled: false,
        },
      ),
    );
  });

  it("disables PR automation switches while Auto Publish is paused", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
        autoPublishEnabled: false,
        prSupervisionStatus: "paused",
      }),
    );
    await openAutomationTab();

    expect(await screen.findByText("Auto Publish paused")).toBeInTheDocument();
    expect(screen.getByTestId("agents-auto-publish-switch")).not.toBeChecked();
    expect(screen.getByTestId("agents-pr-autofix-switch")).toBeDisabled();
    expect(screen.getByTestId("agents-pr-auto-merge-switch")).toBeDisabled();
  });

  it("surfaces PR conflicts and routes Resolve Conflicts through base update", async () => {
    const user = userEvent.setup();
    const conflictingWorkspace = workspace({
      mode: "edit",
      publicationPrNumber: 2857,
      publicationPrUrl: "https://github.com/mock/project/pull/2857",
      publicationPrStatus: "open",
      publicationPushStatus: "pushed",
      autoPublishEnabled: true,
      prSupervisionStatus: "blocked",
      prSupervisionSummary:
        "PR #2857 has merge conflicts. GitHub reports the pull request is conflicting.",
    });
    updateWorkspaceFromBaseMock.mockResolvedValue({
      workspace: conflictingWorkspace,
      updated: true,
      targetRef: "origin/main",
      baseCommit: "base-sha",
    });

    renderPane("publish", conflictingWorkspace);

    expect(await screen.findByTestId("agents-pr-conflict")).toHaveTextContent(
      "PR #2857 has merge conflicts",
    );
    expect(screen.getByText(/Auto Publish is waiting/i)).toBeInTheDocument();
    expect(screen.getByText("PR conflicts")).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-workspace-sync-status"),
    ).toHaveTextContent("Conflicting");
    await user.click(screen.getByRole("button", { name: "Resolve conflicts" }));
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Resolve conflicts",
      }),
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
  });

  it("surfaces paused PR conflicts without Auto Publish waiting copy", async () => {
    const conflictingWorkspace = workspace({
      mode: "edit",
      publicationPrNumber: 2857,
      publicationPrUrl: "https://github.com/mock/project/pull/2857",
      publicationPrStatus: "open",
      publicationPushStatus: "pushed",
      autoPublishEnabled: false,
      prSupervisionStatus: "blocked",
      prSupervisionSummary:
        "PR #2857 has merge conflicts. GitHub reports the pull request is conflicting.",
    });

    renderPane("publish", conflictingWorkspace);

    expect(await screen.findByTestId("agents-pr-conflict")).toHaveTextContent(
      "PR #2857 has merge conflicts",
    );
    expect(
      screen.getByText(
        "This pull request has conflicts. Resolve conflicts to update the branch from base before publishing can continue.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Resolve conflicts" }),
    ).toBeEnabled();
  });

  it("surfaces git auth repair actions in the publish pane", () => {
    useGitAuthDiagnosticsMock.mockReturnValue({
      data: {
        fetchUrl: "https://github.com/mock/project.git",
        pushUrl: "git@github.com:mock/project.git",
        fetchKind: "HTTPS",
        pushKind: "SSH",
        mixedAuthModes: true,
        githubHttpsCredentialHelperConfigured: false,
        canSwitchToSsh: true,
        suggestedSshUrl: "git@github.com:mock/project.git",
      },
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });

    renderPane("publish", workspace({ mode: "edit" }));

    expect(screen.getByTestId("git-auth-repair-panel")).toBeInTheDocument();
    expect(
      screen.getByText(/Fetch and push use different auth modes/i),
    ).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-switch-ssh")).toBeInTheDocument();
  });

  it("shows a GitHub PR sign-in action for all-SSH publish workspaces when gh is missing", () => {
    useGhAuthStatusMock.mockReturnValue({
      data: {
        state: "unauthenticated",
        diagnostic: "missing_credentials",
        ghInstalled: true,
        authenticated: false,
        host: "github.com",
        account: null,
      },
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

    expect(
      screen.getByTestId("agents-artifact-tab-publish"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(screen.getByText("PR #90")).toBeInTheDocument();
  });

  it("allows Commit & Publish for linked pipeline-owned ideation PRs", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
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
    await openAutomationTab();

    const publishButton = screen.getByTestId("agents-publish-confirm");
    expect(publishButton).toHaveTextContent("Commit & Publish");
    expect(publishButton).toBeEnabled();
    expect(
      screen.getByRole("switch", { name: "Autofix CI & Reviews" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "GitHub auto-merge" }),
    ).toBeInTheDocument();

    await user.click(publishButton);
    expect(publish).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog", {
      name: "Commit and publish workspace?",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "Commit & Publish" }),
    );

    await waitFor(() => expect(publish).toHaveBeenCalledWith("conversation-1"));
  });

  it("allows PR maintenance actions for pipeline-owned ideation workspaces", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      freshnessScope: "full",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "old-base",
      targetBaseCommit: "new-base",
      isBaseAhead: true,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
      remoteRefreshed: true,
      worktreeStatusChecked: true,
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

    expect(
      await screen.findByTestId(
        "agents-base-stale",
        {},
        deferredHydrationTimeout,
      ),
    ).toHaveTextContent("feature/agent-screen");
    expect(screen.queryByTestId("agents-close-pr")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-actions-menu")).toBeEnabled();
    expect(screen.getByTestId("agents-update-from-base")).toBeEnabled();
    expect(
      screen.queryByTestId("agents-publish-confirm"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      }),
    );
    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );

    await userEvent.click(screen.getByTestId("agents-publish-actions-menu"));
    await userEvent.click(await screen.findByTestId("agents-close-pr"));
    expect(closeWorkspacePrMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Close PR",
      }),
    );
    await waitFor(() =>
      expect(closeWorkspacePrMock).toHaveBeenCalledWith("conversation-1"),
    );
    expect(publish).not.toHaveBeenCalled();
  });

  it("allows Update from base for pre-PR pipeline-owned ideation workspaces", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      freshnessScope: "full",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "old-base",
      targetBaseCommit: "new-base",
      isBaseAhead: true,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
      remoteRefreshed: true,
      worktreeStatusChecked: true,
    });
    updateWorkspaceFromBaseMock.mockResolvedValue({
      workspace: workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
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
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
      }),
      publish,
    );

    expect(await screen.findByTestId("agents-update-from-base")).toBeEnabled();
    expect(
      screen.queryByTestId("agents-publish-confirm"),
    ).not.toBeInTheDocument();
    expect(getWorkspaceFreshnessMock).toHaveBeenCalledWith("conversation-1", {
      scope: "full",
    });

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      }),
    );
    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    expect(publish).not.toHaveBeenCalled();
  });

  it("renders the publish pane shell before hydrating git-backed publish facts", async () => {
    renderPane("publish", workspace({ mode: "edit" }));

    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(screen.getByText("Loading changed files...")).toBeInTheDocument();
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
    expect(getWorkspaceChangesMock).not.toHaveBeenCalled();
    expect(getWorkspaceFreshnessMock).not.toHaveBeenCalled();
    expect(listPublicationEventsMock).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(getWorkspaceFreshnessMock).toHaveBeenCalledWith("conversation-1", {
        scope: "full",
      }),
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

    await waitFor(() =>
      expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"),
    );
    expect(useDependencyGraphMock).toHaveBeenCalledWith("");
    expect(useVerificationStatusMock).toHaveBeenCalledWith(
      undefined,
      "conversation-1",
    );
  });

  it("hydrates a Plan workspace from a plan artifact tool result when the workspace link is stale", async () => {
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
                name: "mcp__ralphx__create_plan_artifact",
                arguments: { session_id: "session-1" },
                result: {
                  session_id: "session-1",
                  artifact_id: "artifact-1",
                },
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
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "draft",
      },
    });

    renderPane(
      "plan",
      workspace({ mode: "plan", linkedIdeationSessionId: null }),
      vi.fn(),
      false,
      conversation(),
    );

    await waitFor(() =>
      expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"),
    );
    await waitFor(() =>
      expect(getSessionPlanMock).toHaveBeenCalledWith("session-1"),
    );
    expect(screen.queryByText("No plan yet")).not.toBeInTheDocument();
  });

  it("opens the start composer with the selected plan reference from the Plan overflow menu", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({
      focusedProjectId: "project-1",
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
      startConversationDraft: null,
    });
    useChatStore
      .getState()
      .setActiveConversation("project:project-1", "conversation-1");
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
                name: "mcp__ralphx__create_plan_artifact",
                arguments: { session_id: "session-1" },
                result: {
                  session_id: "session-1",
                  artifact_id: "artifact-1",
                },
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
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 2,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 2,
        approvedAt: "2026-04-23T09:05:00Z",
      },
    });

    renderPane(
      "plan",
      workspace({ mode: "plan", linkedIdeationSessionId: null }),
      vi.fn(),
      false,
      conversation(),
    );

    await waitFor(() =>
      expect(getSessionPlanMock).toHaveBeenCalledWith("session-1"),
    );
    await user.click(await screen.findByLabelText("Plan actions"));
    await user.click(
      screen.getByRole("menuitem", { name: /new conversation/i }),
    );

    expect(useAgentSessionStore.getState().startConversationDraft).toEqual({
      projectId: "project-1",
      content: "",
      mode: "edit",
      composerArtifactReferences: [
        {
          kind: "plan",
          artifactId: "artifact-1",
          title: "Implementation Plan",
          sessionId: "session-1",
          version: 2,
          status: "approved",
        },
      ],
    });
    expect(useAgentSessionStore.getState().focusedProjectId).toBe("project-1");
    expect(useAgentSessionStore.getState().selectedConversationId).toBeNull();
    expect(
      useChatStore.getState().activeConversationIds["project:project-1"],
    ).toBeNull();
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
    expect(approvePlanArtifactMock).not.toHaveBeenCalled();
    expect(confirmVerificationMock).not.toHaveBeenCalled();
  });

  it("fetches the current planning-session plan even when session data has a stale null plan id", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: null,
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "draft",
      },
    });

    renderPane(
      "plan",
      workspace({ mode: "plan", linkedIdeationSessionId: "session-1" }),
      vi.fn(),
      false,
      conversation(),
    );

    await waitFor(() =>
      expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"),
    );
    await waitFor(() =>
      expect(getSessionPlanMock).toHaveBeenCalledWith("session-1"),
    );
    expect(screen.queryByText("No plan yet")).not.toBeInTheDocument();
  });

  it("activates a durable Tasks pipeline before requesting proposals", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2020-01-01T00:00:00Z",
      },
    });

    const onConversationModeSwitched = vi.fn();
    const onFocusIdeationSessionForConversation = vi.fn();

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
      {
        onConversationModeSwitched,
        onFocusIdeationSessionForConversation,
      },
    );

    const planContent = await screen.findByTestId(
      "agents-artifact-content-plan",
    );
    const createProposalsButton = await within(planContent).findByRole(
      "button",
      {
        name: /Create Proposals/i,
      },
    );
    activateAgentTaskPipelineMock.mockClear();
    sendAgentMessageMock.mockClear();

    await userEvent.click(createProposalsButton);

    await waitFor(() =>
      expect(activateAgentTaskPipelineMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        sessionId: "session-1",
        runtimeOverride: approvedPlanRuntime,
      }),
    );
    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledWith(
        "ideation",
        "session-1",
        expect.stringContaining("Create implementation task proposals"),
        undefined,
        { runtimeOverride: approvedPlanRuntime },
      ),
    );
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "tasks",
      expect.objectContaining({
        linkedIdeationSessionId: "session-1",
        taskPipelineSessionId: "session-1",
        mode: "tasks",
      }),
    );
    expect(onFocusIdeationSessionForConversation).toHaveBeenCalledWith(
      "conversation-1",
      "session-1",
    );
    expect(
      useChatStore.getState().activeConversationIds["session:session-1"],
    ).toBe("ideation-conversation-1");
    expect(sendAgentMessageMock.mock.invocationCallOrder[0]!).toBeGreaterThan(
      activateAgentTaskPipelineMock.mock.invocationCallOrder[0]!,
    );
  });

  it("retries proposal decomposition from an attached Tasks pipeline", async () => {
    getIdeationSessionMock.mockResolvedValue(ideationSessionResponse());
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());
    activateAgentTaskPipelineMock.mockClear();
    sendAgentMessageMock.mockClear();

    renderPane(
      "plan",
      workspace({
        mode: "tasks",
        linkedIdeationSessionId: "session-1",
        taskPipelineSessionId: "session-1",
        taskPipelineAvailable: true,
      }),
      vi.fn(),
      false,
      conversation({ agentMode: "tasks" }),
    );

    const createProposalsButton = await screen.findByRole("button", {
      name: /Create Proposals/i,
    });
    await userEvent.click(createProposalsButton);

    expect(activateAgentTaskPipelineMock).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledWith(
        "ideation",
        "session-1",
        expect.stringContaining("Create implementation task proposals"),
        undefined,
        undefined,
      ),
    );
  });

  it("omits empty Proposals and Verification tabs for a plan session without evidence", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:30:00Z",
      },
    });

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId("agents-artifact-tab-plan"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-proposal"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-verification"),
    ).not.toBeInTheDocument();
  });

  it("connects Overview and Blueprint tabs to their visible document panels", async () => {
    const user = userEvent.setup();
    getIdeationSessionMock.mockResolvedValue(ideationSessionResponse());
    getSessionPlanMock.mockResolvedValue(approvedPlanBundleArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    const overviewTab = await screen.findByRole("tab", { name: "Overview" });
    const overviewPanel = document.getElementById(
      overviewTab.getAttribute("aria-controls")!,
    );
    expect(overviewPanel).toHaveAttribute("role", "tabpanel");
    expect(overviewPanel).toBeVisible();
    expect(within(overviewPanel!).getByText("Do the work.")).toBeVisible();

    const blueprintTab = screen.getByRole("tab", { name: "Blueprint" });
    await user.click(blueprintTab);
    const blueprintPanel = document.getElementById(
      blueprintTab.getAttribute("aria-controls")!,
    );
    expect(blueprintPanel).toHaveAttribute("role", "tabpanel");
    expect(blueprintPanel).toBeVisible();
    expect(
      within(blueprintPanel!).getByText("Follow these detailed steps."),
    ).toBeVisible();
  });

  it("loads the complete bundle for an attached ordinary ideation session", async () => {
    const user = userEvent.setup();
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({ sessionFlow: "ideation" }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanBundleArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await waitFor(() =>
      expect(getSessionPlanMock).toHaveBeenCalledWith("session-1"),
    );
    expect(getArtifactMock).not.toHaveBeenCalled();

    await user.click(screen.getByRole("tab", { name: "Blueprint" }));
    expect(
      await screen.findByText("Follow these detailed steps."),
    ).toBeVisible();
  });

  it("keeps proposal content inside Plan when a stale Proposals tab is active", async () => {
    const user = userEvent.setup();
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [
        {
          id: "proposal-1",
          sessionId: "session-1",
          title: "Gate embedded proposal access",
          description: "Show proposals inside the Plan tab.",
          category: "frontend",
          steps: ["Update shared tab helper"],
          acceptanceCriteria: ["Proposal content stays embedded in Plan"],
          suggestedPriority: "high",
          priorityScore: 90,
          priorityReason: "Avoids dead-end navigation",
          estimatedComplexity: "simple",
          userPriority: null,
          userModified: false,
          status: "pending",
          createdTaskId: null,
          planArtifactId: "artifact-1",
          planVersionAtCreation: 1,
          sortOrder: 0,
          createdAt: "2026-04-23T09:15:00Z",
          updatedAt: "2026-04-23T09:15:00Z",
        },
      ],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:30:00Z",
      },
    });

    renderPane(
      "proposal" as AgentArtifactTab,
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId("agents-artifact-tab-plan"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-proposal"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-verification"),
    ).not.toBeInTheDocument();
    const proposalsToggle = await screen.findByRole("tab", {
      name: /Proposals \(1\)/i,
    });

    await user.click(proposalsToggle);

    expect(
      await screen.findByText("Gate embedded proposal access"),
    ).toBeInTheDocument();
    await user.click(screen.getByTestId("proposal-card-proposal-1"));
    expect(
      await screen.findByTestId("proposal-detail-sheet"),
    ).toBeInTheDocument();
    await user.click(screen.getByTestId("close-sheet-button"));
    await waitFor(() =>
      expect(
        screen.queryByTestId("proposal-detail-sheet"),
      ).not.toBeInTheDocument(),
    );
    expect(useDependencyGraphMock).toHaveBeenLastCalledWith("session-1");
  });

  it("opens linked proposal cards from the active Plan tab without a standalone Proposals tab", async () => {
    const user = userEvent.setup();
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({}, [
        taskProposal({
          id: "proposal-1",
          title: "Gate embedded proposal access",
          description: "Show proposals inside the Plan tab.",
          acceptanceCriteria: ["Proposal content stays embedded in Plan"],
          status: "pending",
          createdTaskId: null,
        }),
      ]),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId("agents-artifact-tab-plan"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-proposal"),
    ).not.toBeInTheDocument();

    const planDisplay = await screen.findByTestId("plan-display-chromeless");
    const proposalsToggle = within(planDisplay).getByRole("tab", {
      name: /Proposals \(1\)/i,
    });

    expect(
      screen.queryByText("Gate embedded proposal access"),
    ).not.toBeInTheDocument();

    await user.click(proposalsToggle);

    expect(
      screen.queryByTestId("agents-artifact-tab-proposal"),
    ).not.toBeInTheDocument();
    const proposalsPanel = document.getElementById(
      proposalsToggle.getAttribute("aria-controls")!,
    );
    expect(proposalsPanel).toHaveAttribute("role", "tabpanel");
    expect(proposalsPanel).toBeVisible();
    expect(
      await within(proposalsPanel!).findByText("Gate embedded proposal access"),
    ).toBeInTheDocument();
    expect(useDependencyGraphMock).toHaveBeenLastCalledWith("session-1");
  });

  it("opens the Plan export action from the artifact overflow menu", async () => {
    const user = userEvent.setup();
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "active",
        acceptanceStatus: null,
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanBundleArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await user.click(
      await screen.findByRole("button", { name: /Plan actions/i }),
    );
    await user.click(await screen.findByRole("menuitem", { name: /Export/i }));
    expect(await screen.findByText("Export Plan")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Download overview" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Download blueprint" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Download complete bundle" }),
    ).toBeEnabled();
  });

  it("opens the Plan editor from the artifact overflow menu", async () => {
    const user = userEvent.setup();
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "active",
        acceptanceStatus: null,
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await user.click(
      await screen.findByRole("button", { name: /Plan actions/i }),
    );
    await user.click(await screen.findByRole("menuitem", { name: /Edit/i }));
    expect(await screen.findByRole("textbox")).toBeInTheDocument();
  });

  it("shows active execution-plan progress in the Plan banner and Tasks tab badge", async () => {
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-current",
          title: "Current task",
          internalStatus: "executing",
          executionPlanId: "exec-current",
        }),
        task({
          id: "task-old",
          title: "Old task",
          executionPlanId: "exec-old",
        }),
        task({
          id: "task-archived",
          title: "Archived task",
          executionPlanId: "exec-current",
          archivedAt: "2026-04-24T09:00:00Z",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse(
        {
          status: "accepted",
          acceptanceStatus: "accepted",
          convertedAt: "2026-04-23T10:00:00Z",
        },
        [taskProposal({ createdTaskId: "task-old" })],
      ),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId("plan-lifecycle-banner"),
    ).toHaveTextContent("1 task");
    expect(screen.getByText("1 in progress")).toBeInTheDocument();
    expect(
      await screen.findByTestId("agents-artifact-tab-tasks"),
    ).toHaveTextContent("1");
  });

  it("falls back to proposal-created tasks when active execution plan is unavailable", async () => {
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: {},
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-created",
          title: "Created from proposal",
          internalStatus: "executing",
          executionPlanId: "exec-created",
        }),
        task({
          id: "task-other",
          title: "Unrelated task",
          executionPlanId: "exec-other",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse(
        {
          status: "accepted",
          acceptanceStatus: "accepted",
          convertedAt: "2026-04-23T10:00:00Z",
        },
        [taskProposal({ createdTaskId: "task-created" })],
      ),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId("plan-lifecycle-banner"),
    ).toHaveTextContent("1 task");
    expect(
      await screen.findByTestId("agents-artifact-tab-tasks"),
    ).toHaveTextContent("1");
  });

  it("uses attached-session proposal tasks when the project active execution plan is stale", async () => {
    usePlanStore.setState({
      activeExecutionPlanIdByProject: { "project-1": "exec-other" },
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-current",
          title: "Current session task",
          internalStatus: "executing",
          executionPlanId: "exec-current",
          ideationSessionId: "session-1",
        }),
        task({
          id: "task-other",
          title: "Unrelated active-plan task",
          internalStatus: "blocked",
          executionPlanId: "exec-other",
          ideationSessionId: "session-other",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse(
        {
          status: "accepted",
          acceptanceStatus: "accepted",
          convertedAt: "2026-04-23T10:00:00Z",
        },
        [taskProposal({ createdTaskId: "task-current" })],
      ),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId("plan-lifecycle-banner"),
    ).toHaveTextContent("1 task");
    expect(screen.getByText("1 in progress")).toBeInTheDocument();
    expect(screen.queryByText("1 blocked")).not.toBeInTheDocument();
    expect(
      await screen.findByTestId("agents-artifact-tab-tasks"),
    ).toHaveTextContent("1");
  });

  it("does not offer restart from a stale project active execution plan alone", async () => {
    usePlanStore.setState({
      activeExecutionPlanIdByProject: { "project-1": "exec-other" },
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-other",
          title: "Unrelated task",
          executionPlanId: "exec-other",
          ideationSessionId: "session-other",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await screen.findByTestId("plan-display-chromeless");
    expect(screen.getByTestId("plan-lifecycle-banner")).toHaveTextContent(
      "This approved plan is guiding the current workspace agent.",
    );
    expect(
      screen.queryByTestId("restart-implementation-button"),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("view-work-button")).not.toBeInTheDocument();
  });

  it("hides work UI for stale accepted fields without attached implementation tasks", async () => {
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "active",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByText("Implementation Plan")).toBeInTheDocument();
    expect(
      screen.queryByTestId("accepted-session-banner"),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("view-work-button")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-tasks"),
    ).not.toBeInTheDocument();
  });

  it("keeps durable session history visible when the active execution plan is foreign", async () => {
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-other" },
      activeExecutionPlanIdByProject: { "project-1": "exec-foreign" },
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-foreign",
          title: "Foreign active task",
          executionPlanId: "exec-foreign",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByText("Implementation Plan")).toBeInTheDocument();
    expect(
      screen.queryByTestId("accepted-session-banner"),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("view-work-button")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-artifact-tab-tasks")).toBeInTheDocument();
  });

  it("opens the Tasks tab from the accepted Plan progress banner", async () => {
    const user = userEvent.setup();
    const onTabChange = vi.fn();
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-current",
          executionPlanId: "exec-current",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
      { onTabChange },
    );

    await user.click(await screen.findByTestId("view-work-button"));

    expect(onTabChange).toHaveBeenCalledWith("tasks");
  });

  it("confirms before restarting accepted implementation work", async () => {
    const user = userEvent.setup();
    const loadActivePlan = vi.fn().mockResolvedValue(undefined);
    const restartResult = deferred<{
      sessionId: string;
      oldExecutionPlanId: string;
      executionPlanId: string;
      archivedTaskCount: number;
      createdTaskIds: string[];
    }>();
    restartImplementationMock.mockReturnValueOnce(restartResult.promise);
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
      loadActivePlan,
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-current",
          executionPlanId: "exec-current",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await user.click(
      await screen.findByTestId("restart-implementation-button"),
    );
    let dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Restart implementation?");
    expect(dialog).toHaveTextContent("The accepted plan will remain unchanged");
    expect(dialog).toHaveTextContent(
      "current implementation attempt, Kanban tasks, and uncommitted implementation changes will be discarded",
    );
    expect(dialog).toHaveTextContent(
      "reset the branch to the latest fetched base",
    );

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(restartImplementationMock).not.toHaveBeenCalled();

    await user.click(
      await screen.findByTestId("restart-implementation-button"),
    );
    dialog = await screen.findByRole("alertdialog");
    await user.click(
      within(dialog).getByRole("button", { name: "Restart Implementation" }),
    );

    await waitFor(() =>
      expect(restartImplementationMock).toHaveBeenCalledWith("session-1"),
    );
    const pendingButton = within(dialog).getByRole("button", {
      name: "Restarting…",
    });
    expect(pendingButton).toBeDisabled();
    await user.click(pendingButton);
    expect(restartImplementationMock).toHaveBeenCalledTimes(1);

    restartResult.resolve({
      sessionId: "session-1",
      oldExecutionPlanId: "exec-old",
      executionPlanId: "exec-new",
      archivedTaskCount: 1,
      createdTaskIds: ["task-new"],
    });

    await waitFor(() =>
      expect(loadActivePlan).toHaveBeenCalledWith("project-1"),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Implementation restarted with 1 task",
    );
  });

  it("confirms before pausing and stopping the accepted execution plan", async () => {
    const user = userEvent.setup();
    const loadActivePlan = vi.fn().mockResolvedValue(undefined);
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
      loadActivePlan,
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-current",
          executionPlanId: "exec-current",
          internalStatus: "executing",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId("plan-lifecycle-pause-button"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("plan-lifecycle-stop-button"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("plan-lifecycle-resume-button"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("plan-lifecycle-pause-button"));
    let dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Pause this implementation plan?");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(pauseExecutionPlanMock).not.toHaveBeenCalled();

    await user.click(screen.getByTestId("plan-lifecycle-pause-button"));
    dialog = await screen.findByRole("alertdialog");
    await user.click(
      within(dialog).getByRole("button", { name: "Pause Plan" }),
    );

    await waitFor(() =>
      expect(pauseExecutionPlanMock).toHaveBeenCalledWith({
        projectId: "project-1",
        sessionId: "session-1",
        executionPlanId: "exec-current",
      }),
    );
    expect(loadActivePlan).toHaveBeenCalledWith("project-1");
    expect(toastSuccessMock).toHaveBeenCalledWith("Plan paused");

    await user.click(screen.getByTestId("plan-lifecycle-stop-button"));
    dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Stop this implementation plan?");
    await user.click(within(dialog).getByRole("button", { name: "Stop Plan" }));

    await waitFor(() =>
      expect(stopExecutionPlanMock).toHaveBeenCalledWith({
        projectId: "project-1",
        sessionId: "session-1",
        executionPlanId: "exec-current",
      }),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Plan stopped");
  });

  it("shows a confirmed resume control for paused accepted execution plan work", async () => {
    const user = userEvent.setup();
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-current",
          executionPlanId: "exec-current",
          internalStatus: "paused",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId("plan-lifecycle-resume-button"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("plan-lifecycle-pause-button"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("plan-lifecycle-stop-button"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("plan-lifecycle-resume-button"));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Resume this implementation plan?");
    await user.click(
      within(dialog).getByRole("button", { name: "Resume Plan" }),
    );

    await waitFor(() =>
      expect(resumeExecutionPlanMock).toHaveBeenCalledWith({
        projectId: "project-1",
        sessionId: "session-1",
        executionPlanId: "exec-current",
      }),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Plan resumed");
  });

  it("reports restart implementation failures from the confirmation action", async () => {
    const user = userEvent.setup();
    restartImplementationMock.mockRejectedValueOnce(
      new Error("Restart failed"),
    );
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-current",
          executionPlanId: "exec-current",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await user.click(
      await screen.findByTestId("restart-implementation-button"),
    );
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Restart Implementation",
      }),
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith("Restart failed"),
    );
  });

  it("reports string restart implementation failures from the confirmation action", async () => {
    const user = userEvent.setup();
    restartImplementationMock.mockRejectedValueOnce(
      "RalphX could not safely restore this implementation workspace because the linked branch is checked out elsewhere or no longer matches the plan",
    );
    usePlanStore.setState({
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-current",
          executionPlanId: "exec-current",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await user.click(
      await screen.findByTestId("restart-implementation-button"),
    );
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Restart Implementation",
      }),
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "RalphX could not safely restore this implementation workspace because the linked branch is checked out elsewhere or no longer matches the plan",
      ),
    );
    expect(toastErrorMock.mock.calls[0]?.[0]).not.toContain("/Users/");
  });

  it("falls back to the Plan tab when Proposals is active but no proposals exist", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:30:00Z",
      },
    });

    renderPane(
      "proposal" as AgentArtifactTab,
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    const planTab = await screen.findByTestId("agents-artifact-tab-plan");

    expect(
      screen.queryByTestId("agents-artifact-tab-proposal"),
    ).not.toBeInTheDocument();
    expect(
      planTab.querySelector("span[style='background: var(--accent-primary);']"),
    ).not.toBeNull();
    expect(useDependencyGraphMock).toHaveBeenLastCalledWith("");
  });

  it("shows a warning lifecycle banner for a draft plan with stale accepted fields", async () => {
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "active",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(draftPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    const banner = await screen.findByTestId("plan-lifecycle-banner");

    expect(banner).toHaveAttribute("data-lifecycle-state", "needs_approval");
    expect(banner.style.getPropertyValue("--plan-lifecycle-accent")).toBe(
      "var(--status-warning)",
    );
    expect(within(banner).getByText("Plan needs approval")).toBeInTheDocument();
    expect(
      within(banner).getByRole("button", { name: /Approve Plan/i }),
    ).toBeInTheDocument();
    expect(
      within(banner).getByRole("button", { name: /Verify Plan/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("accepted-session-banner"),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("view-work-button")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-tasks"),
    ).not.toBeInTheDocument();

    const planDisplay = await screen.findByTestId("plan-display-chromeless");
    expect(
      within(planDisplay).queryByTestId("plan-approve-button"),
    ).not.toBeInTheDocument();
    expect(
      within(planDisplay).queryByTestId("plan-verify-button"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Select plan lines" }),
    ).not.toBeInTheDocument();

    const planBody = screen.getByText("Do the work.");
    expect(
      planBody.closest("[data-artifact-selectable-region]"),
    ).not.toBeNull();
    expect(banner.closest("[data-artifact-selectable-region]")).toBeNull();
    expect(
      within(banner)
        .getByRole("button", { name: /Approve Plan/i })
        .closest("[data-artifact-selectable-region]"),
    ).toBeNull();
  });

  it("does not show the draft approval lifecycle banner outside Plan mode", async () => {
    getIdeationSessionMock.mockResolvedValue(ideationSessionResponse());
    getSessionPlanMock.mockResolvedValue(draftPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "edit",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await screen.findByTestId("plan-display-chromeless");

    expect(
      screen.queryByTestId("plan-lifecycle-banner"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Plan needs approval")).not.toBeInTheDocument();
    expect(approvePlanArtifactMock).not.toHaveBeenCalled();
  });

  it("shows an info lifecycle banner for an approved plan without work", async () => {
    getIdeationSessionMock.mockResolvedValue(ideationSessionResponse());
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());
    getPlanComplexityAssessmentMock.mockResolvedValue({
      id: "assessment-1",
      sessionId: "session-1",
      artifactId: "artifact-1",
      artifactVersion: 1,
      level: "complex",
      score: 82,
      recommendedAction: "create_proposals",
      confidence: 0.88,
      reasonSummary:
        "Multiple dependent work items need tracked review checkpoints.",
      signals: { dependency_count: 4 },
      assessedBy: "ralphx-utility-plan-complexity",
      createdAt: "2026-04-23T09:31:00Z",
      updatedAt: "2026-04-23T09:31:00Z",
    });

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    const banner = await screen.findByTestId("plan-lifecycle-banner");

    expect(banner).toHaveAttribute("data-lifecycle-state", "approved");
    expect(banner.style.getPropertyValue("--plan-lifecycle-accent")).toBe(
      "var(--status-info)",
    );
    expect(within(banner).getByText("Plan approved")).toBeInTheDocument();
    await waitFor(() =>
      expect(
        within(banner).getByText(/Recommended: Create Proposals/i),
      ).toBeInTheDocument(),
    );
    expect(
      within(banner).getByRole("button", { name: /Create Proposals/i }),
    ).toBeInTheDocument();
    expect(
      within(banner).getByRole("button", { name: /Implement Directly/i }),
    ).toBeInTheDocument();
    expect(
      within(banner).getByRole("button", { name: /Verify Plan/i }),
    ).toBeInTheDocument();
    expect(within(banner).queryByText(/\d+ tasks?/i)).not.toBeInTheDocument();
    expect(screen.queryByTestId("view-work-button")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-tasks"),
    ).not.toBeInTheDocument();

    const planDisplay = screen.getByTestId("plan-display-chromeless");
    expect(
      within(planDisplay).queryByTestId("plan-verify-button"),
    ).not.toBeInTheDocument();
    expect(
      within(planDisplay).queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();
    expect(
      within(planDisplay).queryByRole("button", {
        name: /Implement Directly/i,
      }),
    ).not.toBeInTheDocument();
  });

  it("keeps an approved plan direct-only while Tasks are off", async () => {
    tasksEnabledRef.current = false;
    getIdeationSessionMock.mockResolvedValue(ideationSessionResponse());
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    const banner = await screen.findByTestId("plan-lifecycle-banner");
    expect(
      within(banner).getByRole("button", { name: /Implement Directly/i }),
    ).toBeEnabled();
    expect(
      within(banner).queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();
    expect(within(banner).getByText(/Tasks is off/i)).toBeInTheDocument();
    expect(getPlanComplexityAssessmentMock).not.toHaveBeenCalled();
  });

  it("starts the complete reviewed proposal set from Tasks mode", async () => {
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({}, [
        taskProposal({
          id: "proposal-1",
          status: "pending",
          createdTaskId: null,
        }),
      ]),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());
    startAgentTaskPipelineMock.mockClear();

    renderPane(
      "plan",
      workspace({
        mode: "tasks",
        linkedIdeationSessionId: "session-1",
        taskPipelineSessionId: "session-1",
        taskPipelineAvailable: true,
      }),
      vi.fn(),
      false,
      conversation({ agentMode: "tasks" }),
    );

    const startTasks = await screen.findByRole("button", {
      name: /Start Tasks \(1\)/i,
    });
    await userEvent.click(startTasks);

    await waitFor(() =>
      expect(startAgentTaskPipelineMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        sessionId: "session-1",
        proposalIds: ["proposal-1"],
      }),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Started 1 task");
  });

  it("uses the success lifecycle banner only when accepted work exists", async () => {
    usePlanStore.setState({
      activePlanByProject: { "project-1": "session-1" },
      activeExecutionPlanIdByProject: { "project-1": "exec-current" },
    });
    useTasksMock.mockReturnValue({
      data: [
        task({
          id: "task-active",
          executionPlanId: "exec-current",
          internalStatus: "executing",
        }),
        task({
          id: "task-done",
          executionPlanId: "exec-current",
          internalStatus: "merged",
        }),
      ],
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({
        status: "accepted",
        acceptanceStatus: "accepted",
        convertedAt: "2026-04-23T10:00:00Z",
      }),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    const banner = await screen.findByTestId("plan-lifecycle-banner");

    expect(banner).toHaveAttribute("data-lifecycle-state", "accepted");
    expect(banner.style.getPropertyValue("--plan-lifecycle-accent")).toBe(
      "var(--status-success)",
    );
    expect(within(banner).getByText("Plan accepted")).toBeInTheDocument();
    expect(within(banner).getByText("2 tasks")).toBeInTheDocument();
    expect(within(banner).getByText("1 in progress")).toBeInTheDocument();
    expect(within(banner).getByText("1 completed")).toBeInTheDocument();
    expect(
      within(banner).getByRole("button", { name: /View Work/i }),
    ).toBeInTheDocument();
    expect(within(banner).queryByText("Plan approved")).not.toBeInTheDocument();
  });

  it("leaves only the Plan proposals tab in the plan document row", async () => {
    getIdeationSessionMock.mockResolvedValue(
      ideationSessionResponse({}, [
        taskProposal({
          id: "proposal-1",
          title: "Keep proposals in Plan",
          status: "pending",
          createdTaskId: null,
        }),
      ]),
    );
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    const banner = await screen.findByTestId("plan-lifecycle-banner");
    const planDisplay = screen.getByTestId("plan-display-chromeless");

    expect(
      within(banner).getByRole("button", { name: /Implement Directly/i }),
    ).toBeInTheDocument();
    expect(
      within(banner).getByRole("button", { name: /Verify Plan/i }),
    ).toBeInTheDocument();
    expect(
      within(planDisplay).getByTestId("plan-proposals-tab"),
    ).toBeInTheDocument();
    expect(
      within(planDisplay).queryByTestId("plan-approve-button"),
    ).not.toBeInTheDocument();
    expect(
      within(planDisplay).queryByTestId("plan-verify-button"),
    ).not.toBeInTheDocument();
    expect(
      within(planDisplay).queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();
    expect(
      within(planDisplay).queryByRole("button", {
        name: /Implement Directly/i,
      }),
    ).not.toBeInTheDocument();
  });

  it("shows plan complexity guidance while still allowing direct implementation", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:30:00Z",
      },
    });
    getPlanComplexityAssessmentMock.mockResolvedValue({
      id: "assessment-1",
      sessionId: "session-1",
      artifactId: "artifact-1",
      artifactVersion: 1,
      level: "complex",
      score: 82,
      recommendedAction: "create_proposals",
      confidence: 0.88,
      reasonSummary:
        "Multiple dependent work items need tracked review checkpoints.",
      signals: { dependency_count: 4 },
      assessedBy: "ralphx-utility-plan-complexity",
      createdAt: "2026-04-23T09:31:00Z",
      updatedAt: "2026-04-23T09:31:00Z",
    });
    const onConversationModeSwitched = vi.fn();

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
      { onConversationModeSwitched },
    );

    expect(
      await screen.findByText(/Recommended: Create Proposals/i),
    ).toHaveTextContent("Both paths remain available");

    await userEvent.click(
      screen.getByRole("button", { name: /Implement Directly/i }),
    );

    await waitFor(() =>
      expect(activateAgentPlanDirectImplementationMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        sessionId: "session-1",
        retry: false,
      }),
    );
    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledWith(
        "project",
        "project-1",
        expect.stringContaining("Implement the approved plan directly"),
        undefined,
        {
          conversationId: "conversation-1",
          runtimeOverride: approvedPlanRuntime,
          composerArtifactReferences: [
            expect.objectContaining({ artifactId: "artifact-1" }),
            expect.objectContaining({ artifactId: "blueprint-1" }),
          ],
          suppressUserMessage: true,
        },
      ),
    );
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(sendAgentMessageMock.mock.calls[0]?.[2]).not.toContain(
      "do not create task proposals",
    );
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "edit",
      expect.objectContaining({ mode: "edit" }),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Implementation started");
  });

  it("shows and disables Plan tab CTAs while the recommendation check is running", async () => {
    const assessment = deferred<null>();
    useVerificationStatusMock.mockReturnValue({
      data: { status: "verifying", inProgress: true },
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: new Date().toISOString(),
      },
    });
    getPlanComplexityAssessmentMock.mockReturnValue(assessment.promise);

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(
      await screen.findByText(/Checking recommended next action/i),
    ).toBeInTheDocument();

    const implementButton = screen.getByRole("button", {
      name: /Implement Directly/i,
    });
    const createButton = screen.getByRole("button", {
      name: /Create Proposals/i,
    });
    const verifyButton = screen.getByRole("button", { name: /Verifying/i });

    expect(implementButton).toBeDisabled();
    expect(createButton).toBeDisabled();
    expect(verifyButton).toBeDisabled();

    await userEvent.click(implementButton);
    await userEvent.click(createButton);
    await userEvent.click(verifyButton);

    expect(sendAgentMessageMock).not.toHaveBeenCalled();
    expect(confirmVerificationMock).not.toHaveBeenCalled();

    assessment.resolve(null);
  });

  it("approves a draft Plan-mode artifact without requesting proposals", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    const draftPlan = {
      ...approvedPlanBundleArtifact(),
      planApproval: {
        status: "draft",
      },
    };
    getSessionPlanMock.mockResolvedValue(draftPlan);
    approvePlanArtifactMock.mockResolvedValue({
      ...draftPlan,
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:30:00Z",
      },
    });

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await userEvent.click(
      await screen.findByRole("button", { name: /Approve Plan/i }),
    );

    await waitFor(() =>
      expect(approvePlanArtifactMock).toHaveBeenCalledWith({
        sessionId: "session-1",
        artifactId: "artifact-1",
        blueprintArtifactId: "blueprint-1",
        blueprintArtifactVersion: 2,
      }),
    );
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
  });

  it("keeps Approve Plan visible for automation-run plan conversations", async () => {
    const draftPlan = {
      ...approvedPlanArtifact(),
      planApproval: { status: "draft" as const },
    };
    getAutomationMock.mockResolvedValue(
      automationDetailFixture({
        runs: [
          automationRunFixture({
            status: "awaiting_plan_approval",
            prNumber: null,
            prUrl: null,
            planArtifactId: "artifact-1",
          }),
        ],
      }),
    );
    getIdeationSessionMock.mockResolvedValue(ideationSessionResponse());
    getSessionPlanMock.mockResolvedValue(draftPlan);
    approvePlanArtifactMock.mockResolvedValue({
      ...draftPlan,
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:30:00Z",
      },
    });

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      {
        ...conversation(),
        automationId: "automation-1",
        automationRunId: "run-1",
      },
    );

    expect(
      await screen.findByText(
        "RalphX continues this run automatically after approval.",
      ),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: /Approve Plan/i }),
    );

    await waitFor(() =>
      expect(approvePlanArtifactMock).toHaveBeenCalledWith({
        sessionId: "session-1",
        artifactId: "artifact-1",
      }),
    );
  });

  it("suppresses manual continuation actions for automation-run plan conversations", async () => {
    const user = userEvent.setup();
    getAutomationMock.mockResolvedValue(
      automationDetailFixture({
        runs: [
          automationRunFixture({
            status: "awaiting_plan_approval",
            prNumber: null,
            prUrl: null,
            planArtifactId: "artifact-1",
          }),
        ],
      }),
    );
    getIdeationSessionMock.mockResolvedValue(ideationSessionResponse());
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());
    getPlanComplexityAssessmentMock.mockResolvedValue({
      id: "assessment-1",
      sessionId: "session-1",
      artifactId: "artifact-1",
      artifactVersion: 1,
      level: "straightforward",
      score: 20,
      recommendedAction: "implement_directly",
      confidence: 0.9,
      reasonSummary: "Single scoped change.",
      signals: {},
      assessedBy: "ralphx-utility-plan-complexity",
      createdAt: "2026-04-23T09:31:00Z",
      updatedAt: "2026-04-23T09:31:00Z",
    });

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      {
        ...conversation(),
        automationId: "automation-1",
        automationRunId: "run-1",
      },
    );

    expect(
      await screen.findByText(
        "RalphX continues this run automatically after approval.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Implement Directly/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByLabelText("Plan actions"));
    expect(
      screen.queryByRole("menuitem", { name: /New Conversation/i }),
    ).not.toBeInTheDocument();
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
  });

  it("starts verification for a draft Plan-mode artifact beside approval", async () => {
    const onTabChange = vi.fn();
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "draft",
      },
    });
    getVerificationSpecialistsMock.mockResolvedValue({
      specialists: [
        {
          name: "security-review",
          display_name: "Security Review",
          description: null,
          enabled_by_default: false,
        },
        {
          name: "implementation-feasibility",
          display_name: "Implementation Feasibility",
          description: null,
          enabled_by_default: true,
        },
      ],
    });

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
      { onTabChange },
    );

    expect(
      await screen.findByRole("button", { name: /Approve Plan/i }),
    ).toBeInTheDocument();

    await userEvent.click(
      await screen.findByRole("button", { name: /Verify Plan/i }),
    );

    await waitFor(() =>
      expect(confirmVerificationMock).toHaveBeenCalledWith("session-1"),
    );
    expect(onTabChange).not.toHaveBeenCalledWith("verification");
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Verify Plan queued in this conversation",
    );
    expect(approvePlanArtifactMock).not.toHaveBeenCalled();
  });

  it("starts verification for an approved Plan-mode artifact", async () => {
    const onTabChange = vi.fn();
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:30:00Z",
      },
    });
    getVerificationSpecialistsMock.mockResolvedValue({
      specialists: [
        {
          name: "security-review",
          display_name: "Security Review",
          description: null,
          enabled_by_default: false,
        },
        {
          name: "implementation-feasibility",
          display_name: "Implementation Feasibility",
          description: null,
          enabled_by_default: true,
        },
      ],
    });

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
      { onTabChange },
    );

    await userEvent.click(
      await screen.findByRole("button", { name: /Verify Plan/i }),
    );

    await waitFor(() =>
      expect(confirmVerificationMock).toHaveBeenCalledWith("session-1"),
    );
    expect(onTabChange).not.toHaveBeenCalledWith("verification");
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Verify Plan queued in this conversation",
    );
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
  });

  it("keeps a verified Plan banner control and confirms a manual rerun", async () => {
    useVerificationStatusMock.mockReturnValue({
      data: { status: "verified", inProgress: false },
      isLoading: false,
      isFetching: false,
    });
    getIdeationSessionMock.mockResolvedValue(ideationSessionResponse());
    getSessionPlanMock.mockResolvedValue(approvedPlanArtifact());

    renderPane(
      "plan",
      workspace({ mode: "plan", linkedIdeationSessionId: "session-1" }),
      vi.fn(),
      false,
      conversation(),
    );

    await userEvent.click(
      await screen.findByRole("button", { name: "Verified" }),
    );

    expect(screen.getByText("Verify this plan again?")).toBeInTheDocument();
    expect(confirmVerificationMock).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: "Verify again" }));
    await waitFor(() =>
      expect(confirmVerificationMock).toHaveBeenCalledWith("session-1"),
    );
  });

  it("hides right-side approved plan CTAs when the workspace has changes", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:30:00Z",
      },
    });

    renderPane(
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
      {
        activeWorkspaceFreshness: workspaceFreshness({
          hasUncommittedChanges: true,
        }),
      },
    );

    await screen.findByTestId("plan-display-chromeless");
    expect(screen.getByTestId("plan-lifecycle-banner")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Verify Plan/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Implement Directly/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();
  });

  it("hides Plan-mode action buttons after the workspace switches to direct implementation", async () => {
    getIdeationSessionMock.mockResolvedValue({
      session: {
        id: "session-1",
        projectId: "project-1",
        title: "Planning session",
        titleSource: "auto",
        status: "active",
        planArtifactId: "artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: {
        type: "inline",
        text: "# Implementation Plan\n\nDo the work.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:30:00Z",
      },
    });

    renderPane(
      "plan",
      workspace({
        mode: "edit",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    await screen.findByTestId("plan-display-chromeless");
    expect(screen.getByTestId("plan-lifecycle-banner")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Verify Plan/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Implement Directly/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();
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
      expect(getIdeationSessionMock).toHaveBeenCalledWith("session-focused"),
    );
    expect(getIdeationSessionMock).not.toHaveBeenCalledWith(
      "session-from-workspace",
    );
    expect(useConversationMock).toHaveBeenCalledWith("conversation-1", {
      enabled: false,
      pageSize: 40,
    });
  });

  it("does not revive the retired verification surface from a stale tab key", () => {
    const onFocusVerificationSession = vi.fn();
    renderPane(
      "verification",
      workspace({ mode: "ideation", linkedIdeationSessionId: "session-1" }),
      vi.fn(),
      false,
      conversation(),
      { onFocusVerificationSession },
    );

    expect(useVerificationStatusMock).toHaveBeenCalledWith(
      undefined,
      "conversation-1",
    );
    expect(getIdeationChildrenMock).not.toHaveBeenCalledWith(
      "session-1",
      "verification",
    );
    expect(onFocusVerificationSession).not.toHaveBeenCalled();
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

    await waitFor(() =>
      expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"),
    );
    expect(
      screen.queryByTestId("agents-artifact-tab-plan"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-verification"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-proposal"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-artifact-tab-tasks"),
    ).not.toBeInTheDocument();
  });

  it("confirms publish from the publish pane", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 42,
        publicationPrUrl: "https://github.com/acme/project/pull/42",
        publicationPrStatus: "open",
      }),
      publish,
      false,
      conversation(),
    );

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));

    expect(publish).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Commit & Publish",
      }),
    );

    await waitFor(() => expect(publish).toHaveBeenCalledWith("conversation-1"));
  });

  it("blocks publish while PR supervision preferences are saving", async () => {
    const user = userEvent.setup();
    const supervisionDeferred = deferred<AgentConversationWorkspace>();
    setWorkspacePrSupervisionMock.mockReturnValueOnce(
      supervisionDeferred.promise,
    );
    const publish = vi.fn().mockResolvedValue(undefined);

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 42,
        publicationPrUrl: "https://github.com/acme/project/pull/42",
        publicationPrStatus: "open",
      }),
      publish,
      false,
      conversation(),
    );
    await openAutomationTab();

    await user.click(screen.getByRole("switch", { name: "GitHub auto-merge" }));

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-confirm")).toBeDisabled(),
    );
    expect(screen.getByText("Saving PR supervision")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(publish).not.toHaveBeenCalled();

    await act(async () => {
      supervisionDeferred.resolve(
        workspace({
          mode: "edit",
          publicationPrNumber: 42,
          publicationPrUrl: "https://github.com/acme/project/pull/42",
          publicationPrStatus: "open",
          prAutoMergeDesired: true,
          prAutoMergeMethod: "squash",
          prSupervisionStatus: "monitoring",
        }),
      );
    });
  });

  it("cancels the publish confirmation without starting publish", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      publish,
      false,
      conversation(),
    );

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));
    const dialog = await screen.findByRole("dialog", {
      name: "Commit and publish workspace?",
    });
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Cancel",
      }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Commit and publish workspace?" }),
      ).not.toBeInTheDocument();
    });
    expect(publish).not.toHaveBeenCalled();
  });

  it("keeps commit publish available while freshness is loading", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    const freshnessDeferred = deferred<unknown>();
    getWorkspaceFreshnessMock.mockReturnValue(freshnessDeferred.promise);

    renderPane("publish", workspace({ mode: "edit" }), publish);

    const publishButton = await screen.findByTestId("agents-publish-confirm");
    await waitFor(() =>
      expect(getWorkspaceFreshnessMock).toHaveBeenCalledWith("conversation-1", {
        scope: "full",
      }),
    );
    expect(publishButton).toBeEnabled();
    expect(publishButton).toHaveTextContent("Commit & Publish");
    expect(publishButton).not.toHaveTextContent("Checking");
  });

  it("opens review changes while the file list is still loading", async () => {
    const reviewDeferred = deferred<unknown>();
    getWorkspaceReviewMock.mockReturnValue(reviewDeferred.promise);

    renderPane("publish", workspace({ mode: "edit" }));

    const reviewButton = await screen.findByTestId("agents-review-changes");
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
    expect(reviewButton).toBeEnabled();

    fireEvent.click(reviewButton);

    await waitFor(() =>
      expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1"),
    );
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
  });

  it("disables publish when no changed files are detected", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
    });

    renderPane("publish", workspace({ mode: "edit" }), publish);

    const publishButton = await screen.findByTestId("agents-publish-confirm");
    expect(publishButton).toBeEnabled();
    fireEvent.click(screen.getByTestId("agents-review-changes"));
    await screen.findByText("No changed files detected yet.");
    await waitFor(() =>
      expect(publishButton).toHaveTextContent("Commit & Publish"),
    );
    expect(publishButton).toBeDisabled();

    fireEvent.click(publishButton);

    expect(publish).not.toHaveBeenCalled();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("disables publish once the workspace branch is pushed and current with its PR", async () => {
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
    await waitFor(() =>
      expect(publishButton).toHaveTextContent("PR is up to date"),
    );
    expect(publishButton).toBeDisabled();
    await screen.findByText("1 changed file published for review.");
    expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent(
      "Published changes",
    );

    fireEvent.click(publishButton);

    expect(publish).not.toHaveBeenCalled();
  });

  it("disables publish once a refreshed workspace branch is current with its PR", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue(
      workspaceFreshness({
        freshnessScope: "full",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        targetRef: "origin/feature/agent-screen",
        capturedBaseCommit: "base-sha",
        targetBaseCommit: "base-sha",
        isBaseAhead: false,
        hasUncommittedChanges: false,
        unpublishedCommitCount: 0,
        remoteRefreshed: true,
        worktreeStatusChecked: true,
      }),
    );

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        publicationPushStatus: "refreshed",
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
        publicationPrStatus: "open",
      }),
      publish,
    );

    const publishButton = await screen.findByTestId("agents-publish-confirm");
    await waitFor(() =>
      expect(publishButton).toHaveTextContent("PR is up to date"),
    );
    expect(publishButton).toBeDisabled();

    fireEvent.click(publishButton);

    expect(publish).not.toHaveBeenCalled();
  });

  it("keeps the inline review diff visible after a PR has been opened", async () => {
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "src/Published.tsx",
          status: "modified",
          additions: 4,
          deletions: 1,
          isGenerated: false,
        },
      ],
      commits: [],
      baseRef: "base-sha",
      headRef: "HEAD",
      supportsWorktreeModes: true,
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
        publicationPrStatus: "open",
      }),
    );

    await screen.findByTestId("agents-publish-inline-diffs-section");
    await waitFor(() =>
      expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent(
        "1",
      ),
    );
    expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1");
  });

  it("keeps the PR-backed inline diff visible for a merged missing workspace", async () => {
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "src/Merged.tsx",
          status: "modified",
          additions: 3,
          deletions: 1,
          isGenerated: false,
        },
      ],
      commits: [],
      baseRef: "base-sha",
      headRef: "refs/ralphx/pr-heads/78",
      supportsWorktreeModes: false,
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        status: "missing",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
        publicationPrStatus: "merged",
      }),
    );

    await screen.findByTestId("agents-publish-inline-diffs-section");
    await waitFor(() =>
      expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent(
        "1",
      ),
    );
    expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1");
  });

  it("shows read-only inline diffs for linked ideation plan workspaces", async () => {
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [
        {
          path: "src/PlanBranch.tsx",
          status: "modified",
          additions: 5,
          deletions: 2,
          isGenerated: false,
        },
      ],
      commits: [],
      baseRef: "base-sha",
      headRef: "ralphx/demo/agent-conversation-1",
      supportsWorktreeModes: false,
    });

    renderPane(
      "publish",
      workspace({
        mode: "ideation",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
      }),
    );

    await screen.findByTestId("agents-publish-inline-diffs-section");
    await waitFor(() =>
      expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent(
        "1",
      ),
    );
    expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1");
    expect(screen.getByTestId("agents-publish-confirm")).toHaveTextContent(
      "Managed by Tasks",
    );
    expect(screen.getByTestId("agents-publish-confirm")).toBeDisabled();
  });

  it("keeps publish enabled for a pushed current branch until a PR exists", async () => {
    const user = userEvent.setup();
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
      }),
      publish,
    );

    const publishButton = await screen.findByTestId("agents-publish-confirm");
    await waitFor(() =>
      expect(publishButton).toHaveTextContent("Commit & Publish"),
    );
    await screen.findByText("Loading changed files...");
    expect(publishButton).toBeEnabled();
    expect(publishButton).not.toHaveTextContent("PR is up to date");

    await user.click(publishButton);
    expect(publish).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog", {
      name: "Commit and publish workspace?",
    });
    const confirmButton = within(dialog).getByRole("button", {
      name: "Commit & Publish",
    });
    await waitFor(() => expect(confirmButton).toBeEnabled());
    await user.click(confirmButton);

    await waitFor(() => expect(publish).toHaveBeenCalledWith("conversation-1"));
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
    await waitFor(() =>
      expect(publishButton).toHaveTextContent("Commit & Publish"),
    );
    expect(publishButton).toBeEnabled();

    fireEvent.click(publishButton);
    expect(publish).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Commit & Publish",
      }),
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

    fireEvent.click(
      await screen.findByRole("button", { name: "Open PR #78 in GitHub" }),
    );

    await waitFor(() =>
      expect(openUrlMock).toHaveBeenCalledWith(
        "https://github.com/mock/project/pull/78",
      ),
    );
  });

  it("shows the PR link in the persistent workspace toolbar", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
      }),
    );

    expect(screen.getByTestId("agents-workspace-toolbar")).toBeInTheDocument();
    const prUrl = await screen.findByRole("button", {
      name: "Open PR #78 in GitHub",
    });
    expect(prUrl).toHaveTextContent("PR #78");
    fireEvent.click(prUrl);

    await waitFor(() =>
      expect(openUrlMock).toHaveBeenCalledWith(
        "https://github.com/mock/project/pull/78",
      ),
    );
  });

  it("renders the backend-provided retargeted base state in the publish pane", async () => {
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/deleted-base",
      baseDisplayName: "Current branch (feature/deleted-base)",
      targetRef: "origin/main",
      capturedBaseCommit: "base-sha",
      targetBaseCommit: "base-sha",
      isBaseAhead: false,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
      baseStatus: "retargeted",
      effectiveBaseRef: "main",
      effectiveBaseDisplayName: "Project default (main)",
      baseBlockReason: null,
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/deleted-base",
        baseDisplayName: "Current branch (feature/deleted-base)",
      }),
    );

    expect(
      await screen.findByTestId(
        "agents-base-retargeted",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveTextContent("Base branch retargeted to Project default (main).");
    expect(
      screen.getByLabelText(
        "ralphx/demo/agent-conversation-1 merges into Project default (main)",
      ),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-confirm")).toBeEnabled();
  });

  it("blocks publish actions when backend marks the saved base unsafe", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue({
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
      baseBlockReason:
        "Saved base commit is not contained in the default branch",
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/deleted-base",
        baseDisplayName: "Current branch (feature/deleted-base)",
      }),
      publish,
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId(
        "agents-base-blocked",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveTextContent(
      "Saved base commit is not contained in the default branch",
    );
    expect(
      screen.queryByTestId("agents-publish-confirm"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-rebase-from-base")).toBeEnabled();
    expect(
      screen.queryByTestId("agents-review-changes"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-rebase-from-base"));

    expect(publish).not.toHaveBeenCalled();
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
  });

  it("lets blocked workspaces choose a branch and update from that base", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue({
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
      baseBlockReason:
        "Saved base commit is not contained in the default branch",
    });
    updateWorkspaceFromBaseMock.mockResolvedValue({
      workspace: workspace({
        mode: "edit",
        baseRefKind: "local_branch",
        baseRef: "release/0.8",
        baseDisplayName: "release/0.8",
        baseCommit: "release-base",
      }),
      updated: true,
      targetRef: "release/0.8",
      baseCommit: "release-base",
      baseStatus: "valid",
      effectiveBaseDisplayName: "release/0.8",
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/deleted-base",
        baseDisplayName: "Current branch (feature/deleted-base)",
      }),
      publish,
      false,
      conversation(),
    );

    expect(
      await screen.findByTestId(
        "agents-base-blocked",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveTextContent(
      "Saved base commit is not contained in the default branch",
    );
    expect(screen.getByTestId("agents-rebase-from-base")).toBeEnabled();

    await userEvent.click(screen.getByTestId("agents-rebase-from-base"));

    const dialog = await screen.findByRole("dialog", { name: "Rebase branch" });
    expect(
      within(dialog).getByTestId("agents-rebase-base-select"),
    ).toHaveTextContent("Project default (main)");
    expect(loadBranchBaseOptionsMock).toHaveBeenCalledWith(
      expect.objectContaining({
        workingDirectory: "/tmp/ralphx/conversation-1",
        includeAgentBranches: false,
      }),
    );

    await userEvent.click(
      within(dialog).getByTestId("agents-rebase-base-select"),
    );
    await userEvent.click(await screen.findByText("release/0.8"));
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Rebase branch" }),
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
        {
          kind: "local_branch",
          ref: "release/0.8",
          displayName: "release/0.8",
        },
      ),
    );
    expect(publish).not.toHaveBeenCalled();
  });

  it("closes the Rebase branch dialog and shows a persistent elapsed toast while rebasing", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    const updateDeferred =
      deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
    getWorkspaceFreshnessMock.mockResolvedValue({
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
      baseBlockReason:
        "Saved base commit is not contained in the default branch",
    });
    updateWorkspaceFromBaseMock.mockImplementation(
      () => updateDeferred.promise,
    );

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/deleted-base",
        baseDisplayName: "Current branch (feature/deleted-base)",
      }),
      publish,
      false,
      conversation(),
    );

    await screen.findByTestId(
      "agents-base-blocked",
      undefined,
      deferredHydrationTimeout,
    );
    await userEvent.click(screen.getByTestId("agents-rebase-from-base"));

    const dialog = await screen.findByRole("dialog", { name: "Rebase branch" });
    await userEvent.click(
      within(dialog).getByTestId("agents-rebase-base-select"),
    );
    await userEvent.click(await screen.findByText("release/0.8"));
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Rebase branch" }),
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
        {
          kind: "local_branch",
          ref: "release/0.8",
          displayName: "release/0.8",
        },
      ),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Rebase branch" }),
      ).not.toBeInTheDocument(),
    );
    expect(toastLoadingMock).toHaveBeenCalledWith(
      "Rebasing branch",
      expect.objectContaining({
        description: "Agent conversation • From release/0.8 • 0s",
        duration: Infinity,
        id: "agent-workspace-operation:conversation-1:rebase",
      }),
    );

    updateDeferred.resolve({
      workspace: workspace({
        mode: "edit",
        baseRefKind: "local_branch",
        baseRef: "release/0.8",
        baseDisplayName: "release/0.8",
        baseCommit: "release-base",
      }),
      updated: true,
      targetRef: "release/0.8",
      baseCommit: "release-base",
      baseStatus: "valid",
      effectiveBaseDisplayName: "release/0.8",
    });

    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "Updated from release/0.8",
        {
          description: "Agent conversation • From release/0.8",
          duration: 8_000,
          id: "agent-workspace-operation:conversation-1:rebase",
        },
      ),
    );
    expect(publish).not.toHaveBeenCalled();
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
      "feature/agent-screen",
    );
    const modeStatus = screen.getByTestId("agents-workspace-mode-status");
    expect(modeStatus).toHaveTextContent("Edit");
    expect(screen.getByLabelText("Workspace mode: Edit")).toBe(modeStatus);
    expect(modeStatus).not.toHaveAttribute("style");
    expect(modeStatus.style.borderWidth).toBe("");
    expect(screen.getByTestId("agents-base-stale")).toHaveAttribute(
      "style",
      expect.stringContaining("border-color: var(--border-subtle)"),
    );
    expect(screen.getByTestId("agents-base-stale-icon")).toHaveAttribute(
      "style",
      expect.stringContaining("color: var(--status-warning)"),
    );
    expect(screen.getByTestId("agents-base-stale")).not.toHaveTextContent(
      "Update this workspace before publishing",
    );
    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      }),
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    expect(publish).not.toHaveBeenCalled();
  });

  it("automatically updates a clean workspace from its configured base", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    getWorkspaceFreshnessMock.mockResolvedValue(
      workspaceFreshness({
        freshnessScope: "full",
        baseRef: "release/1.2",
        baseDisplayName: "release/1.2",
        targetRef: "origin/release/1.2",
        capturedBaseCommit: "old-release-base",
        targetBaseCommit: "new-release-base",
        isBaseAhead: true,
        hasUncommittedChanges: false,
        unpublishedCommitCount: 0,
        remoteRefreshed: true,
        worktreeStatusChecked: true,
        baseStatus: "valid",
        effectiveBaseRef: "release/1.2",
        effectiveBaseDisplayName: "release/1.2",
      }),
    );
    updateWorkspaceFromBaseMock.mockResolvedValue({
      workspace: workspace({
        mode: "edit",
        baseRefKind: "local_branch",
        baseRef: "release/1.2",
        baseDisplayName: "release/1.2",
        baseCommit: "new-release-base",
      }),
      updated: true,
      targetRef: "origin/release/1.2",
      baseCommit: "new-release-base",
      baseStatus: "valid",
      effectiveBaseDisplayName: "release/1.2",
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRefKind: "local_branch",
        baseRef: "release/1.2",
        baseDisplayName: "release/1.2",
        baseCommit: "old-release-base",
      }),
      publish,
      false,
      conversation(),
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    expect(updateWorkspaceFromBaseMock.mock.calls[0]).toHaveLength(1);
    expect(updateWorkspaceFromBaseMock).toHaveBeenCalledTimes(1);
    expect(toastLoadingMock).toHaveBeenCalledWith(
      "Refreshing branch",
      expect.objectContaining({
        description: "Agent conversation • From release/1.2 • 0s",
        id: "agent-workspace-operation:conversation-1:update-from-base",
      }),
    );
    expect(publish).not.toHaveBeenCalled();
  });

  it("cancels Update from base without starting the operation toast", async () => {
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
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByTestId("agents-base-stale")).toHaveTextContent(
      "feature/agent-screen",
    );

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Cancel",
      }),
    );

    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    expect(toastLoadingMock).not.toHaveBeenCalled();
  });

  it("closes the Update from base confirmation and shows a persistent elapsed toast while updating", async () => {
    const updateDeferred =
      deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
    updateWorkspaceFromBaseMock.mockImplementation(
      () => updateDeferred.promise,
    );
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
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByTestId("agents-base-stale")).toHaveTextContent(
      "feature/agent-screen",
    );

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Update branch",
      }),
    );

    await waitFor(() => {
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      );
    });
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
    expect(toastLoadingMock).toHaveBeenCalledWith(
      "Updating branch",
      expect.objectContaining({
        description: "Agent conversation • From feature/agent-screen • 0s",
        duration: Infinity,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      }),
    );

    updateDeferred.resolve({
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

    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "Updated from origin/feature/agent-screen",
        {
          description: "Agent conversation • From feature/agent-screen",
          duration: 8_000,
          id: "agent-workspace-operation:conversation-1:update-from-base",
        },
      ),
    );
  });

  it("keeps the Update from base progress toast connected after the pane unmounts while pending", async () => {
    const clearIntervalSpy = vi.spyOn(globalThis, "clearInterval");
    try {
      const updateDeferred =
        deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
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
      updateWorkspaceFromBaseMock.mockImplementation(
        () => updateDeferred.promise,
      );

      const { unmount } = renderPane(
        "publish",
        workspace({
          mode: "edit",
          baseRef: "feature/agent-screen",
          baseDisplayName: "Current branch (feature/agent-screen)",
          baseCommit: "old-base",
        }),
        vi.fn(),
        false,
        conversation(),
      );

      fireEvent.click(await screen.findByTestId("agents-update-from-base"));
      fireEvent.click(
        within(await screen.findByRole("alertdialog")).getByRole("button", {
          name: "Update branch",
        }),
      );
      await waitFor(() =>
        expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
          "conversation-1",
        ),
      );

      clearIntervalSpy.mockClear();
      unmount();

      expect(clearIntervalSpy).not.toHaveBeenCalled();

      await act(async () => {
        updateDeferred.resolve({
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
        await updateDeferred.promise;
      });
    } finally {
      clearIntervalSpy.mockRestore();
    }
  });

  it("replaces the persistent success toast if Update from base settles after the pane unmounts", async () => {
    const updateDeferred =
      deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
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
    updateWorkspaceFromBaseMock.mockImplementation(
      () => updateDeferred.promise,
    );

    const { unmount } = renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    fireEvent.click(await screen.findByTestId("agents-update-from-base"));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      }),
    );
    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );

    unmount();
    await act(async () => {
      updateDeferred.resolve({
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
      await updateDeferred.promise;
    });

    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "Updated from origin/feature/agent-screen",
        {
          description: "Agent conversation • From feature/agent-screen",
          duration: 8_000,
          id: "agent-workspace-operation:conversation-1:update-from-base",
        },
      ),
    );
  });

  it("replaces the persistent error toast if Update from base fails after the pane unmounts", async () => {
    const updateDeferred =
      deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
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
    updateWorkspaceFromBaseMock.mockImplementation(
      () => updateDeferred.promise,
    );

    const { unmount } = renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    fireEvent.click(await screen.findByTestId("agents-update-from-base"));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      }),
    );
    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );

    unmount();
    await act(async () => {
      updateDeferred.reject(new Error("base update failed"));
      await updateDeferred.promise.catch(() => undefined);
    });

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "Failed to update from base",
        {
          closeButton: true,
          description: "Agent conversation • base update failed",
          dismissible: true,
          duration: 12_000,
          id: "agent-workspace-operation:conversation-1:update-from-base",
        },
      ),
    );
  });

  it("replaces the persistent repair toast if Update from base starts repair after the pane unmounts", async () => {
    const updateDeferred =
      deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
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
    updateWorkspaceFromBaseMock.mockImplementation(
      () => updateDeferred.promise,
    );
    getConversationWorkspaceMock.mockResolvedValue(
      workspace({
        mode: "edit",
        publicationPushStatus: "needs_agent",
      }),
    );

    const { unmount } = renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    fireEvent.click(await screen.findByTestId("agents-update-from-base"));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      }),
    );
    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );

    unmount();
    await act(async () => {
      updateDeferred.reject(new Error("Merge conflicts detected"));
      await updateDeferred.promise.catch(() => undefined);
    });

    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith("Repair started", {
        description: "Agent conversation • Merge conflicts detected",
        dismissible: true,
        duration: 8_000,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      }),
    );
  });

  it("refreshes workspace facts when Update from base fails", async () => {
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
      baseStatus: "valid",
      effectiveBaseRef: "feature/agent-screen",
      effectiveBaseDisplayName: "Current branch (feature/agent-screen)",
      baseBlockReason: null,
    });
    updateWorkspaceFromBaseMock.mockRejectedValue(
      new Error("base update failed"),
    );

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByTestId("agents-base-stale")).toHaveTextContent(
      "feature/agent-screen",
    );
    getWorkspaceFreshnessMock.mockClear();

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      }),
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "Failed to update from base",
        {
          closeButton: true,
          description: "Agent conversation • base update failed",
          dismissible: true,
          duration: 12_000,
          id: "agent-workspace-operation:conversation-1:update-from-base",
        },
      ),
    );
    await waitFor(() =>
      expect(getWorkspaceFreshnessMock).toHaveBeenCalledWith("conversation-1", {
        scope: "full",
      }),
    );
  });

  it("shows an auto-dismissing repair toast when Update from base starts agent repair", async () => {
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
      baseStatus: "valid",
      effectiveBaseRef: "feature/agent-screen",
      effectiveBaseDisplayName: "Current branch (feature/agent-screen)",
      baseBlockReason: null,
    });
    updateWorkspaceFromBaseMock.mockRejectedValue(
      new Error("Merge conflicts detected"),
    );
    getConversationWorkspaceMock.mockResolvedValue(
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
        publicationPushStatus: "needs_agent",
      }),
    );

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        baseCommit: "old-base",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    expect(await screen.findByTestId("agents-base-stale")).toHaveTextContent(
      "feature/agent-screen",
    );

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      }),
    );

    await waitFor(() =>
      expect(getConversationWorkspaceMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith("Repair started", {
        description: "Agent conversation • Merge conflicts detected",
        dismissible: true,
        duration: 8_000,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      }),
    );
    expect(toastErrorMock).not.toHaveBeenCalledWith(
      "Failed to update from base",
      expect.anything(),
    );
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
    expect(
      screen.queryByTestId("agents-update-from-base"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(
        "PR #91 has been merged. By continuing this conversation, a new workspace branch will be created automatically.",
      ),
    ).toBeInTheDocument();
    expect(getWorkspaceFreshnessMock).not.toHaveBeenCalled();

    fireEvent.click(publishButton);

    expect(publish).not.toHaveBeenCalled();
  });

  it("shows merged publication state instead of stale blocked PR supervision", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 91,
        publicationPrStatus: "merged",
        publicationPushStatus: "needs_agent",
        prSupervisionStatus: "blocked",
      }),
    );

    expect(
      await screen.findByTestId("agents-publish-confirm"),
    ).toHaveTextContent("Merged");
    expect(
      screen.queryByTestId("agents-pr-supervision-status"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("PR supervision blocked"),
    ).not.toBeInTheDocument();
  });

  it("replaces base update controls while agent repair is pending", async () => {
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

    const repairButton = await screen.findByTestId(
      "agents-publish-repair-pending",
    );
    expect(repairButton).toBeDisabled();
    expect(repairButton).toHaveTextContent("Repair pending");
    expect(
      screen.queryByTestId("agents-update-from-base"),
    ).not.toBeInTheDocument();

    updateWorkspaceFromBaseMock.mockClear();
    fireEvent.click(repairButton);

    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    expect(getWorkspaceFreshnessMock).not.toHaveBeenCalled();
  });

  it("shows repair diff buckets without loading normal workspace review", async () => {
    getWorkspaceReviewMock.mockRejectedValue(
      new Error(
        "Agent conversation workspace is checked out at 'HEAD' instead of branch",
      ),
    );
    const queryClient = createTestQueryClient();
    queryClient.setQueryData(
      agentWorkspaceKeys.scopedFreshness("conversation-1", "full"),
      workspaceFreshness({
        freshnessScope: "full",
        capturedBaseCommit: "old-base",
        targetBaseCommit: "new-base",
        isBaseAhead: true,
      }),
    );

    const publish = vi.fn();
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        publicationPushStatus: "needs_agent",
      }),
      publish,
      false,
      null,
      {},
      queryClient,
    );

    const repairState = await screen.findByTestId(
      "agents-publish-repair-state",
    );
    const actionbar = screen.getByTestId("agents-publish-actionbar");
    const workspaceToolbar = screen.getByTestId("agents-workspace-toolbar");
    expect(repairState).toBeInTheDocument();
    expect(
      within(actionbar).getByText(/RalphX routed this workspace to the agent/),
    ).toBeInTheDocument();
    expect(
      within(repairState).queryByText("Repairing workspace"),
    ).not.toBeInTheDocument();
    expect(
      within(repairState).queryByText(
        /RalphX routed this workspace to the agent/,
      ),
    ).not.toBeInTheDocument();
    expect(
      screen.getAllByText(/RalphX routed this workspace to the agent/),
    ).toHaveLength(1);
    expect(screen.queryByTestId("agents-base-stale")).not.toBeInTheDocument();
    expect(
      within(workspaceToolbar).getByTestId("agents-workspace-sync-status"),
    ).toHaveTextContent("Repair pending");
    await waitFor(() =>
      expect(
        screen.getByTestId("agents-publish-repair-bucket-conflicted"),
      ).toHaveTextContent("Conflicted: 1"),
    );
    expect(
      screen.getByTestId("agents-publish-repair-bucket-unstaged"),
    ).toHaveTextContent("Unstaged: 1 file");
    expect(
      screen.getByTestId("agents-publish-repair-bucket-staged"),
    ).toHaveTextContent("Staged: 1 file");
    expect(
      screen.getByTestId("agents-publish-repair-conflicted-files"),
    ).toHaveTextContent("frontend/src/App.tsx");
    expect(
      screen.queryByText("Could not load workspace changes"),
    ).not.toBeInTheDocument();
    await openAutomationTab();
    expect(
      screen.getByTestId("agents-pr-supervision-controls"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "Auto Publish" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "Autofix CI & Reviews" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "GitHub auto-merge" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-repair-pending")).toBeDisabled();
    expect(
      screen.queryByTestId("agents-update-from-base"),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-close-pr")).not.toBeInTheDocument();
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
    expect(getWorkspaceFreshnessMock).not.toHaveBeenCalled();
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    expect(closeWorkspacePrMock).not.toHaveBeenCalled();
    expect(publish).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(getWorkspaceRepairSummaryMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    expect(getWorkspaceRepairConflictDiffMock).not.toHaveBeenCalled();
    expect(getWorkspaceRepairUnstagedChangesMock).not.toHaveBeenCalled();
  });

  it("keeps pre-PR repair automation preferences configurable while initial Auto Publish is off", async () => {
    setWorkspacePrSupervisionMock.mockImplementation(
      async (
        conversationId: string,
        input: { autoFixEnabled: boolean; autoMergeDesired: boolean },
      ) =>
        workspace({
          mode: "edit",
          conversationId,
          publicationPushStatus: "needs_agent",
          autoPublishInitialPrEnabled: false,
          prAutofixEnabled: input.autoFixEnabled,
          prAutoMergeDesired: input.autoMergeDesired,
          prAutoMergeMethod: "squash",
          prSupervisionStatus:
            input.autoFixEnabled || input.autoMergeDesired
              ? "monitoring"
              : "disabled",
        }),
    );
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "needs_agent",
        autoPublishInitialPrEnabled: false,
      }),
    );
    await openAutomationTab();

    expect(
      await screen.findByTestId("agents-auto-publish-switch"),
    ).not.toBeChecked();
    expect(screen.getByTestId("agents-pr-autofix-switch")).toBeEnabled();
    expect(screen.getByTestId("agents-pr-auto-merge-switch")).toBeEnabled();

    fireEvent.click(screen.getByTestId("agents-pr-autofix-switch"));
    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: false,
          autoMergeMethod: "squash",
        },
      ),
    );
    await waitFor(() =>
      expect(screen.getByTestId("agents-pr-autofix-switch")).toBeChecked(),
    );

    fireEvent.click(screen.getByTestId("agents-pr-auto-merge-switch"));
    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: true,
          autoMergeMethod: "squash",
        },
      ),
    );
    expect(screen.getByTestId("agents-publish-repair-pending")).toBeDisabled();
  });

  it("preserves published-PR automation pause and resume semantics during repair", async () => {
    const initialRepair = publishedPrSupervisionWorkspace({
      publicationPushStatus: "needs_agent",
      prAutofixEnabled: false,
      prAutoMergeDesired: false,
      prAutoMergeCurrent: false,
      prSupervisionStatus: "disabled",
    });
    const enabledRepair = publishedPrSupervisionWorkspace({
      publicationPushStatus: "needs_agent",
      prAutofixEnabled: true,
      prAutoMergeDesired: true,
      prAutoMergeCurrent: true,
      prSupervisionStatus: "monitoring",
    });
    const pausedRepair = publishedPrSupervisionWorkspace({
      publicationPushStatus: "needs_agent",
      autoPublishEnabled: false,
      autoPublishPausedPrAutofixEnabled: true,
      autoPublishPausedPrAutoMergeDesired: true,
      prAutofixEnabled: false,
      prAutoMergeDesired: false,
      prAutoMergeCurrent: false,
      prSupervisionStatus: "paused",
    });
    setWorkspacePrSupervisionMock.mockImplementation(
      async (
        _conversationId: string,
        input: { autoFixEnabled: boolean; autoMergeDesired: boolean },
      ) => ({
        ...initialRepair,
        prAutofixEnabled: input.autoFixEnabled,
        prAutoMergeDesired: input.autoMergeDesired,
        prAutoMergeCurrent: input.autoMergeDesired,
        prSupervisionStatus: "monitoring",
      }),
    );
    setWorkspaceAutoPublishMock.mockImplementation(
      async (
        _conversationId: string,
        input: { autoPublishEnabled: boolean },
      ) => (input.autoPublishEnabled ? enabledRepair : pausedRepair),
    );
    const { rerenderWorkspace } =
      renderPublishPanelForWorkspaceRerender(initialRepair);

    expect(await screen.findByTestId("agents-pr-autofix-switch")).toBeEnabled();
    expect(screen.getByTestId("agents-pr-auto-merge-switch")).toBeEnabled();

    fireEvent.click(screen.getByTestId("agents-pr-autofix-switch"));
    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: false,
          autoMergeMethod: "squash",
        },
      ),
    );
    await waitFor(() =>
      expect(screen.getByTestId("agents-pr-autofix-switch")).toBeChecked(),
    );

    fireEvent.click(screen.getByTestId("agents-pr-auto-merge-switch"));
    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: true,
          autoMergeMethod: "squash",
        },
      ),
    );
    rerenderWorkspace(enabledRepair);

    fireEvent.click(screen.getByTestId("agents-auto-publish-switch"));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Pause Auto Publish",
      }),
    );
    await waitFor(() =>
      expect(setWorkspaceAutoPublishMock).toHaveBeenLastCalledWith(
        "conversation-1",
        { autoPublishEnabled: false },
      ),
    );
    rerenderWorkspace(pausedRepair);

    expect(screen.getByTestId("agents-auto-publish-switch")).not.toBeChecked();
    expect(screen.getByTestId("agents-pr-autofix-switch")).toBeDisabled();
    expect(screen.getByTestId("agents-pr-auto-merge-switch")).toBeDisabled();
    expect(screen.getByTestId("agents-publish-repair-pending")).toBeDisabled();
    expect(
      screen.queryByTestId("agents-update-from-base"),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-close-pr")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-auto-publish-switch"));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Resume Auto Publish",
      }),
    );
    await waitFor(() =>
      expect(setWorkspaceAutoPublishMock).toHaveBeenLastCalledWith(
        "conversation-1",
        { autoPublishEnabled: true },
      ),
    );
    rerenderWorkspace(enabledRepair);

    expect(screen.getByTestId("agents-auto-publish-switch")).toBeChecked();
    expect(screen.getByTestId("agents-pr-autofix-switch")).toBeEnabled();
    expect(screen.getByTestId("agents-pr-auto-merge-switch")).toBeEnabled();
  });

  it("labels merge-paused repair state", async () => {
    getWorkspaceRepairSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 0, additions: 0, deletions: 0 },
      conflicted: { fileCount: 0, files: [] },
      repairState: {
        expectedBranch: "ralphx/demo/agent-conversation-1",
        checkedOutBranch: "HEAD",
        rebaseInProgress: false,
        mergeInProgress: true,
      },
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "needs_agent",
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByTestId("agents-publish-repair-state-label"),
      ).toHaveTextContent("Merge paused for repair"),
    );
  });

  it("labels branch-ready repair state", async () => {
    getWorkspaceRepairSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 0, additions: 0, deletions: 0 },
      conflicted: { fileCount: 0, files: [] },
      repairState: {
        expectedBranch: "ralphx/demo/agent-conversation-1",
        checkedOutBranch: "ralphx/demo/agent-conversation-1",
        rebaseInProgress: false,
        mergeInProgress: false,
      },
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "needs_agent",
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByTestId("agents-publish-repair-state-label"),
      ).toHaveTextContent("Branch ready for repair"),
    );
  });

  it("labels detected repair state when branch details do not match known states", async () => {
    getWorkspaceRepairSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 0, additions: 0, deletions: 0 },
      conflicted: { fileCount: 0, files: [] },
      repairState: {
        expectedBranch: "ralphx/demo/agent-conversation-1",
        checkedOutBranch: "detached-review",
        rebaseInProgress: false,
        mergeInProgress: false,
      },
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "needs_agent",
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByTestId("agents-publish-repair-state-label"),
      ).toHaveTextContent("Repair state detected"),
    );
  });

  it("loads workspace changes for review before publishing", async () => {
    renderPane("publish", workspace({ mode: "edit" }));

    await waitFor(() =>
      expect(screen.getByTestId("agents-review-changes")).toBeEnabled(),
    );
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
      expect(precomputePrDescriptionMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
  });

  it("does not precompute the PR description when the workspace is behind base", async () => {
    getWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      freshnessScope: "full",
      baseRef: "main",
      baseDisplayName: "Project default (main)",
      targetRef: "origin/main",
      capturedBaseCommit: "old-base-sha",
      targetBaseCommit: "new-base-sha",
      isBaseAhead: true,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
      remoteRefreshed: true,
      worktreeStatusChecked: true,
      baseStatus: "valid",
      effectiveBaseRef: "main",
      effectiveBaseDisplayName: "Project default (main)",
      baseBlockReason: null,
    });
    renderPane("publish", workspace({ mode: "edit" }));

    await screen.findByTestId("agents-base-stale");
    fireEvent.click(await screen.findByTestId("agents-review-changes"));

    await waitFor(() =>
      expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1"),
    );
    await screen.findByText("frontend/src/App.tsx");
    expect(precomputePrDescriptionMock).not.toHaveBeenCalled();
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

    await waitFor(() =>
      expect(screen.getByTestId("agents-review-changes")).toBeEnabled(),
    );
    fireEvent.click(screen.getByTestId("agents-review-changes"));
    await waitFor(() =>
      expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1"),
    );
    await user.click(
      await screen.findByTestId(
        "tab-history",
        undefined,
        deferredHydrationTimeout,
      ),
    );

    expect(
      await screen.findByTestId(
        "commit-abc123d",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveTextContent("Update Codex model catalog");
  });

  it("shows description failure without opening a pull request", () => {
    renderPane(
      "publish",
      workspace({ mode: "edit", publicationPushStatus: "description_failed" }),
    );

    expect(screen.getByTestId("agents-publish-pipeline")).toBeInTheDocument();
    expect(
      within(screen.getByTestId("agents-publish-pipeline")).getByText(
        /no pull request was opened/i,
      ),
    ).toBeInTheDocument();
  });

  it("shows auto-merge deferred warning after the pull request is published with waiting status", () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
        prAutoMergeDesired: true,
        prAutoMergeCurrent: false,
        prSupervisionStatus: "waiting",
      }),
    );

    expect(screen.getByTestId("agents-publish-pipeline")).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-publish-step-auto_merge"),
    ).toHaveTextContent("Auto-merge deferred");
    expect(
      screen.queryByText(/latest publish attempt failed/i),
    ).not.toBeInTheDocument();
  });

  it("surfaces workspace review auto-merge guard states in Commit & Publish", () => {
    const paneWorkspace = workspace({
      mode: "edit",
      publicationPushStatus: "pushed",
      publicationPrNumber: 78,
      publicationPrUrl: "https://github.com/mock/project/pull/78",
      prAutoMergeDesired: true,
      prAutoMergeCurrent: false,
      prSupervisionStatus: "waiting",
    });
    const { rerenderWorkspace } = renderPublishPanelForWorkspaceRerender(
      paneWorkspace,
      createTestQueryClient(),
      workspaceReviewContext({
        reviewGateStatus: "passed",
        reviewOutcome: "passed",
        autoMergeGuardStatus: "paused_for_review",
        autoMergeGuardPrNumber: 78,
        autoMergeGuardMethod: "squash",
        isCurrent: true,
      }),
    );

    expect(
      screen.getByTestId("agents-publish-review-auto-merge-guard"),
    ).toHaveTextContent(
      "GitHub auto-merge is paused while Workspace Review is active.",
    );

    rerenderWorkspace(
      paneWorkspace,
      workspaceReviewContext({
        reviewGateStatus: "passed",
        reviewOutcome: "passed",
        autoMergeGuardStatus: "awaiting_publish",
        autoMergeGuardPrNumber: 78,
        autoMergeGuardMethod: "squash",
        isCurrent: true,
      }),
    );
    expect(
      screen.getByTestId("agents-publish-review-auto-merge-guard"),
    ).toHaveTextContent(
      "GitHub auto-merge will resume after these reviewed changes are published.",
    );

    rerenderWorkspace(
      paneWorkspace,
      workspaceReviewContext({
        reviewGateStatus: "passed",
        reviewOutcome: "passed",
        autoMergeGuardStatus: "restore_failed",
        autoMergeGuardPrNumber: 78,
        autoMergeGuardMethod: "squash",
        autoMergeGuardLastError: "Branch protection changed",
        isCurrent: true,
      }),
    );
    expect(
      screen.getByTestId("agents-publish-review-auto-merge-guard"),
    ).toHaveTextContent("Branch protection changed");
  });

  it("does not keep auto-merge request progress active while PR supervision is monitoring", () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
        prAutoMergeDesired: true,
        prAutoMergeCurrent: false,
        prSupervisionStatus: "monitoring",
      }),
    );

    expect(
      screen.queryByTestId("agents-publish-pipeline"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Monitoring PR")).toBeInTheDocument();
  });

  it("does not render a redundant synced-annotation summary for published workspaces", async () => {
    getWorkspacePrAnnotationsMock.mockResolvedValue({
      prNumber: 78,
      headSha: "head-sha",
      annotations: [
        {
          id: "review-comment:1",
          source: "review_comment",
          path: "frontend/src/App.tsx",
          side: "right",
          startLine: 1,
          endLine: 1,
          startColumn: null,
          endColumn: null,
          level: "comment",
          status: null,
          title: null,
          message: "Please adjust this line.",
          author: "octocat",
          checkName: null,
          url: null,
          isOutdated: false,
          createdAt: null,
        },
      ],
      sourcesUnavailable: [],
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      }),
    );

    await waitFor(
      () =>
        expect(getWorkspacePrAnnotationsMock).toHaveBeenCalledWith(
          "conversation-1",
        ),
      deferredHydrationTimeout,
    );
    expect(
      screen.queryByText("1 GitHub annotation synced"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-publish-summaries"),
    ).not.toBeInTheDocument();
  });

  it("shows partial GitHub PR annotation unavailability for published workspaces", async () => {
    getWorkspacePrAnnotationsMock.mockResolvedValue({
      prNumber: 78,
      headSha: null,
      annotations: [],
      sourcesUnavailable: [
        {
          source: "check_runs",
          reason: "Missing checks permission",
        },
      ],
    });

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      }),
    );

    await waitFor(
      () =>
        expect(
          screen.getByTestId("agents-pr-annotations-partial-warning"),
        ).toHaveTextContent("GitHub annotations partially unavailable"),
      deferredHydrationTimeout,
    );
  });

  it("hides the publish pipeline after agent repair terminal state", () => {
    renderPane(
      "publish",
      workspace({ mode: "edit", publicationPushStatus: "needs_agent" }),
    );

    expect(
      screen.queryByTestId("agents-publish-pipeline"),
    ).not.toBeInTheDocument();
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

    renderPane(
      "publish",
      workspace({ mode: "edit", publicationPushStatus: "needs_agent" }),
    );
    await openHistoryTab();

    expect(
      await screen.findByTestId(
        "agents-publish-events",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toBeInTheDocument();
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
    await openHistoryTab();

    expect(
      await screen.findByTestId(
        "agents-publish-events",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Checking workspace changes"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Pushing agent branch")).not.toBeInTheDocument();
    expect(screen.getByText("Draft pull request is ready")).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-publish-event-icon-event-published"),
    ).toHaveAttribute("data-state", "succeeded");
  });

  it("shows approved-plan CTAs for an imported clone session discovered via v1_start_ideation", async () => {
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
                result: {
                  session_id: "cloned-session-1",
                  plan_imported: true,
                  cloned_plan_artifact_id: "cloned-artifact-1",
                  source_plan_artifact_id: "source-artifact-1",
                },
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
        id: "cloned-session-1",
        projectId: "project-1",
        title: "Imported Plan",
        titleSource: "auto",
        status: "active",
        planArtifactId: "cloned-artifact-1",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        sourceSessionId: "source-session-1",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "cloned-artifact-1",
      type: "specification",
      name: "Imported Plan",
      content: {
        type: "inline",
        text: "# Imported Plan\n\nCloned content.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "plan_import",
        version: 1,
      },
      derivedFrom: ["source-artifact-1"],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "cloned-artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-04-23T09:00:00Z",
      },
    });

    renderPane(
      "plan",
      workspace({ mode: "plan", linkedIdeationSessionId: null }),
      vi.fn(),
      false,
      conversation(),
    );

    await waitFor(() =>
      expect(getIdeationSessionMock).toHaveBeenCalledWith("cloned-session-1"),
    );
    await waitFor(() =>
      expect(getSessionPlanMock).toHaveBeenCalledWith("cloned-session-1"),
    );

    expect(
      await screen.findByRole("button", { name: /Create Proposals/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Implement Directly/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Approve Plan/i }),
    ).not.toBeInTheDocument();
  });

  it("shows draft-approval CTA for an imported clone session with a draft plan", async () => {
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
                result: {
                  session_id: "cloned-session-draft",
                  plan_imported: true,
                },
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
        id: "cloned-session-draft",
        projectId: "project-1",
        title: "Draft Imported Plan",
        titleSource: "auto",
        status: "active",
        planArtifactId: "cloned-artifact-draft",
        seedTaskId: null,
        parentSessionId: null,
        createdAt: "2026-04-23T09:00:00Z",
        updatedAt: "2026-04-23T09:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        sessionFlow: "planning",
        sourceSessionId: "source-session-1",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getSessionPlanMock.mockResolvedValue({
      id: "cloned-artifact-draft",
      type: "specification",
      name: "Draft Imported Plan",
      content: {
        type: "inline",
        text: "# Draft Plan\n\nNeeds approval.",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "plan_import",
        version: 1,
      },
      derivedFrom: ["source-artifact-1"],
      bucketId: "prd-library",
      planApproval: {
        status: "draft",
      },
    });

    renderPane(
      "plan",
      workspace({ mode: "plan", linkedIdeationSessionId: null }),
      vi.fn(),
      false,
      conversation(),
    );

    await waitFor(() =>
      expect(getIdeationSessionMock).toHaveBeenCalledWith(
        "cloned-session-draft",
      ),
    );

    expect(
      await screen.findByRole("button", { name: /Approve Plan/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Implement Directly/i }),
    ).not.toBeInTheDocument();
  });
});
