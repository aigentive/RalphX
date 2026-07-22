import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AutomationsView } from "./AutomationsView";
import type { Automation, AutomationRun } from "@/api/automations";

const { listAutomationsMock, getAutomationMock, preloadAutomationDetailViewMock } = vi.hoisted(() => ({
  listAutomationsMock: vi.fn(),
  getAutomationMock: vi.fn(),
  preloadAutomationDetailViewMock: vi.fn(() => new Promise(() => {})),
}));

vi.mock("@/api/automations", () => ({
  automationsApi: {
    list: listAutomationsMock,
    get: getAutomationMock,
  },
}));

vi.mock("@/components/automations/preloadAutomationDetailView", () => ({
  preloadAutomationDetailView: preloadAutomationDetailViewMock,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function renderView(props: Partial<ComponentProps<typeof AutomationsView>> = {}) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <AutomationsView
        projectId="project-1"
        projectName="Demo Project"
        {...props}
      />
    </QueryClientProvider>,
  );
}

function automation(overrides: Partial<Automation> = {}): Automation {
  const now = "2026-07-05T00:00:00Z";
  return {
    id: "automation-1",
    projectId: "project-1",
    name: "Ship migration loop",
    status: "active",
    pausedReasonCode: null,
    pausedReasonDetail: null,
    goalPrompt: "Keep landing migration tasks.",
    setupConversationId: "conversation-1",
    specArtifactId: null,
    providerHarness: "codex",
    modelId: "gpt-5.4",
    logicalEffort: "high",
    runMode: "edit",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "main",
    baseSourcePullRequestJson: null,
    goalItemsJson: null,
    chainMode: "merged_base",
    completionSignal: "pr_merged",
    planApprovalMode: "manual",
    prMergeMode: "manual",
    planDeepVerification: false,
    maxRuns: 25,
    maxConsecutiveFailures: 3,
    firstRunPrompt: null,
    setupAnalysisSummary: null,
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
    runIndex: 1,
    status: "merged",
    judgeState: "done",
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
    runPrompt: "Continue the automation.",
    promptAuthor: "judge",
    baseRefKind: "project_default",
    baseRefUsed: "main",
    baseFromRunId: null,
    goalItemId: null,
    branchName: "ralphx/test",
    prNumber: null,
    prUrl: null,
    prTitle: null,
    prHeadRefName: null,
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
    startedAt: now,
    finishedAt: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

const emptyUsage = {
  inputTokens: 0,
  outputTokens: 0,
  cacheCreationTokens: 0,
  cacheReadTokens: 0,
  estimatedUsd: null,
};

describe("AutomationsView", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "requestAnimationFrame",
      (cb: FrameRequestCallback): number =>
        window.setTimeout(() => cb(performance.now()), 0),
    );
    vi.stubGlobal("cancelAnimationFrame", (handle: number): void => {
      window.clearTimeout(handle);
    });
    listAutomationsMock.mockReset();
    getAutomationMock.mockReset();
    preloadAutomationDetailViewMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("paints skeleton rows before enabling or resolving the list query", async () => {
    const pendingList = deferred<Automation[]>();
    listAutomationsMock.mockReturnValue(pendingList.promise);

    renderView();

    expect(screen.getByTestId("automations-list-skeleton")).toBeInTheDocument();
    expect(listAutomationsMock).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(listAutomationsMock).toHaveBeenCalledWith({ projectId: "project-1" }),
    );
    expect(screen.getByTestId("automations-list-skeleton")).toBeInTheDocument();

    await act(async () => {
      pendingList.resolve([]);
      await pendingList.promise;
    });

    expect(await screen.findByTestId("automations-empty-state")).toBeInTheDocument();
  });

  it("renders project-scoped automation rows from the list API", async () => {
    const item = automation({
      firstRunPrompt: "Start with dependency updates.",
      goalItemsJson:
        '[{"id":"phase-1","title":"Update dependencies","status":"pending"},{"id":"phase-2","title":"Verify CI","status":"pending"}]',
    });
    listAutomationsMock.mockResolvedValue([item]);
    getAutomationMock.mockResolvedValue({ automation: item, runs: [], usage: emptyUsage });

    renderView();

    const row = await screen.findByTestId("automation-row-automation-1");
    expect(row).toBeInTheDocument();
    expect(within(row).getByText("Ship migration loop")).toBeInTheDocument();
    expect(within(row).getByText("Demo Project")).toBeInTheDocument();
    expect(row).toHaveTextContent("edit · codex/gpt-5.4/high");
    expect(screen.getByTestId("automation-row-automation-1-metadata")).toHaveTextContent(
      "Goal set · 2 phases · First run ready",
    );
  });

  it("renders a project selector in the header and reports project changes", async () => {
    const onProjectChange = vi.fn();
    listAutomationsMock.mockResolvedValue([]);

    renderView({
      projectOptions: [
        { id: "project-1", name: "Demo Project" },
        { id: "project-2", name: "Ops Project" },
      ],
      onProjectChange,
    });

    const selector = screen.getByTestId("automations-project-select");
    expect(selector).toHaveValue("project-1");

    await userEvent.selectOptions(selector, "project-2");

    expect(onProjectChange).toHaveBeenCalledWith("project-2");
  });

  it("paints the detail shell synchronously on row click before the detail bundle resolves", async () => {
    listAutomationsMock.mockResolvedValue([automation()]);
    getAutomationMock.mockResolvedValue({ automation: automation(), runs: [], usage: emptyUsage });

    renderView();

    await userEvent.click(await screen.findByTestId("automation-row-automation-1"));

    expect(screen.getByTestId("automation-detail-shell")).toBeInTheDocument();
    expect(preloadAutomationDetailViewMock).toHaveBeenCalled();
  });

  it("renders an empty disabled state without a selected project", () => {
    const onNewAutomation = vi.fn();

    renderView({ projectId: null, projectName: null, onNewAutomation });

    expect(screen.getByTestId("automations-empty-state")).toBeInTheDocument();
    expect(screen.getByTestId("automations-new-button")).toBeDisabled();
    expect(screen.getByTestId("automations-empty-new-button")).toBeDisabled();
    expect(listAutomationsMock).not.toHaveBeenCalled();
  });

  it("renders an error state when the project automation list fails", async () => {
    listAutomationsMock.mockRejectedValue(new Error("boom"));

    renderView();

    expect(await screen.findByTestId("automations-error-state")).toBeInTheDocument();
    expect(screen.getByText("Could not load automations.")).toBeInTheDocument();
  });

  it("uses controlled selected automation state and reports back navigation", async () => {
    const onSelectedAutomationChange = vi.fn();

    renderView({
      selectedAutomationId: "automation-1",
      onSelectedAutomationChange,
    });

    expect(screen.getByTestId("automation-detail-shell")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Back" }));

    expect(onSelectedAutomationChange).toHaveBeenCalledWith(null);
  });

  it("summarizes all automation statuses and next-action branches", async () => {
    const rows = [
      automation({ id: "draft", name: "Draft automation", status: "draft", logicalEffort: null }),
      automation({ id: "paused", name: "Paused automation", status: "paused", pausedReasonCode: "review_gate" }),
      automation({ id: "completed", name: "Completed automation", status: "completed" }),
      automation({ id: "stopped", name: "Stopped automation", status: "stopped" }),
      automation({ id: "empty-active", name: "Empty active automation", baseDisplayName: null, baseRef: "", baseRefKind: "current_branch" }),
      automation({ id: "judging", name: "Judging automation" }),
      automation({ id: "judge-failed", name: "Judge failed automation" }),
      automation({ id: "running", name: "Running automation" }),
      automation({ id: "awaiting-plan", name: "Awaiting plan automation" }),
      automation({ id: "published-no-pr", name: "Published without PR automation" }),
      automation({ id: "published-with-pr", name: "Published with PR automation" }),
      automation({ id: "waiting-judge", name: "Waiting judge automation" }),
      automation({ id: "scheduling", name: "Scheduling automation" }),
    ];
    const runsByAutomation = new Map<string, AutomationRun[]>([
      ["judging", [run({ automationId: "judging", judgeState: "in_progress" })]],
      ["judge-failed", [run({ automationId: "judge-failed", judgeState: "failed" })]],
      ["running", [run({ automationId: "running", runIndex: 2, status: "running", judgeState: "none" })]],
      ["awaiting-plan", [run({ automationId: "awaiting-plan", runIndex: 2, status: "awaiting_plan_approval", judgeState: "none" })]],
      ["published-no-pr", [run({ automationId: "published-no-pr", status: "published", judgeState: "none" })]],
      ["published-with-pr", [run({ automationId: "published-with-pr", status: "published", judgeState: "none", prNumber: 593 })]],
      ["waiting-judge", [run({ automationId: "waiting-judge", status: "merged", judgeState: "none" })]],
      ["scheduling", [run({ automationId: "scheduling", status: "merged", judgeState: "done" })]],
    ]);

    listAutomationsMock.mockResolvedValue(rows);
    getAutomationMock.mockImplementation((id: string) => Promise.resolve({
      automation: rows.find((item) => item.id === id) ?? automation({ id }),
      runs: runsByAutomation.get(id) ?? [],
      usage: emptyUsage,
    }));

    renderView({ onOpenAutomation: vi.fn() });

    expect(await screen.findByText("Draft automation")).toBeInTheDocument();
    expect(await screen.findByText("Draft setup")).toBeInTheDocument();
    expect(screen.getByText("Paused: review_gate")).toBeInTheDocument();
    expect(screen.getByText("Goal completed")).toBeInTheDocument();
    expect(screen.getAllByText("Stopped").length).toBeGreaterThan(0);
    expect(screen.getByText("Waiting for first run")).toBeInTheDocument();
    expect(screen.getByText("Terminal judge running")).toBeInTheDocument();
    expect(screen.getByText("Terminal judge failed")).toBeInTheDocument();
    expect(screen.getByText("Run 2 in progress")).toBeInTheDocument();
    expect(
      within(screen.getByTestId("automation-row-awaiting-plan")).getByText(
        "Run 2 Awaiting plan approval",
      ),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("automation-row-awaiting-plan")).getByText(
        "Awaiting plan approval",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Waiting for PR merge")).toBeInTheDocument();
    expect(screen.getByText("Waiting for PR #593 to merge")).toBeInTheDocument();
    expect(screen.getByText("Terminal judge pending")).toBeInTheDocument();
    expect(screen.getByText("Scheduling next run")).toBeInTheDocument();
    expect(screen.getByText("current_branch")).toBeInTheDocument();
    expect(screen.getAllByText("0 / 25").length).toBeGreaterThan(0);
  });
});
