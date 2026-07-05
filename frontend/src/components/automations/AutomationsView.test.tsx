import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AutomationsView } from "./AutomationsView";
import type { Automation } from "@/api/automations";

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
    maxRuns: 25,
    maxConsecutiveFailures: 3,
    firstRunPrompt: null,
    setupAnalysisSummary: null,
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
    listAutomationsMock.mockResolvedValue([automation()]);
    getAutomationMock.mockResolvedValue({ automation: automation(), runs: [], usage: emptyUsage });

    renderView();

    const row = await screen.findByTestId("automation-row-automation-1");
    expect(row).toBeInTheDocument();
    expect(within(row).getByText("Ship migration loop")).toBeInTheDocument();
    expect(within(row).getByText("Demo Project")).toBeInTheDocument();
    expect(within(row).getByText("edit · codex/gpt-5.4/high")).toBeInTheDocument();
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
});
