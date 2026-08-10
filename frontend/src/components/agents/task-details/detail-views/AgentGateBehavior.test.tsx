import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  AGENT_CONTROL_DISABLED_HINT,
  REMOTE_UNAVAILABLE_HINT,
  resolveAffordanceGate,
} from "@/lib/remote/agent-gate";
import type { Task } from "@/types/task";

import { HumanReviewTaskDetail } from "./HumanReviewTaskDetail";
import { ExecutionTaskDetail } from "./ExecutionTaskDetail";
import { ReviewingTaskDetail } from "./ReviewingTaskDetail";
import { renderGatedDetailView } from "./agent-gate.test-utils";

const apiMocks = vi.hoisted(() => ({
  approveTask: vi.fn(),
  requestTaskChanges: vi.fn(),
  stop: vi.fn(),
}));

vi.mock("@/hooks/useReviews", () => ({
  useTaskStateHistory: () => ({ data: [], isLoading: false }),
  reviewKeys: { all: ["reviews"] },
}));
vi.mock("@/hooks/useTaskStateTransitions", () => ({
  useTaskStateTransitions: () => ({ data: [] }),
}));
vi.mock("@/hooks/useValidationEvents", () => ({
  useValidationEvents: () => [],
}));
vi.mock("@/hooks/useTaskSteps", () => ({
  useTaskSteps: () => ({ data: [], isLoading: false }),
  useStepProgress: () => ({ data: undefined }),
}));
vi.mock("../StepList", () => ({ StepList: () => null }));
vi.mock("@/hooks/useTaskValidationSummary", () => ({
  useTaskValidationSummary: () => ({ data: undefined, isLoading: false, isError: false }),
}));
vi.mock("@/hooks/useTaskValidationEvents", () => ({
  useTaskValidationLiveState: () => null,
  useDisplayTaskValidationSummary: () => undefined,
}));
vi.mock("@/api/review-issues", () => ({
  reviewIssuesApi: {
    getByTaskId: vi.fn(async () => []),
    getProgress: vi.fn(async () => ({ total: 0 })),
  },
}));
vi.mock("@/components/reviews/IssueList", () => ({
  IssueList: () => null,
  IssueProgressBar: () => null,
}));
vi.mock("@/components/reviews/ReviewDetailModal", () => ({ ReviewDetailModal: () => null }));
vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: vi.fn(async () => true),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));
vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: { stop: apiMocks.stop },
    reviews: {
      approveTask: apiMocks.approveTask,
      requestTaskChanges: apiMocks.requestTaskChanges,
    },
  },
}));

function task(internalStatus: Task["internalStatus"]): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    title: "Gated task",
    description: "Detail view gating fixture",
    internalStatus,
    category: "feature",
    startedAt: "2026-07-07T11:00:00Z",
    completedAt: null,
    metadata: null,
  } as Task;
}

function expectHint(): void {
  expect(screen.getAllByTestId("agent-gate-tooltip")[0]).toHaveAttribute("data-agent-gated", "true");
}

describe("Agents task-detail action gates", () => {
  beforeEach(() => vi.clearAllMocks());

  it.each([
    ["remote-default", true, AGENT_CONTROL_DISABLED_HINT],
    ["remote-agent", false, null],
    ["local", false, null],
  ] as const)("HumanReview approve in %s", (environment, disabled, hint) => {
    renderGatedDetailView(<HumanReviewTaskDetail task={task("review_passed")} />, environment);
    const approve = screen.getByTestId("approve-button");
    if (disabled) {
      expect(approve).toBeDisabled();
    } else {
      expect(approve).toBeEnabled();
    }
    if (hint) {
      expectHint();
      expect(resolveAffordanceGate("taskApprove", true, ["ui:read", "ui:operate"]).reason).toBe(hint);
    } else {
      expect(screen.queryByTestId("agent-gate-tooltip")).not.toBeInTheDocument();
    }
  });

  it.each([
    ["remote-default", true],
    ["remote-agent", false],
    ["local", false],
  ] as const)("Reviewing stop in %s", (environment, disabled) => {
    renderGatedDetailView(<ReviewingTaskDetail task={task("reviewing")} />, environment);
    const stop = screen.getByTestId("stop-review-action");
    if (disabled) {
      expect(stop).toBeDisabled();
    } else {
      expect(stop).toBeEnabled();
    }
    if (disabled) {
      expectHint();
      expect(resolveAffordanceGate("taskStop", true, ["ui:read", "ui:operate"]).reason)
        .toBe(AGENT_CONTROL_DISABLED_HINT);
    }
  });

  it.each(["remote-default", "remote-agent"] as const)(
    "Reviewing request changes is unavailable in %s",
    (environment) => {
      renderGatedDetailView(<ReviewingTaskDetail task={task("reviewing")} />, environment);
      expect(screen.getByTestId("request-changes-action")).toBeDisabled();
      expectHint();
      expect(resolveAffordanceGate(
        "taskRequestChangesFromReviewing",
        true,
        environment === "remote-agent"
          ? ["ui:read", "ui:operate", "ui:agent"]
          : ["ui:read", "ui:operate"],
      ).reason).toBe(REMOTE_UNAVAILABLE_HINT);
    },
  );

  it("keeps both reviewing actions enabled locally without a hint", () => {
    renderGatedDetailView(<ReviewingTaskDetail task={task("reviewing")} />, "local");
    expect(screen.getByTestId("stop-review-action")).toBeEnabled();
    expect(screen.getByTestId("request-changes-action")).toBeEnabled();
    expect(screen.queryByTestId("agent-gate-tooltip")).not.toBeInTheDocument();
  });

  it.each([
    ["remote-default", true],
    ["remote-agent", false],
    ["local", false],
  ] as const)("Execution stop and cancel in %s", async (environment, disabled) => {
    const user = userEvent.setup();
    renderGatedDetailView(<ExecutionTaskDetail task={task("executing")} />, environment);
    await user.click(screen.getByTestId("action-dropdown-trigger"));

    const stop = screen.getByTestId("stop-action");
    const cancel = screen.getByTestId("cancel-action");
    if (disabled) {
      expect(stop).toHaveAttribute("aria-disabled", "true");
      expect(cancel).toHaveAttribute("aria-disabled", "true");
    } else {
      expect(stop).not.toHaveAttribute("aria-disabled");
      expect(cancel).not.toHaveAttribute("aria-disabled");
    }
    if (disabled) {
      expect(screen.getAllByTestId("agent-gate-tooltip")).toHaveLength(2);
    } else {
      expect(screen.queryByTestId("agent-gate-tooltip")).not.toBeInTheDocument();
    }
  });
});
