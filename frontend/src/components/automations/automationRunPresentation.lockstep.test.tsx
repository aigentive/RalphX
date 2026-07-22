import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Automation, AutomationDetail, AutomationRun } from "@/api/automations";
import { AgentsAutomationPanel } from "@/components/agents/AgentsAutomationPanel";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AutomationDetailView } from "./AutomationDetailView";
import { AutomationRunStatusHeader } from "./AutomationRunStatusHeader";
import { getRunCardBadges } from "./automationRunBadges";
import { getAutomationRunView } from "./automationRunView";

const {
  useAutomationDetailMock,
  useAutomationEventsMock,
  useArtifactMock,
  useAskUserQuestionMock,
  listConversationTasksMock,
  openExternalUrlMock,
  toastSuccessMock,
  toastErrorMock,
} = vi.hoisted(() => ({
  useAutomationDetailMock: vi.fn(),
  useAutomationEventsMock: vi.fn(),
  useArtifactMock: vi.fn(),
  useAskUserQuestionMock: vi.fn(),
  listConversationTasksMock: vi.fn(),
  openExternalUrlMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  toastErrorMock: vi.fn(),
}));

vi.mock("@/components/agents/agentDeferredFrame", () => ({
  useAfterPaintMounted: () => true,
}));

vi.mock("@/hooks/useArtifacts", () => ({
  useArtifact: (...args: unknown[]) => useArtifactMock(...args),
}));

vi.mock("@/hooks/useAutomations", () => ({
  evictDeletedAutomation: vi.fn(),
  invalidateAutomationQueries: vi.fn(),
  useAutomationDetail: (...args: unknown[]) => useAutomationDetailMock(...args),
  useAutomationEvents: (...args: unknown[]) => useAutomationEventsMock(...args),
}));

vi.mock("@/hooks/useAskUserQuestion", () => ({
  useAskUserQuestion: (...args: unknown[]) => useAskUserQuestionMock(...args),
}));

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: () => ({
    registry: {
      claude: [
        {
          id: "sonnet",
          label: "sonnet",
          menuLabel: "sonnet",
          defaultEffort: "medium",
          supportedEfforts: ["low", "medium", "high", "max"],
        },
      ],
      codex: [
        {
          id: "gpt-5.5",
          label: "gpt-5.5",
          menuLabel: "gpt-5.5",
          defaultEffort: "xhigh",
          supportedEfforts: ["low", "medium", "high", "xhigh"],
        },
      ],
    },
  }),
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: () => ({
    providers: [
      {
        provider: "codex",
        enabled: true,
        available: true,
        cliVersion: "1.2.3",
        status: "ready",
        error: null,
        missingCoreExecFeatures: [],
        supportedModelAliases: ["gpt-5.5", "gpt-5.4"],
        supportedEfforts: ["low", "medium", "high", "xhigh"],
      },
    ],
    isLoading: false,
    isPlaceholderData: false,
  }),
}));

vi.mock("@/api/agent-tasks", () => ({
  agentTaskApi: {
    listConversationTasks: (...args: unknown[]) => listConversationTasksMock(...args),
  },
}));

vi.mock("@/api/automations", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/automations")>();
  return {
    ...actual,
    automationsApi: {
      ...actual.automationsApi,
      pause: vi.fn(),
      resume: vi.fn(),
      finalize: vi.fn(),
      stop: vi.fn(),
      triggerRunNow: vi.fn(),
      skipJudge: vi.fn(),
      delete: vi.fn(),
      cancelRun: vi.fn(),
      updateSettings: vi.fn(),
      setupAgent: {
        ...actual.automationsApi.setupAgent,
        updateAutomation: vi.fn(),
      },
    },
  };
});

vi.mock("@/lib/open-external", () => ({
  openExternalUrl: (...args: unknown[]) => openExternalUrlMock(...args),
}));

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

function automation(overrides: Partial<Automation> = {}): Automation {
  const now = "2026-07-05T00:00:00Z";
  return {
    id: "automation-1",
    projectId: "project-1",
    name: "Release automation",
    status: "active",
    pausedReasonCode: null,
    pausedReasonDetail: null,
    goalPrompt: "Ship the remaining release tasks.",
    setupConversationId: "conversation-setup",
    specArtifactId: null,
    providerHarness: "codex",
    modelId: "gpt-5.4",
    logicalEffort: "medium",
    runMode: "edit",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "main",
    baseSourcePullRequestJson: null,
    goalItemsJson:
      '[{"id":"phase-1","title":"Build shared context model","status":"in_progress"}]',
    chainMode: "merged_base",
    completionSignal: "pr_merged",
    planApprovalMode: "manual",
    prMergeMode: "manual",
    planDeepVerification: false,
    maxRuns: 25,
    maxConsecutiveFailures: 3,
    firstRunPrompt: "Build the shared context model in a scoped PR.",
    setupAnalysisSummary: "Configure selected artifact context for chat.",
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function run(overrides: Partial<AutomationRun> = {}): AutomationRun {
  const now = "2026-07-05T00:00:00Z";
  return {
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
    planArtifactId: "plan-artifact-1",
    planApprovedBy: null,
    planApprovedArtifactVersion: null,
    planApprovedAt: null,
    conversationId: "conversation-run",
    runPrompt: "Continue the release automation.",
    promptAuthor: "judge",
    baseRefKind: "project_default",
    baseRefUsed: "main",
    baseFromRunId: null,
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
    agentSummary: "Implemented the run.",
    judgeVerdictJson: null,
    judgeModelId: null,
    errorCode: null,
    errorDetail: null,
    signalCheckFailures: 0,
    startedAt: now,
    finishedAt: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

const usage = {
  inputTokens: 0,
  outputTokens: 0,
  cacheCreationTokens: 0,
  cacheReadTokens: 0,
  estimatedUsd: null,
};

function detailFixture(
  candidate: AutomationRun,
  automationOverrides: Partial<Automation> = {},
): AutomationDetail {
  return {
    automation: automation(automationOverrides),
    runs: [candidate],
    usage,
  };
}

function renderWithClient(ui: ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{ui}</TooltipProvider>
    </QueryClientProvider>,
  );
}

function textByTestId(container: HTMLElement, testId: string): string {
  return within(container).getByTestId(testId).textContent ?? "";
}

function summaryValue(container: HTMLElement, testId: string, label: string): string {
  const text = textByTestId(container, testId);
  expect(text.startsWith(label)).toBe(true);
  return text.slice(label.length);
}

interface RunPresentation {
  status: string;
  stage: string;
  pr: string;
}

function assertPresentationMatchesSelector(
  presentation: RunPresentation,
  expected: RunPresentation,
) {
  expect(new Set(Object.values(presentation))).toEqual(
    new Set(Object.values(expected)),
  );
  expect(presentation).toEqual(expected);
}

function collectRunPresentation(detail: AutomationDetail) {
  const candidate = detail.runs[0];
  if (!candidate) {
    throw new Error("presentation fixture needs one run");
  }
  const view = getAutomationRunView(detail.automation, candidate);
  const expected: RunPresentation = {
    status: view.statusLabel,
    stage: view.stageLabel,
    pr: view.pr.value,
  };

  useAutomationDetailMock.mockReturnValue({
    data: detail,
    isLoading: false,
    isError: false,
  });
  const compactRender = renderWithClient(
    <AgentsAutomationPanel automationId={detail.automation.id} />,
  );
  const compact: RunPresentation = {
    status: textByTestId(
      compactRender.container,
      `agents-automation-run-${candidate.runIndex}-status`,
    ),
    stage: summaryValue(
      compactRender.container,
      "agents-automation-stage",
      "Stage",
    ),
    pr: summaryValue(compactRender.container, "agents-automation-pr", view.pr.rowLabel),
  };
  compactRender.unmount();

  useAutomationDetailMock.mockReturnValue({
    data: detail,
    isLoading: false,
    isError: false,
  });
  const detailRender = renderWithClient(
    <AutomationDetailView
      automationId={detail.automation.id}
      projectId={detail.automation.projectId}
      projectName="RalphX"
      onBack={vi.fn()}
      onOpenRunConversation={vi.fn()}
      onOpenAutomationRun={vi.fn()}
    />,
  );
  // Timeline lives behind the page-level Runs tab (deferred mount is mocked
  // synchronous via useAfterPaintMounted above). Radix tab triggers activate
  // on mousedown, so fire that alongside click.
  const runsTabTrigger = within(detailRender.container).getByTestId(
    "automation-tab-runs",
  );
  fireEvent.mouseDown(runsTabTrigger);
  fireEvent.click(runsTabTrigger);
  // The timeline card renders the de-duplicated badge contract instead of the
  // raw status/stage pair: assert its badges match `getRunCardBadges` exactly.
  const expectedBadges = getRunCardBadges(detail.automation, candidate);
  for (const key of ["status", "judge", "stage"] as const) {
    const badge = expectedBadges.find((entry) => entry.key === key);
    const rendered = within(detailRender.container).queryByTestId(
      `automation-run-${candidate.id}-header-${key}`,
    );
    if (badge) {
      expect(rendered?.textContent, `timeline ${key} badge`).toBe(badge.label);
    } else {
      expect(rendered, `timeline ${key} badge suppressed`).toBeNull();
    }
  }
  const timeline: RunPresentation = {
    status: textByTestId(
      detailRender.container,
      `automation-run-${candidate.id}-header-status`,
    ),
    stage: expected.stage,
    pr: textByTestId(
      detailRender.container,
      `automation-run-${candidate.id}-pr-state`,
    ),
  };
  detailRender.unmount();

  const bannerRender = renderWithClient(
    <AutomationRunStatusHeader
      automation={detail.automation}
      run={candidate}
      density="banner"
      testId="run-conversation-banner"
      message="Automation run conversations are read-only while the automation is working on this run."
    />,
  );
  const runConversation: RunPresentation = {
    status: textByTestId(bannerRender.container, "run-conversation-banner-status"),
    stage: textByTestId(bannerRender.container, "run-conversation-banner-stage"),
    pr: textByTestId(bannerRender.container, "run-conversation-banner-pr"),
  };
  bannerRender.unmount();

  return { expected, compact, timeline, runConversation };
}

describe("automation run presentation lockstep", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useArtifactMock.mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
    });
    useAskUserQuestionMock.mockReturnValue({
      activeQuestion: null,
      answeredQuestion: undefined,
      submitAnswer: vi.fn(),
      dismissQuestion: vi.fn(),
      clearAnswered: vi.fn(),
      isLoading: false,
    });
    listConversationTasksMock.mockResolvedValue([]);
  });

  afterEach(() => {
    cleanup();
  });

  it.each([
    [
      "cancelled+open-PR",
      detailFixture(run({ id: "cancelled-open-pr", status: "cancelled" })),
    ],
    [
      "merged+judge-pending",
      detailFixture(
        run({
          id: "merged-judge-pending",
          status: "merged",
          judgeState: "in_progress",
          prMergedAt: "2026-07-05T01:00:00Z",
          mergeCommitSha: "abc123",
        }),
      ),
    ],
    [
      "parked+judging",
      detailFixture(
        run({
          id: "parked-judging",
          status: "awaiting_plan_approval",
          planJudgeState: "in_progress",
        }),
      ),
    ],
    [
      "parked+approved-awaiting-delivery",
      detailFixture(
        run({
          id: "parked-approved",
          status: "awaiting_plan_approval",
          planApprovedBy: "user",
          planApprovedArtifactVersion: 2,
          planApprovedAt: "2026-07-05T01:00:00Z",
        }),
      ),
    ],
    [
      "completed-unjudged",
      detailFixture(run({ id: "completed-unjudged", status: "completed" })),
    ],
    [
      "judge-failed",
      detailFixture(
        run({
          id: "judge-failed",
          status: "merged",
          judgeState: "failed",
        }),
      ),
    ],
    ["published", detailFixture(run({ id: "published", status: "published" }))],
    [
      "cancel-vs-chip split",
      detailFixture(
        run({
          id: "cancel-vs-chip",
          status: "cancelled",
          judgeState: "in_progress",
        }),
      ),
    ],
    [
      "cancel-race workspace-merged",
      detailFixture(
        run({
          id: "cancel-race",
          status: "cancelled",
          judgeState: "none",
          prMergedAt: "2026-07-05T01:00:00Z",
          mergeCommitSha: "abc123",
        }),
      ),
    ],
  ] satisfies Array<[string, AutomationDetail]>)(
    "keeps compact, timeline, and run banner presentation aligned for %s",
    (_name, detail) => {
      const presentations = collectRunPresentation(detail);

      assertPresentationMatchesSelector(presentations.compact, presentations.expected);
      assertPresentationMatchesSelector(presentations.timeline, presentations.expected);
      assertPresentationMatchesSelector(
        presentations.runConversation,
        presentations.expected,
      );
    },
  );

  it("keeps the cancel-race story status-neutral across every surface", () => {
    const presentations = collectRunPresentation(
      detailFixture(
        run({
          id: "cancel-race",
          status: "cancelled",
          judgeState: "none",
          prMergedAt: "2026-07-05T01:00:00Z",
          mergeCommitSha: "abc123",
        }),
      ),
    );

    for (const presentation of [
      presentations.compact,
      presentations.timeline,
      presentations.runConversation,
    ]) {
      expect(Object.values(presentation).join(" ")).not.toContain("Current PR");
      expect(Object.values(presentation).join(" ")).not.toContain(
        "Waiting for judge",
      );
      expect(presentation).toEqual({
        status: "Cancelled",
        stage: "Cancelled",
        pr: "PR #593 on cancelled run",
      });
    }
  });
});
