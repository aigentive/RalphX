import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  AutomationRunTaskLedger,
} from "./AutomationRunTaskLedger";
import { automationRunTaskLedgerRefetchInterval } from "./automationRunTaskLedgerPolling";
import type { AgentTaskSummary } from "@/api/agent-tasks";
import type { AutomationRunStatus } from "@/api/automations";

const { listConversationTasksMock } = vi.hoisted(() => ({
  listConversationTasksMock: vi.fn(),
}));

vi.mock("@/api/agent-tasks", () => ({
  agentTaskApi: {
    listConversationTasks: (...args: unknown[]) => listConversationTasksMock(...args),
  },
}));

function task(overrides: Partial<AgentTaskSummary> = {}): AgentTaskSummary {
  return {
    taskId: "task-1",
    taskNumber: 1,
    title: "Task one",
    state: "active",
    ownerAgent: null,
    blockedBy: [],
    blocks: [],
    availability: "available",
    updatedAt: "2026-07-05T00:00:00Z",
    ...overrides,
  };
}

function renderLedger(runStatus: AutomationRunStatus) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <AutomationRunTaskLedger
        conversationId="conversation-1"
        projectId="project-1"
        runStatus={runStatus}
      />
    </QueryClientProvider>,
  );
}

describe("AutomationRunTaskLedger", () => {
  afterEach(() => {
    vi.useRealTimers();
    listConversationTasksMock.mockReset();
  });

  it("renders active/open tasks prominently with a done/dropped summary", async () => {
    listConversationTasksMock.mockResolvedValue([
      task({ taskId: "a", taskNumber: 1, title: "Active work", state: "active", ownerAgent: "coder-1" }),
      task({ taskId: "b", taskNumber: 2, title: "Open work", state: "open" }),
      task({ taskId: "c", taskNumber: 3, title: "Done work", state: "done" }),
      task({ taskId: "d", taskNumber: 4, title: "Dropped work", state: "dropped" }),
    ]);

    renderLedger("running");

    expect(await screen.findByText("Active work")).toBeInTheDocument();
    expect(screen.getByText("Open work")).toBeInTheDocument();
    expect(screen.queryByText("coder-1")).not.toBeInTheDocument();

    // Only actionable (active/open) tasks get their own rows.
    expect(screen.getAllByTestId("automation-run-task-ledger-row")).toHaveLength(2);
    expect(screen.queryByText("Done work")).not.toBeInTheDocument();
    expect(screen.queryByText("Dropped work")).not.toBeInTheDocument();

    const labelRow = screen.getByTestId("automation-run-task-ledger-label-row");
    const summary = screen.getByTestId("automation-run-task-ledger-summary");
    expect(labelRow).toContainElement(summary);
    expect(summary).toHaveTextContent("1 done · 1 dropped");
  });

  it("shows the empty state when there are no agent tasks", async () => {
    listConversationTasksMock.mockResolvedValue([]);

    renderLedger("merged");

    expect(await screen.findByText("No agent tasks yet.")).toBeInTheDocument();
  });

  it("surfaces an error state when the task query fails", async () => {
    listConversationTasksMock.mockRejectedValue(new Error("boom"));

    renderLedger("running");

    expect(
      await screen.findByText("Could not load agent tasks."),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("automation-run-task-ledger-row"),
    ).not.toBeInTheDocument();
  });

  it("shows the no-active-tasks state when every task is terminal", async () => {
    listConversationTasksMock.mockResolvedValue([
      task({ taskId: "a", taskNumber: 1, title: "Done work", state: "done" }),
      task({ taskId: "b", taskNumber: 2, title: "Dropped work", state: "dropped" }),
    ]);

    renderLedger("running");

    expect(await screen.findByText("No active tasks right now.")).toBeInTheDocument();
    expect(screen.getByTestId("automation-run-task-ledger-summary")).toHaveTextContent(
      "1 done · 1 dropped",
    );
  });

  it("coerces a lingering active task to Done once the run is terminal", async () => {
    listConversationTasksMock.mockResolvedValue([
      task({ taskId: "a", taskNumber: 1, title: "Still marked active", state: "active" }),
    ]);

    renderLedger("merged");

    expect(await screen.findByText("Still marked active")).toBeInTheDocument();
    // effectiveTaskState maps active → done for a merged run, so the row reads "Done".
    const stateLabel = screen.getByTestId("automation-run-task-ledger-row-state");
    expect(stateLabel).toHaveTextContent("Done");
  });

  it("polls on an interval while the run is open", async () => {
    vi.useFakeTimers();
    listConversationTasksMock.mockResolvedValue([task()]);

    renderLedger("running");

    await vi.advanceTimersByTimeAsync(0);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(2_500);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(2);
  });

  it("widens running poll cadence after repeated identical refetches", async () => {
    vi.useFakeTimers();
    listConversationTasksMock.mockResolvedValue([task()]);

    renderLedger("running");

    await vi.advanceTimersByTimeAsync(0);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(1);

    for (let index = 0; index < 41; index += 1) {
      await vi.advanceTimersByTimeAsync(2_500);
    }
    expect(listConversationTasksMock).toHaveBeenCalledTimes(42);

    await vi.advanceTimersByTimeAsync(2_500);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(42);

    await vi.advanceTimersByTimeAsync(12_500);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(43);
  });

  it("slows down running polls after repeated unchanged responses", () => {
    expect(automationRunTaskLedgerRefetchInterval("running", 39)).toBe(2_500);
    expect(automationRunTaskLedgerRefetchInterval("running", 40)).toBe(15_000);
    expect(automationRunTaskLedgerRefetchInterval("published", 0)).toBe(15_000);
    expect(automationRunTaskLedgerRefetchInterval("merged", 0)).toBe(false);
  });

  it("polls parked and published runs on a slower interval", async () => {
    vi.useFakeTimers();
    listConversationTasksMock.mockResolvedValue([task()]);

    renderLedger("awaiting_plan_approval");

    await vi.advanceTimersByTimeAsync(0);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(2_500);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(12_500);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(2);
  });

  it("does not poll once the run is terminal", async () => {
    vi.useFakeTimers();
    listConversationTasksMock.mockResolvedValue([task({ state: "done" })]);

    renderLedger("merged");

    await vi.advanceTimersByTimeAsync(0);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(10_000);
    expect(listConversationTasksMock).toHaveBeenCalledTimes(1);
  });
});
