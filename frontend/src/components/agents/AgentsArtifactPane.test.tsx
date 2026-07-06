import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState, type ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type {
  AgentConversationRuntimeStatus,
  AgentWorkspacePrReviewContext,
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { useAgentSessionStore, type AgentArtifactTab } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import { createTestQueryClient } from "@/test/store-utils";
import { chatKeys } from "@/hooks/useChat";
import { reviewSettingsKeys } from "@/hooks/useReviewSettings";
import { AgentsArtifactPane } from "./AgentsArtifactPane";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";

const deferredHydrationTimeout = { timeout: 3_000 };

const {
  getWorkspaceChangesMock,
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
  listPublicationEventsMock,
  getWorkspaceFreshnessMock,
  updateWorkspaceFromBaseMock,
  setWorkspaceAutoPublishMock,
  setWorkspacePrSupervisionMock,
  precomputePrDescriptionMock,
  closeWorkspacePrMock,
  sendAgentMessageMock,
  switchAgentConversationModeMock,
  listAgentConversationIssuesMock,
  loadBranchBaseOptionsMock,
  getArtifactMock,
  getSessionPlanMock,
  approvePlanArtifactMock,
  getPlanComplexityAssessmentMock,
  confirmVerificationMock,
  getVerificationSpecialistsMock,
  getIdeationSessionMock,
  getIdeationChildrenMock,
  useConversationMock,
  useDependencyGraphMock,
  useAgentComposerPlanReferencesMock,
  useFileDropMock,
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
} = vi.hoisted(() => ({
  getWorkspaceChangesMock: vi.fn(),
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
  listPublicationEventsMock: vi.fn(),
  getWorkspaceFreshnessMock: vi.fn(),
  updateWorkspaceFromBaseMock: vi.fn(),
  setWorkspaceAutoPublishMock: vi.fn(),
  setWorkspacePrSupervisionMock: vi.fn(),
  precomputePrDescriptionMock: vi.fn(),
  closeWorkspacePrMock: vi.fn(),
  sendAgentMessageMock: vi.fn(),
  switchAgentConversationModeMock: vi.fn(),
  listAgentConversationIssuesMock: vi.fn(),
  loadBranchBaseOptionsMock: vi.fn(),
  getArtifactMock: vi.fn(),
  getSessionPlanMock: vi.fn(),
  approvePlanArtifactMock: vi.fn(),
  getPlanComplexityAssessmentMock: vi.fn(),
  confirmVerificationMock: vi.fn(),
  getVerificationSpecialistsMock: vi.fn(),
  getIdeationSessionMock: vi.fn(),
  getIdeationChildrenMock: vi.fn(),
  useConversationMock: vi.fn(),
  useDependencyGraphMock: vi.fn(),
  useAgentComposerPlanReferencesMock: vi.fn(),
  useFileDropMock: vi.fn(),
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
      precomputeAgentConversationWorkspacePrDescription: (...args: unknown[]) =>
        precomputePrDescriptionMock(...args),
      closeAgentWorkspacePr: (...args: unknown[]) =>
        closeWorkspacePrMock(...args),
      sendAgentMessage: (...args: unknown[]) =>
        sendAgentMessageMock(...args),
      switchAgentConversationMode: (...args: unknown[]) =>
        switchAgentConversationModeMock(...args),
      listAgentConversationIssues: (...args: unknown[]) =>
        listAgentConversationIssuesMock(...args),
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
    getAgentConversationWorkspaceRepairChangeSummary: (...args: unknown[]) =>
      getWorkspaceRepairSummaryMock(...args),
    getAgentConversationWorkspaceRepairStagedFileChanges: (...args: unknown[]) =>
      getWorkspaceRepairStagedChangesMock(...args),
    getAgentConversationWorkspaceRepairUnstagedFileChanges: (...args: unknown[]) =>
      getWorkspaceRepairUnstagedChangesMock(...args),
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
  const actual = await importOriginal<typeof import("@/components/shared/branchBaseOptions")>();
  return {
    ...actual,
    loadBranchBaseOptions: (...args: unknown[]) => loadBranchBaseOptionsMock(...args),
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
      },
    },
  };
});

vi.mock("@/components/Ideation/VerificationPanel", () => ({
  VerificationPanel: ({ session }: { session: { id: string } }) => (
    <div data-testid="mock-verification-panel">{session.id}</div>
  ),
}));

vi.mock("@/components/tasks/TaskBoard", () => ({
  TaskBoard: ({ onTaskSelect }: { onTaskSelect?: (taskId: string) => void }) => (
    <button
      type="button"
      data-testid="mock-agent-task-card"
      onClick={() => onTaskSelect?.("task-1")}
    >
      Open task
    </button>
  ),
}));

vi.mock("@/components/agents/task-details/AgentsTaskDetailOverlay", () => ({
  AgentsTaskDetailOverlay: ({
    selectedTaskIdOverride,
    onCloseOverride,
  }: {
    selectedTaskIdOverride?: string | null;
    onCloseOverride?: () => void;
  }) =>
    selectedTaskIdOverride ? (
      <div
        data-testid="mock-agent-task-detail"
        data-task-id={selectedTaskIdOverride}
      >
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
      PR #{workspace?.publicationPrNumber ?? workspace?.sourcePullRequest?.number ?? "none"}
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
    getSpecialists: (...args: unknown[]) => getVerificationSpecialistsMock(...args),
  },
}));

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

vi.mock("@/hooks/useAgentComposerResources", () => ({
  useAgentComposerPlanReferences: (...args: unknown[]) =>
    useAgentComposerPlanReferencesMock(...args),
}));

vi.mock("@/hooks/useFileDrop", () => ({
  useFileDrop: (...args: unknown[]) => useFileDropMock(...args),
}));

vi.mock("@/hooks/useVerificationStatus", () => ({
  useVerificationStatus: (...args: unknown[]) => useVerificationStatusMock(...args),
  verificationStatusKey: (sessionId: string) => ["verification", sessionId] as const,
}));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGitAuthDiagnostics: (...args: unknown[]) => useGitAuthDiagnosticsMock(...args),
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
  autoPublishEnabled: true,
  autoPublishInitialPrEnabled: false,
  autoPublishPausedPrAutofixEnabled: null,
  autoPublishPausedPrAutoMergeDesired: null,
  status: "active",
  createdAt: "2026-04-23T09:00:00Z",
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

function reviewSettings(
  overrides: Partial<ReturnType<typeof baseReviewSettings>> = {},
) {
  return {
    ...baseReviewSettings(),
    ...overrides,
  };
}

function baseReviewSettings() {
  return {
    require_human_review: false,
    require_workspace_review: true,
    max_fix_attempts: 3,
    max_revision_cycles: 5,
    ai_review_enabled: true,
    ai_review_auto_fix: true,
    require_fix_approval: false,
    auto_create_followup_agent_conversation: true,
    run_task_validations: true,
  };
}

let reviewSettingsResponse = reviewSettings();

const workspaceReviewTarget = {
  scope: "selected_source",
  baseRef: "base-sha",
  baseSha: "base-sha",
  headRef: "refs/ralphx/pr-heads/351",
  headSha: "head-sha",
  diffFingerprint: "fingerprint-351",
  sourcePullRequestNumber: 351,
};

function workspaceReviewContext(overrides: {
  conversationId?: string;
  target?: typeof workspaceReviewTarget | null;
  status?: "idle" | "ready" | "reviewing" | "blocked";
  reviewOutcome?: "none" | "passed" | "blocking" | "no_changes" | "run_failed";
  reviewGateStatus?:
    | "not_required"
    | "required"
    | "reviewing"
    | "passed"
    | "blocking"
    | "failed";
  reviewArtifactId?: string | null;
  reviewArtifactVersion?: number | null;
  reviewConversationId?: string | null;
  isCurrent?: boolean;
  isOutdated?: boolean;
  shouldShowTab?: boolean;
  lastError?: string | null;
} = {}) {
  const target = overrides.target === undefined ? workspaceReviewTarget : overrides.target;
  const reviewArtifactId = overrides.reviewArtifactId ?? null;
  const conversationId = overrides.conversationId ?? "conversation-1";

  return {
    success: true,
    workspace: workspace({ conversationId, mode: "edit" }),
    events: [],
    target,
    monitor: {
      conversationId,
      status: overrides.status ?? "idle",
      reviewOutcome: overrides.reviewOutcome ?? "none",
      reviewGateStatus: overrides.reviewGateStatus ?? "not_required",
      reviewConversationId: overrides.reviewConversationId ?? null,
      reviewArtifactId,
      reviewArtifactVersion: overrides.reviewArtifactVersion ?? null,
      lastError: overrides.lastError ?? null,
    },
    isCurrent: overrides.isCurrent ?? false,
    isOutdated: overrides.isOutdated ?? false,
    shouldShowTab: overrides.shouldShowTab ?? Boolean(target || reviewArtifactId),
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
    health: null,
    reviewFeedback: null,
    monitor: {
      conversationId,
      projectId: "project-1",
      prNumber: 78,
      status: "watching",
      monitorEnabled: true,
      firstReviewCompleted: Boolean(reviewArtifactId),
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
    </QueryClientProvider>
  );
}

function artifactTabIds(tabRow: HTMLElement): string[] {
  return Array.from(
    tabRow.querySelectorAll("[data-testid^='agents-artifact-tab-']"),
  ).map((tab) => tab.getAttribute("data-testid") ?? "");
}

function expectReviewImmediatelyBeforePublish(tabRow: HTMLElement) {
  const ids = artifactTabIds(tabRow);
  const reviewIndex = ids.indexOf("agents-artifact-tab-review");
  const publishIndex = ids.indexOf("agents-artifact-tab-publish");

  expect(reviewIndex).toBeGreaterThanOrEqual(0);
  expect(publishIndex).toBeGreaterThanOrEqual(0);
  expect(reviewIndex).toBe(publishIndex - 1);
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
    vi.mocked(invoke).mockReset();
    reviewSettingsResponse = reviewSettings();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_review_settings") {
        return Promise.resolve(reviewSettingsResponse);
      }
      return Promise.resolve(undefined);
    });
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
      mergedWithMarkers: "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n",
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
        input: { autoFixEnabled: boolean; autoMergeDesired: boolean }
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
        })
    );
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
          prSupervisionStatus: input.autoPublishEnabled ? "monitoring" : "paused",
        })
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
    listAgentConversationIssuesMock.mockResolvedValue([]);
    getArtifactMock.mockResolvedValue(null);
    getSessionPlanMock.mockResolvedValue(null);
    approvePlanArtifactMock.mockResolvedValue(null);
    getPlanComplexityAssessmentMock.mockResolvedValue(null);
    confirmVerificationMock.mockResolvedValue({ status: "ok" });
    getVerificationSpecialistsMock.mockResolvedValue({ specialists: [] });
    getIdeationSessionMock.mockResolvedValue(null);
    getIdeationChildrenMock.mockResolvedValue([]);
    useConversationMock.mockReturnValue({
      data: null,
      isLoading: false,
    });
    useDependencyGraphMock.mockReturnValue({
      data: null,
      isLoading: false,
    });
    useAgentComposerPlanReferencesMock.mockReturnValue({
      data: { plans: [], truncated: false },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
    });
    useFileDropMock.mockReturnValue({
      isDragging: false,
      dropProps: {
        onDragEnter: vi.fn(),
        onDragOver: vi.fn(),
        onDragLeave: vi.fn(),
        onDrop: vi.fn(),
      },
      error: null,
      clearError: vi.fn(),
    });
    useVerificationStatusMock.mockReturnValue({
      data: null,
      isLoading: false,
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
      data: true,
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

  it("hides the Issues tab when a project conversation has no open issues", async () => {
    listAgentConversationIssuesMock.mockResolvedValue([]);

    renderPane("publish", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    await waitFor(() =>
      expect(listAgentConversationIssuesMock).toHaveBeenCalledWith("conversation-1"),
    );
    expect(screen.queryByTestId("agents-artifact-tab-issues")).not.toBeInTheDocument();
  });

  it("shows the Issues tab when a project conversation has open issues", async () => {
    listAgentConversationIssuesMock.mockResolvedValue([{ id: "issue-1" }]);

    renderPane("publish", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    expect(await screen.findByTestId("agents-artifact-tab-issues")).toBeInTheDocument();
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
      null,
      vi.fn(),
      false,
      {
        ...conversation(),
        contextType: "ideation",
        contextId: "session-1",
        agentMode: "ideation",
      },
    );

    await waitFor(() => expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"));
    await waitFor(() => expect(getSessionPlanMock).toHaveBeenCalledWith("session-1"));
    expect(screen.queryByText("No plan yet")).not.toBeInTheDocument();
  });

  it("renders the lightweight Plan start surface without an attached ideation run", () => {
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
      screen.getByRole("searchbox", { name: "Search project plans" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Import markdown")).toBeInTheDocument();
    expect(screen.queryByText("No ideation run attached")).not.toBeInTheDocument();
    expect(getIdeationSessionMock).not.toHaveBeenCalled();
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
      "border-color: var(--overlay-faint);"
    );
    expect(activeTab.parentElement?.className).toContain("self-stretch");
    expect(activeTab.className).toContain("self-stretch");
    expect(activeTab.getAttribute("data-theme-button-skip")).toBe("true");
    expect(inactiveTab.getAttribute("data-theme-button-skip")).toBe("true");
    expect(activeTab.className).not.toContain("border-b-2");
    expect(activeTab.querySelector("span[style='background: var(--accent-primary);']")).not.toBeNull();
    expect(inactiveTab.querySelector("span[style='background: var(--accent-primary);']")).toBeNull();
  });

  it("opens task details inside the Agents tasks artifact surface", async () => {
    const onTaskArtifactSelectionChange = vi.fn();
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

  it("selects task details from an external task focus request", async () => {
    const onTaskArtifactSelectionChange = vi.fn();
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

  it("renders only the publish tab for edit workspaces", () => {
    renderPane("publish", workspace({ mode: "edit" }));

    expect(screen.getByTestId("agents-artifact-tab-publish")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-plan")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-verification")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-proposal")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-tasks")).not.toBeInTheDocument();
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
    expect(await screen.findByTestId("mock-pr-detail-panel")).toHaveTextContent("PR #42");
  });

  it("renders the Review tab immediately before Commit & Publish for merged edit workspaces with reviewable PR changes", async () => {
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

    const tabRow = screen.getByTestId("agents-artifact-tab-row");
    await screen.findByTestId("agents-artifact-tab-review");

    expectReviewImmediatelyBeforePublish(tabRow);
    expect(screen.getByTestId("agents-artifact-tab-publish")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pane")).toBeInTheDocument();
  });

  it("does not auto-start Review when fallback selects the Review tab", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        shouldShowTab: true,
      }),
    );

    renderPane("tasks", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run review" })).toBeInTheDocument();
    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();
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

    fireEvent.click(await screen.findByTestId("agents-artifact-tab-review"));

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(startWorkspaceReviewMock).not.toHaveBeenCalled();
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

    fireEvent.click(await screen.findByTestId("agents-artifact-tab-review"));

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(onFocusWorkspaceReview).toHaveBeenCalledWith("review-conversation-1");
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

    fireEvent.click(await screen.findByTestId("agents-artifact-tab-review"));

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();
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

    fireEvent.click(await screen.findByTestId("agents-publish-review-required"));

    expect(await screen.findByText("Review not run")).toBeInTheDocument();
    expect(onFocusWorkspaceReview).toHaveBeenCalledWith("review-conversation-1");
  });

  it("does not block publishing on a required Review gate when policy is disabled", async () => {
    const queryClient = createTestQueryClient();
    const disabledReviewSettings = reviewSettings({
      require_workspace_review: false,
    });
    reviewSettingsResponse = disabledReviewSettings;
    queryClient.setQueryData(reviewSettingsKeys.all, disabledReviewSettings);
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
      expect(getWorkspaceReviewContextMock).toHaveBeenCalledWith("conversation-1"),
    );
    await screen.findByTestId("agents-artifact-tab-review");
    expect(
      screen.queryByTestId("agents-publish-review-required"),
    ).not.toBeInTheDocument();
    expect(await screen.findByTestId("agents-publish-confirm")).toBeInTheDocument();
  });

  it("does not render a Review tab status dot for non-running review states", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        reviewGateStatus: "required",
        shouldShowTab: true,
      }),
    );

    renderPane("publish", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    const reviewTab = await screen.findByTestId("agents-artifact-tab-review");

    expect(reviewTab.querySelector('span[aria-hidden="true"].rounded-full')).toBeNull();
  });

  it("renders a Review tab status dot only while review is running", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "reviewing",
        reviewGateStatus: "reviewing",
        shouldShowTab: true,
      }),
    );

    renderPane("publish", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    const reviewTab = await screen.findByTestId("agents-artifact-tab-review");

    expect(
      reviewTab.querySelector('span[aria-hidden="true"].rounded-full'),
    ).toBeInTheDocument();
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

    renderPane("publish", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    const reviewTab = await screen.findByTestId("agents-artifact-tab-review");
    const reviewIcon = reviewTab.querySelector("svg");

    expect(reviewIcon).toHaveStyle({ color: "var(--status-success)" });
    expect(reviewTab.querySelector('span[aria-hidden="true"].rounded-full')).toBeNull();
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

    renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

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
      expect(queryClient.getQueryData(agentWorkspaceKeys.workspaceReview("conversation-1")))
        .toEqual(startedContext),
    );
    expect(invalidateQueriesSpy).toHaveBeenCalledWith({
      queryKey: chatKeys.conversationTimeline("review-conversation-1"),
    });
    expect(onFocusWorkspaceReview).toHaveBeenCalledWith("review-conversation-1");
  });

  it("runs a forced update for an outdated Review artifact", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-1",
        reviewArtifactVersion: 2,
        isOutdated: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-1",
      type: "workspace_review",
      name: "Workspace Review",
      content: {
        type: "inline",
        text: "# Workspace Review\n\nNo blocking findings.",
      },
      metadata: {
        createdAt: "2026-04-23T09:30:00Z",
        createdBy: "ralphx-workspace-reviewer",
        version: 2,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    expect(await screen.findByText("Review is outdated")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Update review" }));

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: true,
      }),
    );
  });

  it("disables the Review update action while a related runtime is generating", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-2",
        reviewArtifactVersion: 2,
        isOutdated: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockResolvedValue(workspaceReviewArtifact(2));
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": conversationRuntimeStatus(),
    });

    renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    expect(await screen.findByText("Review is outdated")).toBeInTheDocument();

    const updateReviewButton = screen.getByRole("button", {
      name: "Update review",
    });
    await waitFor(() => expect(updateReviewButton).toBeDisabled());
    expect(
      await screen.findByTestId("agents-review-action-disabled-reason"),
    ).toHaveTextContent("Review is available after the current agent run finishes.");
    expect(updateReviewButton).toHaveAttribute(
      "aria-describedby",
      "agents-review-action-disabled-reason",
    );

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
        isOutdated: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockResolvedValue(workspaceReviewArtifact(2));
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

    renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    expect(await screen.findByText("Review is outdated")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Update review" })).toBeDisabled();
    });

    expect(useChatStore.getState().agentStatus[storeKey]).toBe("generating");
    expect(
      useChatStore.getState().agentActivityLabels[storeKey],
    ).toBe("running");
  });

  it("keeps the Review update action enabled while a related runtime is waiting for input", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-2",
        reviewArtifactVersion: 2,
        isOutdated: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockResolvedValue(workspaceReviewArtifact(2));
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

    renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    expect(await screen.findByText("Review is outdated")).toBeInTheDocument();

    const updateReviewButton = screen.getByRole("button", {
      name: "Update review",
    });
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

    renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    const content = await screen.findByTestId("agents-artifact-content-review");

    expect(await within(content).findByText("Reviewing")).toBeInTheDocument();
    expect(within(content).queryByRole("heading", { name: "Review" })).not.toBeInTheDocument();
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

    expect(screen.queryByTestId("agents-artifact-tab-review")).not.toBeInTheDocument();
    expect(screen.queryByText("Reviewing")).not.toBeInTheDocument();
  });

  it("offers a forced rerun for a current Review artifact without success toasts", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-1",
        reviewArtifactVersion: 2,
        reviewGateStatus: "passed",
        isCurrent: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-1",
      type: "workspace_review",
      name: "Workspace Review",
      content: {
        type: "inline",
        text: "# Workspace Review\n\nNo blocking findings.",
      },
      metadata: {
        createdAt: "2026-04-23T09:30:00Z",
        createdBy: "ralphx-workspace-reviewer",
        version: 2,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    expect(await screen.findByText("Review passed")).toBeInTheDocument();
    expect(screen.getByTestId("agents-review-open-publish")).toHaveTextContent(
      "Commit & Publish",
    );
    fireEvent.pointerDown(screen.getByTestId("agents-review-actions-menu"), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(screen.getByTestId("agents-review-rerun"));

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: true,
      }),
    );
    expect(toastMessageMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("routes the promoted Review publish CTA through the parent publish opener", async () => {
    const openPublish = vi.fn();
    const tabChange = vi.fn();
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-1",
        reviewArtifactVersion: 2,
        reviewGateStatus: "passed",
        isCurrent: true,
        isOutdated: false,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockResolvedValue({
      ...workspaceReviewArtifact(2),
      id: "review-artifact-1",
    });

    renderPane(
      "review",
      workspace({ mode: "edit" }),
      vi.fn(),
      false,
      conversation(),
      {
        onOpenPublish: openPublish,
        onTabChange: tabChange,
      },
    );

    fireEvent.click(await screen.findByTestId("agents-review-open-publish"));

    expect(openPublish).toHaveBeenCalledTimes(1);
    expect(tabChange).not.toHaveBeenCalledWith("publish");
  });

  it("does not let stale completed start data override the current Review context", async () => {
    getWorkspaceReviewContextMock.mockResolvedValue(
      workspaceReviewContext({
        target: workspaceReviewTarget,
        status: "ready",
        reviewArtifactId: "review-artifact-v2",
        reviewArtifactVersion: 2,
        reviewGateStatus: "passed",
        isCurrent: true,
        shouldShowTab: true,
      }),
    );
    getArtifactMock.mockResolvedValue({
      id: "review-artifact-v2",
      type: "workspace_review",
      name: "Workspace Review",
      content: {
        type: "inline",
        text: "# Workspace Review\n\nCurrent v2 findings.",
      },
      metadata: {
        createdAt: "2026-04-23T09:35:00Z",
        createdBy: "ralphx-workspace-reviewer",
        version: 2,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });
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

    renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    expect(await screen.findByText("Review passed")).toBeInTheDocument();
    fireEvent.pointerDown(screen.getByTestId("agents-review-actions-menu"), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(screen.getByTestId("agents-review-rerun"));

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: true,
      }),
    );
    expect(screen.getByText("Review passed")).toBeInTheDocument();
    expect(screen.queryByText("Reviewing")).not.toBeInTheDocument();
    expect(screen.queryByText("Review is outdated")).not.toBeInTheDocument();
    expect(screen.queryByText(/The Review below is still available/)).not.toBeInTheDocument();
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

    renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

    expect(await screen.findByText("Review failed")).toBeInTheDocument();
    expect(screen.getByText("Reviewer child chat failed")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry review" }));

    await waitFor(() =>
      expect(startWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1", {
        force: true,
      }),
    );
  });

  it("polls the Review context while the background review is preparing", async () => {
    vi.useFakeTimers();
    try {
      getWorkspaceReviewContextMock.mockResolvedValue(
        workspaceReviewContext({
          target: workspaceReviewTarget,
          status: "reviewing",
          shouldShowTab: true,
        }),
      );

      renderPane("review", workspace({ mode: "edit" }), vi.fn(), false, conversation());

      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      await act(async () => {});

      expect(screen.getByText("Reviewing")).toBeInTheDocument();
      expect(getWorkspaceReviewContextMock).toHaveBeenCalledTimes(1);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2_000);
      });

      expect(getWorkspaceReviewContextMock).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not fall back to publish for generic edit workspace pane opens", () => {
    renderPane("plan", workspace({ mode: "edit" }));

    expect(screen.getByTestId("agents-artifact-tab-publish")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-publish-pane")).not.toBeInTheDocument();
  });

  it("shows pre-PR Auto Publish with independent PR automation controls", async () => {
    renderPane("publish", workspace({ mode: "edit" }));

    expect(await screen.findByTestId("agents-auto-publish-switch")).not.toBeChecked();
    expect(screen.getByTestId("agents-pr-autofix-switch")).toBeEnabled();
    expect(screen.getByTestId("agents-pr-auto-merge-switch")).toBeEnabled();
  });

  it("renders the Review tab for Review PR workspaces without plan tabs", async () => {
    getPrReviewContextMock.mockResolvedValue({
      success: true,
      workspace: workspace({ mode: "review_pr" }),
      events: [],
      prNumber: 78,
      prUrl: "https://github.com/mock/project/pull/78",
      currentHeadSha: "head-sha",
      health: null,
      reviewFeedback: null,
      monitor: {
        conversationId: "conversation-1",
        projectId: "project-1",
        prNumber: 78,
        status: "watching",
        monitorEnabled: true,
        firstReviewCompleted: true,
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
      { ...conversation(), agentMode: "review_pr" },
    );

    const tabRow = screen.getByTestId("agents-artifact-tab-row");
    await screen.findByTestId("agents-artifact-tab-review");

    expect(artifactTabIds(tabRow)).toContain("agents-artifact-tab-review");
    expect(screen.queryByTestId("agents-artifact-tab-plan")).not.toBeInTheDocument();
    expect(await screen.findByText("PR Review")).toBeInTheDocument();
    expect(getPrReviewContextMock).toHaveBeenCalledWith("conversation-1");
    expect(getArtifactMock).toHaveBeenCalledWith("review-artifact-1");
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

    expect(await screen.findByTestId("agents-artifact-tab-review")).toBeInTheDocument();
    expect(await screen.findByText("PR Review")).toBeInTheDocument();

    getPrReviewContextMock.mockReturnValue(
      deferred<AgentWorkspacePrReviewContext>().promise,
    );
    rerender(pane("conversation-2"));

    expect(screen.queryByTestId("agents-artifact-tab-review")).not.toBeInTheDocument();
    expect(screen.queryByText("PR Review")).not.toBeInTheDocument();
  });

  it("persists pre-PR autofix preference while initial Auto Publish is off", async () => {
    renderPane("publish", workspace({ mode: "edit" }));

    expect(await screen.findByTestId("agents-auto-publish-switch")).not.toBeChecked();

    fireEvent.click(screen.getByTestId("agents-pr-autofix-switch"));

    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: false,
          autoMergeMethod: "squash",
        },
      )
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

    expect(await screen.findByTestId("agents-auto-publish-switch")).not.toBeChecked();

    fireEvent.click(screen.getByTestId("agents-pr-auto-merge-switch"));

    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenLastCalledWith(
        "conversation-1",
        {
          autoFixEnabled: true,
          autoMergeDesired: true,
          autoMergeMethod: "squash",
        },
      )
    );
  });

  it("confirms enabling pre-PR Auto Publish from the publish pane", async () => {
    setWorkspaceAutoPublishMock.mockImplementationOnce(
      async (conversationId: string, input: { autoPublishEnabled: boolean }) =>
        workspace({
          mode: "edit",
          conversationId,
          autoPublishInitialPrEnabled: input.autoPublishEnabled,
        })
    );
    renderPane("publish", workspace({ mode: "edit" }));

    fireEvent.click(await screen.findByTestId("agents-auto-publish-switch"));

    expect(setWorkspaceAutoPublishMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Enable Auto Publish",
      })
    );

    await waitFor(() =>
      expect(setWorkspaceAutoPublishMock).toHaveBeenCalledWith("conversation-1", {
        autoPublishEnabled: true,
      })
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

    fireEvent.click(await screen.findByTestId("agents-pr-autofix-switch"));

    await waitFor(() =>
      expect(setWorkspacePrSupervisionMock).toHaveBeenCalledWith("conversation-1", {
        autoFixEnabled: true,
        autoMergeDesired: false,
        autoMergeMethod: "squash",
      })
    );
  });

  it("opens Execution settings from PR automation tooltip actions", async () => {
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

    await user.hover(
      await screen.findByRole("button", { name: "About Autofix CI and Reviews" }),
    );
    const settingsActions = await screen.findAllByTestId(
      "agents-tooltip-settings-execution",
    );
    await user.click(settingsActions[0]);

    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({ section: "execution" });
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

    fireEvent.click(await screen.findByTestId("agents-auto-publish-switch"));

    expect(setWorkspaceAutoPublishMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Pause Auto Publish",
      })
    );

    await waitFor(() =>
      expect(setWorkspaceAutoPublishMock).toHaveBeenCalledWith("conversation-1", {
        autoPublishEnabled: false,
      })
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
    expect(screen.getByTestId("agents-publish-push-status-pill")).toHaveTextContent(
      "Conflicting",
    );
    await user.click(
      screen.getByRole("button", { name: "Resolve conflicts" }),
    );
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Resolve conflicts",
      }),
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1"),
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
    expect(screen.getByText(/Fetch and push use different auth modes/i)).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-switch-ssh")).toBeInTheDocument();
  });

  it("shows a GitHub PR sign-in action for all-SSH publish workspaces when gh is missing", () => {
    useGhAuthStatusMock.mockReturnValue({
      data: false,
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

    expect(screen.getByTestId("agents-artifact-tab-publish")).toBeInTheDocument();
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

    const publishButton = screen.getByTestId("agents-publish-confirm");
    expect(publishButton).toHaveTextContent("Commit & Publish");
    expect(publishButton).toBeEnabled();
    expect(
      screen.getByRole("switch", { name: "Autofix CI & Reviews" })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "GitHub auto-merge" })
    ).toBeInTheDocument();

    await user.click(publishButton);
    expect(publish).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog", {
      name: "Commit and publish workspace?",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "Commit & Publish" })
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
      )
    ).toHaveTextContent(
      "feature/agent-screen"
    );
    expect(screen.queryByTestId("agents-close-pr")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-actions-menu")).toBeEnabled();
    expect(screen.getByTestId("agents-update-from-base")).toBeEnabled();
    expect(screen.queryByTestId("agents-publish-confirm")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      })
    );
    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1")
    );

    await userEvent.click(screen.getByTestId("agents-publish-actions-menu"));
    await userEvent.click(await screen.findByTestId("agents-close-pr"));
    expect(closeWorkspacePrMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Close PR",
      })
    );
    await waitFor(() =>
      expect(closeWorkspacePrMock).toHaveBeenCalledWith("conversation-1")
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
    expect(screen.queryByTestId("agents-publish-confirm")).not.toBeInTheDocument();
    expect(getWorkspaceFreshnessMock).toHaveBeenCalledWith("conversation-1", {
      scope: "full",
    });

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      })
    );
    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1")
    );
    expect(publish).not.toHaveBeenCalled();
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

    await waitFor(() => expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"));
    await waitFor(() => expect(getSessionPlanMock).toHaveBeenCalledWith("session-1"));
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

    await waitFor(() => expect(getSessionPlanMock).toHaveBeenCalledWith("session-1"));
    await user.click(await screen.findByLabelText("Plan actions"));
    await user.click(screen.getByRole("menuitem", { name: /new conversation/i }));

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
    expect(useChatStore.getState().activeConversationIds["project:project-1"]).toBeNull();
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

    await waitFor(() => expect(getIdeationSessionMock).toHaveBeenCalledWith("session-1"));
    await waitFor(() => expect(getSessionPlanMock).toHaveBeenCalledWith("session-1"));
    expect(screen.queryByText("No plan yet")).not.toBeInTheDocument();
  });

  it("promotes a Plan workspace to Ideation before requesting proposals", async () => {
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

    const planContent = await screen.findByTestId("agents-artifact-content-plan");
    const createProposalsButton = await within(planContent).findByRole("button", {
      name: /Create Proposals/i,
    });
    switchAgentConversationModeMock.mockClear();
    sendAgentMessageMock.mockClear();

    await userEvent.click(
      createProposalsButton,
    );

    await waitFor(() =>
      expect(switchAgentConversationModeMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        mode: "ideation",
      })
    );
    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledWith(
        "ideation",
        "session-1",
        expect.stringContaining("Proceed to proposals"),
      )
    );
    expect(
      sendAgentMessageMock.mock.invocationCallOrder[0]!,
    ).toBeGreaterThan(switchAgentConversationModeMock.mock.invocationCallOrder[0]!);
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

    expect(await screen.findByTestId("agents-artifact-tab-plan")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-proposal")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-verification")).not.toBeInTheDocument();
  });

  it("shows the Proposals tab for a plan session once proposals exist", async () => {
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
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [
        {
          id: "proposal-1",
          sessionId: "session-1",
          title: "Gate proposal tab visibility",
          description: "Show the Proposals tab only when it has content.",
          category: "frontend",
          steps: ["Update shared tab helper"],
          acceptanceCriteria: ["Empty sessions do not show Proposals"],
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
      "plan",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    const proposalsTab = await screen.findByTestId("agents-artifact-tab-proposal");

    expect(proposalsTab).toBeInTheDocument();
    expect(within(proposalsTab).getByText("1")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-verification")).not.toBeInTheDocument();
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
      "proposal",
      workspace({
        mode: "plan",
        linkedIdeationSessionId: "session-1",
      }),
      vi.fn(),
      false,
      conversation(),
    );

    const planTab = await screen.findByTestId("agents-artifact-tab-plan");

    expect(screen.queryByTestId("agents-artifact-tab-proposal")).not.toBeInTheDocument();
    expect(
      planTab.querySelector("span[style='background: var(--accent-primary);']"),
    ).not.toBeNull();
    expect(useDependencyGraphMock).toHaveBeenLastCalledWith("");
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
      reasonSummary: "Multiple dependent work items need tracked review checkpoints.",
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

    expect(
      await screen.findByText(/Recommended: Create Proposals/i),
    ).toHaveTextContent("Both paths remain available");

    await userEvent.click(
      screen.getByRole("button", { name: /Implement Directly/i }),
    );

    await waitFor(() =>
      expect(switchAgentConversationModeMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        mode: "edit",
      }),
    );
    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledWith(
        "project",
        "project-1",
        expect.stringContaining("Implement the approved plan directly"),
        undefined,
        undefined,
        {
          conversationId: "conversation-1",
          suppressUserMessage: true,
        },
      ),
    );
    expect(sendAgentMessageMock.mock.calls[0]?.[2]).not.toContain(
      "do not create task proposals",
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Implementation started");
  });

  it("shows and disables Plan tab CTAs while the recommendation check is running", async () => {
    const assessment = deferred<null>();
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
    const verifyButton = screen.getByRole("button", { name: /Verify Plan/i });

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
        sessionFlow: "planning",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    const draftPlan = {
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
      }),
    );
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
      expect(confirmVerificationMock).toHaveBeenCalledWith("session-1", [
        "security-review",
      ]),
    );
    expect(onTabChange).toHaveBeenCalledWith("verification");
    expect(toastSuccessMock).toHaveBeenCalledWith("Plan verification started");
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
      expect(confirmVerificationMock).toHaveBeenCalledWith("session-1", [
        "security-review",
      ]),
    );
    expect(onTabChange).toHaveBeenCalledWith("verification");
    expect(toastSuccessMock).toHaveBeenCalledWith("Plan verification started");
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
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

    expect(await screen.findByText("Plan Approved")).toBeInTheDocument();
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

    expect(await screen.findByText("Plan Approved")).toBeInTheDocument();
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
      expect(getIdeationSessionMock).toHaveBeenCalledWith("session-focused")
    );
    expect(getIdeationSessionMock).not.toHaveBeenCalledWith("session-from-workspace");
    expect(useConversationMock).toHaveBeenCalledWith("conversation-1", {
      enabled: false,
      pageSize: 40,
    });
  });

  it("keeps the parent plan conversation focused when the verification tab opens", async () => {
    const onFocusVerificationSession = vi.fn();
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
        convertedAt: null,
        verificationStatus: "verified",
        verificationInProgress: false,
        gapScore: 0,
        inheritedPlanArtifactId: null,
        sessionPurpose: "general",
        acceptanceStatus: null,
      },
      proposals: [],
      messages: [],
    });
    getIdeationChildrenMock.mockResolvedValue([
      {
        id: "verification-old",
        projectId: "project-1",
        title: "Old verifier",
        titleSource: "auto",
        status: "active",
        planArtifactId: null,
        seedTaskId: null,
        parentSessionId: "session-1",
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
        sessionPurpose: "verification",
        acceptanceStatus: null,
      },
      {
        id: "verification-new",
        projectId: "project-1",
        title: "New verifier",
        titleSource: "auto",
        status: "active",
        planArtifactId: null,
        seedTaskId: null,
        parentSessionId: "session-1",
        teamMode: null,
        teamConfig: null,
        createdAt: "2026-04-23T10:00:00Z",
        updatedAt: "2026-04-23T10:00:00Z",
        archivedAt: null,
        convertedAt: null,
        verificationStatus: "unverified",
        verificationInProgress: false,
        gapScore: null,
        inheritedPlanArtifactId: null,
        sessionPurpose: "verification",
        acceptanceStatus: null,
      },
    ]);
    useVerificationStatusMock.mockReturnValue({
      data: {
        sessionId: "session-1",
        status: "verified",
        inProgress: false,
        gaps: [],
        rounds: [],
        roundDetails: [],
        runHistory: [],
      },
      isLoading: false,
    });

    renderPane(
      "verification",
      workspace({ mode: "ideation", linkedIdeationSessionId: "session-1" }),
      vi.fn(),
      false,
      conversation(),
      { onFocusVerificationSession },
    );

    await waitFor(() => expect(useVerificationStatusMock).toHaveBeenCalled());
    expect(getIdeationChildrenMock).not.toHaveBeenCalledWith(
      "session-1",
      "verification",
    );
    expect(onFocusVerificationSession).not.toHaveBeenCalled();
  });

  it("keeps only Plan visible until the attached ideation run has a plan", async () => {
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
    expect(screen.getByTestId("agents-artifact-tab-plan")).toBeInTheDocument();
    expect(screen.getByTestId("agent-plan-start-panel")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-verification")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-proposal")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-artifact-tab-tasks")).not.toBeInTheDocument();
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
      })
    );

    await waitFor(() => expect(publish).toHaveBeenCalledWith("conversation-1"));
  });

  it("blocks publish while PR supervision preferences are saving", async () => {
    const user = userEvent.setup();
    const supervisionDeferred = deferred<AgentConversationWorkspace>();
    setWorkspacePrSupervisionMock.mockReturnValueOnce(supervisionDeferred.promise);
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

    await user.click(
      screen.getByRole("switch", { name: "GitHub auto-merge" }),
    );

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

  it("turns the publish confirmation into dismissible progress after confirming", async () => {
    const publishDeferred = deferred<void>();
    const publish = vi.fn(() => publishDeferred.promise);

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      publish,
      false,
      conversation(),
    );

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Commit & Publish",
      }),
    );

    const progressDialog = await screen.findByRole("dialog", {
      name: "Publishing workspace",
    });
    expect(publish).toHaveBeenCalledWith("conversation-1");
    expect(
      within(progressDialog).getByTestId("agents-publish-dialog-pipeline"),
    ).toBeInTheDocument();
    expect(
      within(progressDialog).queryByText(
        "Progress is also available in Commit & Publish.",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(progressDialog).getByTestId("agents-publish-dialog-pipeline-steps")
        .className,
    ).toContain("repeat(auto-fit,minmax(9.5rem,1fr))");
    expect(
      within(progressDialog).getByTestId("agents-publish-dialog-step-checking"),
    ).toHaveTextContent("Check workspace");
    await waitFor(() =>
      expect(toastLoadingMock).toHaveBeenCalledWith(
        "Publishing workspace",
        expect.objectContaining({
          description: "Agent conversation • Check workspace • 0s",
          duration: Infinity,
          id: "agent-workspace-operation:conversation-1:publish",
        }),
      ),
    );
    const closeButton = within(progressDialog).getByTestId(
      "agents-publish-dialog-close",
    );
    expect(closeButton).toBeEnabled();

    fireEvent.click(closeButton);

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Publishing workspace" }),
      ).not.toBeInTheDocument();
    });
    expect(publish).toHaveBeenCalledTimes(1);

    await act(async () => {
      publishDeferred.resolve();
      await publishDeferred.promise;
    });
  });

  it("advances publish progress from durable events while the workspace status is stale", async () => {
    const publishDeferred = deferred<void>();
    const publish = vi.fn(() => publishDeferred.promise);
    listPublicationEventsMock.mockResolvedValue([
      {
        id: "event-describing",
        conversationId: "conversation-1",
        step: "describing",
        status: "started",
        summary: "Drafting pull request description",
        classification: null,
        createdAt: new Date().toISOString(),
      },
    ]);

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      publish,
      false,
      conversation(),
    );

    await waitFor(() => expect(listPublicationEventsMock).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("agents-publish-confirm")).toBeEnabled());
    fireEvent.click(screen.getByTestId("agents-publish-confirm"));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Commit & Publish",
      }),
    );

    const progressDialog = await screen.findByRole("dialog", {
      name: "Publishing workspace",
    });
    await waitFor(() => {
      expect(
        within(progressDialog)
          .getByTestId("agents-publish-dialog-step-describing")
          .querySelector(".animate-spin"),
      ).not.toBeNull();
    });
    expect(
      within(progressDialog)
        .getByTestId("agents-publish-dialog-step-checking")
        .querySelector(".animate-spin"),
    ).toBeNull();
    await waitFor(() =>
      expect(toastLoadingMock).toHaveBeenCalledWith(
        "Publishing workspace",
        expect.objectContaining({
          description: "Agent conversation • Draft PR description • 0s",
          duration: Infinity,
          id: "agent-workspace-operation:conversation-1:publish",
        }),
      ),
    );

    await act(async () => {
      publishDeferred.resolve();
      await publishDeferred.promise;
    });
  });

  it("starts a new publish dialog from checking when the workspace has a stale pushed status", async () => {
    const publishDeferred = deferred<void>();
    const publish = vi.fn(() => publishDeferred.promise);
    listPublicationEventsMock.mockResolvedValue([
      {
        id: "event-published",
        conversationId: "conversation-1",
        step: "published",
        status: "completed",
        summary: "Published pull request",
        classification: null,
        createdAt: new Date(Date.now() - 60_000).toISOString(),
      },
    ]);

    renderPane(
      "publish",
      workspace({ mode: "edit", publicationPushStatus: "pushed" }),
      publish,
      false,
      conversation(),
    );

    await waitFor(() => expect(listPublicationEventsMock).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("agents-publish-confirm")).toBeEnabled());
    fireEvent.click(screen.getByTestId("agents-publish-confirm"));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Commit & Publish",
      }),
    );

    const progressDialog = await screen.findByRole("dialog", {
      name: "Publishing workspace",
    });
    expect(
      within(progressDialog)
        .getByTestId("agents-publish-dialog-step-checking")
        .querySelector(".animate-spin"),
    ).not.toBeNull();
    expect(
      within(
        within(progressDialog).getByTestId("agents-publish-dialog-step-pushed"),
      ).getByText("6"),
    ).toBeInTheDocument();

    await act(async () => {
      publishDeferred.resolve();
      await publishDeferred.promise;
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

  it("closes publish progress and reports errors when publishing fails", async () => {
    const publishDeferred = deferred<void>();
    const publish = vi.fn(() => publishDeferred.promise);

    renderPane(
      "publish",
      workspace({ mode: "edit" }),
      publish,
      false,
      conversation(),
    );

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Commit & Publish",
      }),
    );

    await screen.findByRole("dialog", { name: "Publishing workspace" });
    await act(async () => {
      publishDeferred.reject(new Error("Publish failed"));
      await publishDeferred.promise.catch(() => undefined);
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith("Failed to publish branch", {
        closeButton: true,
        description: "Agent conversation • Publish failed",
        dismissible: true,
        duration: 12_000,
        id: "agent-workspace-operation:conversation-1:publish",
      });
      expect(
        screen.queryByRole("dialog", { name: "Publishing workspace" }),
      ).not.toBeInTheDocument();
    });
  });

  it("closes the publish progress dialog once publishing settles", async () => {
    const publishDeferred = deferred<void>();
    const publish = vi.fn(() => publishDeferred.promise);

    renderPane("publish", workspace({ mode: "edit" }), publish);

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Commit & Publish",
      }),
    );

    await screen.findByRole("dialog", { name: "Publishing workspace" });
    await act(async () => {
      publishDeferred.resolve();
      await publishDeferred.promise;
    });

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Publishing workspace" }),
      ).not.toBeInTheDocument();
    });
  });

  it("scopes active publish progress to the conversation that started publishing", async () => {
    const queryClient = createTestQueryClient();
    const publishDeferred = deferred<void>();
    const publish = vi.fn(() => publishDeferred.promise);
    const pane = (conversationId: string, isPublishingWorkspace: boolean) => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={0}>
          <div className="h-[480px]">
            <AgentsArtifactPane
              conversation={{
                ...conversation(),
                id: conversationId,
                title:
                  conversationId === "conversation-1"
                    ? "Publishing conversation"
                    : "Other conversation",
              }}
              workspace={workspace({
                conversationId,
                mode: "edit",
                branchName: `ralphx/demo/agent-${conversationId}`,
                worktreePath: `/tmp/ralphx/${conversationId}`,
              })}
              activeTab="publish"
              taskMode="graph"
              onTabChange={() => {}}
              onTaskModeChange={() => {}}
              onPublishWorkspace={publish}
              isPublishingWorkspace={isPublishingWorkspace}
              onClose={() => {}}
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    );

    const { rerender } = render(pane("conversation-1", false));

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Commit & Publish",
      }),
    );

    expect(await screen.findByRole("dialog", { name: "Publishing workspace" }))
      .toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pipeline")).toBeInTheDocument();

    rerender(pane("conversation-2", false));

    expect(
      screen.queryByRole("dialog", { name: "Publishing workspace" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-publish-pipeline")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-confirm")).toBeEnabled();

    rerender(pane("conversation-1", true));

    expect(await screen.findByRole("dialog", { name: "Publishing workspace" }))
      .toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-pipeline")).toBeInTheDocument();

    await act(async () => {
      publishDeferred.resolve();
      await publishDeferred.promise;
    });
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
    await waitFor(() => expect(publishButton).toHaveTextContent("Commit & Publish"));
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
    await waitFor(() => expect(publishButton).toHaveTextContent("PR is up to date"));
    expect(publishButton).toBeDisabled();
    await screen.findByText("1 changed file published for review.");
    expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Published changes");

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
    await waitFor(() => expect(publishButton).toHaveTextContent("PR is up to date"));
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
      expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent("1"),
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
      expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent("1"),
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
      expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent("1"),
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
    await waitFor(() => expect(publishButton).toHaveTextContent("Commit & Publish"));
    await screen.findByText("Review changes before publishing.");
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
    await waitFor(() => expect(publishButton).toHaveTextContent("Commit & Publish"));
    expect(publishButton).toBeEnabled();

    fireEvent.click(publishButton);
    expect(publish).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Commit & Publish",
      })
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

    fireEvent.click(await screen.findByTestId("agents-open-pr-url"));

    expect(openUrlMock).toHaveBeenCalledWith("https://github.com/mock/project/pull/78");
  });

  it("shows the PR link with readable URL in the compact metadata strip", async () => {
    renderPane(
      "publish",
      workspace({
        mode: "edit",
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
      }),
    );

    expect(screen.getByTestId("agents-publish-metadata-strip")).toBeInTheDocument();
    const prUrl = await screen.findByTestId("agents-open-pr-url");
    expect(prUrl).toHaveTextContent("PR #78");
    fireEvent.click(prUrl);

    expect(openUrlMock).toHaveBeenCalledWith("https://github.com/mock/project/pull/78");
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
    expect(screen.getAllByText("Project default (main)").length).toBeGreaterThan(0);
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
      baseBlockReason: "Saved base commit is not contained in the default branch",
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
    ).toHaveTextContent("Saved base commit is not contained in the default branch");
    expect(screen.queryByTestId("agents-publish-confirm")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-rebase-from-base")).toBeEnabled();
    expect(screen.queryByTestId("agents-review-changes")).not.toBeInTheDocument();

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
      baseBlockReason: "Saved base commit is not contained in the default branch",
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
    ).toHaveTextContent("Saved base commit is not contained in the default branch");
    expect(screen.getByTestId("agents-rebase-from-base")).toBeEnabled();

    await userEvent.click(screen.getByTestId("agents-rebase-from-base"));

    const dialog = await screen.findByRole("dialog", { name: "Rebase branch" });
    expect(within(dialog).getByTestId("agents-rebase-base-select")).toHaveTextContent(
      "Project default (main)",
    );
    expect(loadBranchBaseOptionsMock).toHaveBeenCalledWith(
      expect.objectContaining({
        workingDirectory: "/tmp/ralphx/conversation-1",
        includeAgentBranches: false,
      }),
    );

    await userEvent.click(within(dialog).getByTestId("agents-rebase-base-select"));
    await userEvent.click(await screen.findByText("release/0.8"));
    await userEvent.click(within(dialog).getByRole("button", { name: "Rebase branch" }));

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1", {
        kind: "local_branch",
        ref: "release/0.8",
        displayName: "release/0.8",
      }),
    );
    expect(publish).not.toHaveBeenCalled();
  });

  it("closes the Rebase branch dialog and shows a persistent elapsed toast while rebasing", async () => {
    const publish = vi.fn().mockResolvedValue(undefined);
    const updateDeferred = deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
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
      baseBlockReason: "Saved base commit is not contained in the default branch",
    });
    updateWorkspaceFromBaseMock.mockImplementation(() => updateDeferred.promise);

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
    await userEvent.click(within(dialog).getByTestId("agents-rebase-base-select"));
    await userEvent.click(await screen.findByText("release/0.8"));
    await userEvent.click(within(dialog).getByRole("button", { name: "Rebase branch" }));

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1", {
        kind: "local_branch",
        ref: "release/0.8",
        displayName: "release/0.8",
      }),
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
      "feature/agent-screen"
    );
    expect(screen.getByTestId("agents-publish-status-pill")).toHaveAttribute(
      "style",
      expect.stringContaining("border-color: var(--overlay-weak)"),
    );
    expect(screen.getByTestId("agents-publish-status-pill")).toHaveAttribute(
      "style",
      expect.stringContaining("color: var(--text-secondary)"),
    );
    expect(screen.getByTestId("agents-base-stale")).toHaveAttribute(
      "style",
      expect.stringContaining("border-color: var(--border-subtle)"),
    );
    expect(screen.getByTestId("agents-base-stale-icon")).toHaveAttribute(
      "style",
      expect.stringContaining("color: var(--status-warning)"),
    );
    expect(screen.getByTestId("agents-base-stale")).not.toHaveTextContent(
      "Update this workspace before publishing"
    );
    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      })
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1")
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
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1"),
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
    const updateDeferred = deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
    updateWorkspaceFromBaseMock.mockImplementation(() => updateDeferred.promise);
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
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1");
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
      const updateDeferred = deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
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
      updateWorkspaceFromBaseMock.mockImplementation(() => updateDeferred.promise);

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
        expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1"),
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
    const updateDeferred = deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
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
    updateWorkspaceFromBaseMock.mockImplementation(() => updateDeferred.promise);

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
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1"),
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
    const updateDeferred = deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
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
    updateWorkspaceFromBaseMock.mockImplementation(() => updateDeferred.promise);

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
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1"),
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
    const updateDeferred = deferred<Awaited<ReturnType<typeof updateWorkspaceFromBaseMock>>>();
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
    updateWorkspaceFromBaseMock.mockImplementation(() => updateDeferred.promise);
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
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1"),
    );

    unmount();
    await act(async () => {
      updateDeferred.reject(new Error("Merge conflicts detected"));
      await updateDeferred.promise.catch(() => undefined);
    });

    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith(
        "Repair started",
        {
          description: "Agent conversation • Merge conflicts detected",
          dismissible: true,
          duration: 8_000,
          id: "agent-workspace-operation:conversation-1:update-from-base",
        },
      ),
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
    updateWorkspaceFromBaseMock.mockRejectedValue(new Error("base update failed"));

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
      "feature/agent-screen"
    );
    getWorkspaceFreshnessMock.mockClear();

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Update branch",
      })
    );

    await waitFor(() =>
      expect(updateWorkspaceFromBaseMock).toHaveBeenCalledWith("conversation-1")
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
      })
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
    updateWorkspaceFromBaseMock.mockRejectedValue(new Error("Merge conflicts detected"));
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
      expect(getConversationWorkspaceMock).toHaveBeenCalledWith("conversation-1"),
    );
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith(
        "Repair started",
        {
          description: "Agent conversation • Merge conflicts detected",
          dismissible: true,
          duration: 8_000,
          id: "agent-workspace-operation:conversation-1:update-from-base",
        },
      ),
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
    expect(screen.queryByTestId("agents-update-from-base")).not.toBeInTheDocument();
    expect(
      screen.getByText(
        "PR #91 has been merged. By continuing this conversation, a new workspace branch will be created automatically."
      )
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

    expect(await screen.findByTestId("agents-publish-confirm")).toHaveTextContent(
      "Merged"
    );
    expect(screen.queryByTestId("agents-pr-supervision-status")).not.toBeInTheDocument();
    expect(screen.queryByText("PR supervision blocked")).not.toBeInTheDocument();
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

    const repairButton = await screen.findByTestId("agents-publish-repair-pending");
    expect(repairButton).toBeDisabled();
    expect(repairButton).toHaveTextContent("Repair pending");
    expect(screen.queryByTestId("agents-update-from-base")).not.toBeInTheDocument();

    updateWorkspaceFromBaseMock.mockClear();
    fireEvent.click(repairButton);

    expect(updateWorkspaceFromBaseMock).not.toHaveBeenCalled();
    expect(getWorkspaceFreshnessMock).not.toHaveBeenCalled();
  });

  it("shows repair diff buckets without loading normal workspace review", async () => {
    getWorkspaceReviewMock.mockRejectedValue(
      new Error("Agent conversation workspace is checked out at 'HEAD' instead of branch"),
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

    renderPane(
      "publish",
      workspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        publicationPushStatus: "needs_agent",
      }),
      vi.fn(),
      false,
      null,
      {},
      queryClient,
    );

    const repairState = await screen.findByTestId("agents-publish-repair-state");
    const actionbar = screen.getByTestId("agents-publish-actionbar");
    const metadataStrip = screen.getByTestId("agents-publish-metadata-strip");
    expect(repairState).toBeInTheDocument();
    expect(
      within(actionbar).getByText(/RalphX routed this workspace to the agent/),
    ).toBeInTheDocument();
    expect(
      within(repairState).queryByText("Repairing workspace"),
    ).not.toBeInTheDocument();
    expect(
      within(repairState).queryByText(/RalphX routed this workspace to the agent/),
    ).not.toBeInTheDocument();
    expect(screen.getAllByText(/RalphX routed this workspace to the agent/)).toHaveLength(
      1,
    );
    expect(screen.queryByTestId("agents-base-stale")).not.toBeInTheDocument();
    expect(
      within(metadataStrip).getByTestId("agents-publish-push-status-pill"),
    ).toHaveTextContent("Repair pending");
    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-repair-bucket-conflicted")).toHaveTextContent(
        "Conflicted: 1",
      ),
    );
    expect(screen.getByTestId("agents-publish-repair-bucket-unstaged")).toHaveTextContent(
      "Unstaged: 1 file",
    );
    expect(screen.getByTestId("agents-publish-repair-bucket-staged")).toHaveTextContent(
      "Staged: 1 file",
    );
    expect(screen.getByTestId("agents-publish-repair-conflicted-files")).toHaveTextContent(
      "frontend/src/App.tsx",
    );
    expect(screen.queryByText("Could not load workspace changes")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-pr-supervision-controls")).not.toBeInTheDocument();
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(getWorkspaceRepairSummaryMock).toHaveBeenCalledWith("conversation-1"),
    );
    expect(getWorkspaceRepairConflictDiffMock).not.toHaveBeenCalled();
    expect(getWorkspaceRepairUnstagedChangesMock).not.toHaveBeenCalled();
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
      expect(screen.getByTestId("agents-publish-repair-state-label")).toHaveTextContent(
        "Merge paused for repair",
      ),
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
      expect(screen.getByTestId("agents-publish-repair-state-label")).toHaveTextContent(
        "Branch ready for repair",
      ),
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
      expect(screen.getByTestId("agents-publish-repair-state-label")).toHaveTextContent(
        "Repair state detected",
      ),
    );
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

    await waitFor(() => expect(screen.getByTestId("agents-review-changes")).toBeEnabled());
    fireEvent.click(screen.getByTestId("agents-review-changes"));
    await waitFor(() =>
      expect(getWorkspaceReviewMock).toHaveBeenCalledWith("conversation-1")
    );
    await user.click(
      await screen.findByTestId("tab-history", undefined, deferredHydrationTimeout)
    );

    expect(
      await screen.findByTestId("commit-abc123d", undefined, deferredHydrationTimeout)
    ).toHaveTextContent("Update Codex model catalog");
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

  it("shows the PR description drafting step while publishing", () => {
    renderPane(
      "publish",
      workspace({ mode: "edit", publicationPushStatus: "describing" }),
      vi.fn(),
      true,
    );

    expect(screen.getByTestId("agents-publish-pipeline")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-step-describing")).toHaveTextContent(
      "Draft PR description"
    );
  });

  it("shows description failure without opening a pull request", () => {
    renderPane(
      "publish",
      workspace({ mode: "edit", publicationPushStatus: "description_failed" }),
    );

    expect(screen.getByTestId("agents-publish-pipeline")).toBeInTheDocument();
    expect(screen.getByText(/retry Commit & Publish/i)).toBeInTheDocument();
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
    expect(screen.getByTestId("agents-publish-step-auto_merge")).toHaveTextContent(
      "Auto-merge deferred",
    );
    expect(screen.queryByText(/latest publish attempt failed/i)).not.toBeInTheDocument();
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

    expect(screen.queryByTestId("agents-publish-pipeline")).not.toBeInTheDocument();
    expect(screen.getByText("Monitoring PR")).toBeInTheDocument();
  });

  it("shows synced GitHub PR annotation count for published workspaces", async () => {
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
        expect(screen.getByTestId("agents-pr-annotations-summary")).toHaveTextContent(
          "1 GitHub annotation synced",
        ),
      deferredHydrationTimeout,
    );
    expect(getWorkspacePrAnnotationsMock).toHaveBeenCalledWith("conversation-1");
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
        expect(screen.getByTestId("agents-pr-annotations-summary")).toHaveTextContent(
          "GitHub annotations partially unavailable",
        ),
      deferredHydrationTimeout,
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

    expect(
      await screen.findByTestId(
        "agents-publish-events",
        undefined,
        deferredHydrationTimeout,
      )
    ).toBeInTheDocument();
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

    expect(
      await screen.findByTestId(
        "agents-publish-events",
        undefined,
        deferredHydrationTimeout,
      )
    ).toBeInTheDocument();
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

    expect(
      await screen.findByTestId(
        "agents-publish-events",
        undefined,
        deferredHydrationTimeout,
      )
    ).toBeInTheDocument();
    expect(screen.queryByText("Checking workspace changes")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agents-publish-history-toggle"));
    expect(screen.queryByText("Checking workspace changes")).not.toBeInTheDocument();
    expect(screen.getByText("Pushing agent branch")).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-event-icon-event-pushing"))
      .toHaveAttribute("data-state", "active");
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
    expect(screen.queryByRole("button", { name: /Approve Plan/i })).not.toBeInTheDocument();
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
      expect(getIdeationSessionMock).toHaveBeenCalledWith("cloned-session-draft"),
    );

    expect(
      await screen.findByRole("button", { name: /Approve Plan/i }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Create Proposals/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Implement Directly/i })).not.toBeInTheDocument();
  });
});
