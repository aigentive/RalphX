import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Automation, AutomationDetail, AutomationRun } from "@/api/automations";
import { AgentsAutomationPanel } from "./AgentsAutomationPanel";

const {
  useAutomationDetailMock,
  useAutomationEventsMock,
  pauseAutomationMock,
  resumeAutomationMock,
  stopAutomationMock,
} = vi.hoisted(() => ({
  useAutomationDetailMock: vi.fn(),
  useAutomationEventsMock: vi.fn(),
  pauseAutomationMock: vi.fn(),
  resumeAutomationMock: vi.fn(),
  stopAutomationMock: vi.fn(),
}));

vi.mock("@/components/agents/agentDeferredFrame", () => ({
  useAfterPaintMounted: () => true,
}));

vi.mock("@/hooks/useAutomations", () => ({
  invalidateAutomationQueries: vi.fn(),
  useAutomationDetail: (...args: unknown[]) => useAutomationDetailMock(...args),
  useAutomationEvents: (...args: unknown[]) => useAutomationEventsMock(...args),
}));

vi.mock("@/api/automations", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/automations")>();
  return {
    ...actual,
    automationsApi: {
      ...actual.automationsApi,
      pause: (...args: unknown[]) => pauseAutomationMock(...args),
      resume: (...args: unknown[]) => resumeAutomationMock(...args),
      stop: (...args: unknown[]) => stopAutomationMock(...args),
    },
  };
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
  conversationId: "conversation-1",
  runPrompt: "Continue the release automation.",
  promptAuthor: "judge",
  baseRefKind: "project_default",
  baseRefUsed: "main",
  baseFromRunId: "run-0",
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

function renderPanel(onOpenAutomation = vi.fn()) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  render(
    <QueryClientProvider client={queryClient}>
      <AgentsAutomationPanel
        automationId="automation-1"
        onOpenAutomation={onOpenAutomation}
      />
    </QueryClientProvider>,
  );

  return { onOpenAutomation };
}

describe("AgentsAutomationPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture(),
      isLoading: false,
      isError: false,
    });
  });

  it("shows automation summary controls and opens the automation detail", () => {
    const { onOpenAutomation } = renderPanel();

    expect(screen.getByTestId("agents-automation-panel")).toBeInTheDocument();
    expect(screen.getByText("Release automation")).toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.getByText("3 of 25")).toBeInTheDocument();
    expect(screen.getByText(/PR #593/)).toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-pause")).toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-stop")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-automation-open"));

    expect(onOpenAutomation).toHaveBeenCalledWith("automation-1");
  });
});
