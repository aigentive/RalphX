import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AutomationsView } from "./AutomationsView";
import type { Automation, AutomationRun } from "@/api/automations";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

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
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
      effectiveScopes: {},
      connectionPresentations: {},
    });
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

    const skeleton = screen.getByTestId("automations-list-skeleton");
    expect(skeleton).toBeInTheDocument();
    expect(skeleton.firstElementChild).toHaveClass("min-h-[64px]");
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

  // Wave B4: create_automation_draft has a registered intent twin
  // (request_remote_automation_draft), so a paired ui:agent client creates drafts like the
  // host does — the old disabled-remotely presentation is retired.
  it("enables new automation remotely now that the draft twin registers", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote-1",
      environments: [{ id: "remote-1", name: "Studio", kind: "remote" }],
      effectiveScopes: { "remote-1": ["ui:read", "ui:operate", "ui:agent"] },
      connectionPresentations: {
        "remote-1": { presentation: "connected", blockedFailure: null, blockedMessage: null },
      },
    });
    listAutomationsMock.mockResolvedValue([]);
    const onNewAutomation = vi.fn();

    renderView({ onNewAutomation });

    const button = await screen.findByTestId("automations-new-button");
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(onNewAutomation).toHaveBeenCalledTimes(1);
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
    expect(
      screen.getByText("Demo Project · 1 automations · 1 running · 0 needs attention"),
    ).toBeInTheDocument();
    expect(within(row).queryByText("Demo Project")).not.toBeInTheDocument();
    expect(row).toHaveTextContent("2 phases · edit · gpt-5.4");
    expect(row).not.toHaveTextContent("codex/");
    expect(row).not.toHaveTextContent("/high");
    expect(screen.getByTestId("automation-row-automation-1-metadata")).toHaveTextContent(
      "Waiting for first run · 2 phases · edit · gpt-5.4",
    );
    expect(within(row).getByText("Active").closest("[data-tone]")).toHaveAttribute(
      "data-tone",
      "accent",
    );
    expect(screen.queryByText("PROJECT")).not.toBeInTheDocument();
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

  it("filters priority groups and searches by automation name without changing pill counts", async () => {
    const rows = [
      automation({ id: "paused", name: "Review alerts", status: "paused" }),
      automation({ id: "active", name: "Release train", status: "active" }),
      automation({ id: "completed", name: "Migration", status: "completed" }),
      automation({ id: "draft", name: "New initiative", status: "draft" }),
    ];
    listAutomationsMock.mockResolvedValue(rows);
    getAutomationMock.mockImplementation((id: string) => Promise.resolve({
      automation: rows.find((item) => item.id === id) ?? automation({ id }),
      runs: [],
      usage: emptyUsage,
    }));

    renderView({ onOpenAutomation: vi.fn() });

    expect(await screen.findByTestId("automations-group-attention")).toBeInTheDocument();
    expect(screen.getByTestId("automations-group-running")).toBeInTheDocument();
    expect(screen.getByTestId("automations-group-finished")).toBeInTheDocument();
    expect(screen.getByTestId("automations-group-drafts")).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("automations-filter-finished"));
    expect(screen.getByTestId("automations-group-finished")).toBeInTheDocument();
    expect(screen.queryByTestId("automations-group-running")).not.toBeInTheDocument();
    expect(screen.getByTestId("automations-filter-all")).toHaveTextContent("4");

    await userEvent.type(screen.getByTestId("automations-search"), "missing");
    expect(screen.getByTestId("automations-filter-empty-state")).toHaveTextContent(
      "No automations match this filter.",
    );
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
      automation({
        id: "draft",
        name: "Draft automation",
        status: "draft",
        goalPrompt: "",
        goalItemsJson: null,
        firstRunPrompt: null,
        logicalEffort: null,
      }),
      automation({
        id: "paused",
        name: "Paused automation",
        status: "paused",
        pausedReasonCode: "workspace_review_blocked",
      }),
      automation({
        id: "paused-plain",
        name: "Plain paused automation",
        status: "paused",
        pausedReasonCode: null,
        goalItemsJson: '[{"id":"phase-1"}]',
      }),
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
    expect(
      screen.getByTestId("automation-row-draft-metadata"),
    ).toHaveTextContent("Draft setup · No goal · No phases · No first run");
    await waitFor(() =>
      expect(screen.getByTestId("automation-row-paused-metadata")).toHaveTextContent(
        "Workspace review blocked · edit · gpt-5.4",
      ),
    );
    expect(screen.getByTestId("automation-row-paused")).not.toHaveTextContent(
      "workspace_review_blocked",
    );
    expect(screen.getByTestId("automation-row-paused-plain-metadata")).toHaveTextContent(
      "1 phase · edit · gpt-5.4",
    );
    expect(screen.getByTestId("automation-row-paused-plain")).toHaveTextContent("Paused");
    expect(
      screen.getByTestId("automation-row-paused-plain").textContent?.match(/Paused/g),
    ).toHaveLength(1);
    expect(screen.getByTestId("automation-row-completed-metadata")).toHaveTextContent(
      "Goal completed",
    );
    expect(screen.getByTestId("automation-row-stopped-metadata")).not.toHaveTextContent(
      "Stopped",
    );
    expect(
      within(screen.getByTestId("automation-row-paused")).getByText("Paused").closest("[data-tone]"),
    ).toHaveAttribute("data-tone", "warning");
    expect(
      within(screen.getByTestId("automation-row-completed")).getByText("Completed").closest("[data-tone]"),
    ).toHaveAttribute("data-tone", "success");
    expect(
      within(screen.getByTestId("automation-row-stopped")).getByText("Stopped").closest("[data-tone]"),
    ).toHaveAttribute("data-tone", "neutral");
    expect(screen.getByTestId("automation-row-paused-status-dot")).toHaveStyle({
      backgroundColor: "var(--status-warning, #f4c025)",
    });
    expect(screen.getByTestId("automation-row-stopped-status-dot")).toHaveStyle({
      backgroundColor: "var(--text-subtle, #6a6a72)",
    });
    expect(
      within(screen.getByTestId("automation-row-empty-active")).getByText("Active").closest("[data-tone]"),
    ).toHaveAttribute("data-tone", "accent");
    expect(
      within(screen.getByTestId("automation-row-running"))
        .getByText("Active")
        .closest("[data-tone]")
        ?.querySelector(".animate-pulse"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("automation-row-empty-active-metadata")).toHaveTextContent(
      "Waiting for first run",
    );
    expect(screen.getByTestId("automation-row-judging-metadata")).toHaveTextContent(
      "Terminal judge running",
    );
    expect(screen.getByTestId("automation-row-judge-failed-metadata")).toHaveTextContent(
      "Terminal judge failed",
    );
    expect(screen.getByTestId("automation-row-running-metadata")).toHaveTextContent(
      "Run 2 in progress",
    );
    expect(
      within(screen.getByTestId("automation-row-awaiting-plan")).getByText(
        "Run 2 · Awaiting plan approval",
      ),
    ).toBeInTheDocument();
    expect(screen.getByTestId("automation-row-awaiting-plan-metadata")).toHaveTextContent(
      "Awaiting plan approval",
    );
    expect(screen.getByTestId("automation-row-published-no-pr-metadata")).toHaveTextContent(
      "Waiting for PR merge",
    );
    expect(screen.getByTestId("automation-row-published-with-pr-metadata")).toHaveTextContent(
      "Waiting for PR #593 to merge",
    );
    expect(screen.getByTestId("automation-row-waiting-judge-metadata")).toHaveTextContent(
      "Terminal judge pending",
    );
    expect(screen.getByTestId("automation-row-scheduling-metadata")).toHaveTextContent(
      "Scheduling next run",
    );
    expect(screen.queryByText("current_branch")).not.toBeInTheDocument();
    expect(screen.getAllByText("0/25").length).toBeGreaterThan(0);
    expect(screen.getByTestId("automation-row-running-runs-progress-fill")).toHaveStyle({
      backgroundColor: "var(--accent-primary)",
      width: "4%",
    });
    expect(screen.queryByTestId("automation-row-draft-runs-progress")).not.toBeInTheDocument();
  });

  it("uses error color only for a failed last-run outcome", async () => {
    const item = automation({ id: "failed-latest" });
    const failedRun = run({
      automationId: item.id,
      runIndex: 9,
      status: "agent_failed",
      judgeState: "none",
    });
    listAutomationsMock.mockResolvedValue([item]);
    getAutomationMock.mockResolvedValue({
      automation: item,
      runs: [failedRun],
      usage: emptyUsage,
    });

    renderView({ onOpenAutomation: vi.fn() });

    const lastRun = await screen.findByText("Run 9 · Agent failed");
    expect(lastRun).toHaveStyle({ color: "var(--status-error, #dd3c3c)" });
  });
});
