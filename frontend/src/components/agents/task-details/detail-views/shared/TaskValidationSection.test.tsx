import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskValidationSummary } from "@/hooks/useTaskValidationSummary";
import {
  TaskValidationEvidenceCard,
  TaskValidationSection,
  TaskValidationSummaryCard,
} from "./TaskValidationEvidenceCard";

const testState = vi.hoisted(() => ({
  summary: undefined as
    | {
        data?: TaskValidationSummary;
        isLoading?: boolean;
        isError?: boolean;
      }
    | undefined,
  display: undefined as TaskValidationSummary | undefined,
}));

vi.mock("@/hooks/useTaskValidationSummary", () => ({
  useTaskValidationSummary: () =>
    testState.summary ?? { data: undefined, isLoading: false, isError: false },
}));

vi.mock("@/hooks/useTaskValidationEvents", () => ({
  useTaskValidationLiveState: () => null,
  useDisplayTaskValidationSummary: () => testState.display,
}));

function failedSummary(): TaskValidationSummary {
  return {
    task_id: "task-1",
    project_id: "project-1",
    policy_enabled: true,
    latest_run: {
      id: "run-1",
      purpose: "final",
      context_type: "execution",
      requested_by_agent: "ralphx-execution-worker",
      status: "failed",
      mode: "force",
      policy_enabled: true,
      head_sha: "abcdef1234567890",
      head_short_sha: "abcdef12",
      base_ref: "main",
      started_at: "2026-07-06T11:00:00.000Z",
      completed_at: "2026-07-06T11:00:02.000Z",
    },
    commands: [
      {
        id: "command-1",
        command_source: "agent_selected",
        command_ref: null,
        command: "npm test",
        cwd: "/repo",
        label: "Unit tests",
        category: "test",
        reason: "Regression check",
        related_files: [],
        cache_decision: "ran",
        status: "failed",
        exit_code: 1,
        duration_ms: 1234,
        stdout_snippet: null,
        stderr_snippet: "expected true to be false",
        stdout_log_path: null,
        stderr_log_path: "/logs/stderr.log",
        created_at: "2026-07-06T11:00:02.000Z",
      },
    ],
    legacy_validation_cache: null,
    disabled_reason: null,
  };
}

type RunSummary = NonNullable<TaskValidationSummary["latest_run"]>;

function passedSummary(overrides: Partial<RunSummary> = {}): TaskValidationSummary {
  const summary = failedSummary();
  return {
    ...summary,
    latest_run: {
      ...summary.latest_run!,
      status: "passed",
      completed_at: "2026-07-06T11:00:03.000Z",
      ...overrides,
    },
    commands: [],
  };
}

function runningSummary(): TaskValidationSummary {
  const summary = failedSummary();
  return {
    ...summary,
    latest_run: {
      ...summary.latest_run!,
      status: "running",
      completed_at: null,
    },
    commands: [
      {
        ...summary.commands[0]!,
        status: "running",
        exit_code: null,
        duration_ms: null,
        stderr_snippet: null,
        created_at: "2026-07-06T11:00:00.000Z",
      },
    ],
  };
}

function summaryWithRunStatus(
  status: NonNullable<TaskValidationSummary["latest_run"]>["status"],
): TaskValidationSummary {
  const summary = failedSummary();
  return {
    ...summary,
    latest_run: {
      ...summary.latest_run!,
      status,
    },
    commands: [],
  };
}

describe("TaskValidationSection", () => {
  beforeEach(() => {
    testState.summary = undefined;
    testState.display = undefined;
  });

  it("renders failed command output inline", () => {
    const summary = failedSummary();
    testState.summary = { data: summary, isLoading: false, isError: false };
    testState.display = summary;

    render(<TaskValidationSection taskId="task-1" />);

    expect(screen.getByTestId("task-validation-section")).toBeInTheDocument();
    expect(screen.getByText("Task Validation")).toBeInTheDocument();
    expect(screen.getByText("Failed")).toBeInTheDocument();
    expect(screen.getByText("Validation Commands")).toBeInTheDocument();
    expect(screen.getByText("Unit tests")).toBeInTheDocument();
    expect(screen.getByText("expected true to be false")).toBeInTheDocument();
  });

  it("humanizes command category labels when no label is recorded", () => {
    const summary = failedSummary();
    summary.commands = [
      {
        ...summary.commands[0]!,
        label: "",
        category: "type_check",
        stderr_snippet: null,
      },
    ];
    testState.summary = { data: summary, isLoading: false, isError: false };
    testState.display = summary;

    render(<TaskValidationSection taskId="task-1" />);

    expect(screen.getByText("Type Check")).toBeInTheDocument();
  });

  it("labels historical task detail evidence as latest task validation", () => {
    const summary = failedSummary();
    testState.summary = { data: summary, isLoading: false, isError: false };
    testState.display = summary;

    render(<TaskValidationSection taskId="task-1" isHistorical />);

    expect(screen.getByText("Latest Task Validation")).toBeInTheDocument();
    expect(screen.getByText("Latest task validation run")).toBeInTheDocument();
  });

  it("renders passed persisted evidence without an empty command list", () => {
    const summary = passedSummary();
    testState.summary = { data: summary, isLoading: false, isError: false };
    testState.display = summary;

    render(<TaskValidationEvidenceCard taskId="task-1" />);

    expect(screen.getByText("Passed")).toBeInTheDocument();
    expect(
      screen.getByText("Latest validation run completed successfully."),
    ).toBeInTheDocument();
    expect(screen.queryByText("Validation Commands")).not.toBeInTheDocument();
  });

  it("keeps current eligible post-change passed validation as approving evidence", () => {
    render(
      <TaskValidationSummaryCard
        displaySummary={passedSummary({
          purpose: "final",
          current_for_head: true,
          current_for_execution_episode: true,
          review_evidence_eligible: true,
          ineligible_reason: null,
        })}
        isHistorical={false}
        usingLive={false}
      />,
    );

    expect(screen.getByText("Passed")).toBeInTheDocument();
    expect(
      screen.getByText("Latest validation run completed successfully."),
    ).toBeInTheDocument();
  });

  it("renders backend-classified baseline-only passed evidence as informational", () => {
    render(
      <TaskValidationSummaryCard
        displaySummary={passedSummary({
          purpose: "baseline",
          current_for_head: true,
          current_for_execution_episode: true,
          review_evidence_eligible: false,
          ineligible_reason: "baseline_only",
        })}
        isHistorical={false}
        usingLive={false}
      />,
    );

    expect(screen.getByText("Baseline Only")).toBeInTheDocument();
    expect(
      screen.getByText("Baseline validation passed, but final validation is still needed."),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Latest validation run completed successfully."),
    ).not.toBeInTheDocument();
  });

  it("treats older baseline passed responses without eligibility fields as non-approving", () => {
    render(
      <TaskValidationSummaryCard
        displaySummary={passedSummary({ purpose: "baseline" })}
        isHistorical={false}
        usingLive={false}
      />,
    );

    expect(screen.getByText("Baseline Only")).toBeInTheDocument();
    expect(
      screen.getByText("Baseline validation passed, but final validation is still needed."),
    ).toBeInTheDocument();
  });

  it.each([
    ["stale_head", "Validation passed for an older commit. Final validation is still needed."],
    [
      "stale_episode",
      "Validation passed for an older execution attempt. Final validation is still needed.",
    ],
  ] as const)("renders %s passed evidence as a warning", (reason, message) => {
    render(
      <TaskValidationSummaryCard
        displaySummary={passedSummary({
          purpose: "final",
          current_for_head: reason !== "stale_head",
          current_for_execution_episode: reason !== "stale_episode",
          review_evidence_eligible: false,
          ineligible_reason: reason,
        })}
        isHistorical={false}
        usingLive={false}
      />,
    );

    expect(screen.getByText("Stale Evidence")).toBeInTheDocument();
    expect(screen.getByText(message)).toBeInTheDocument();
    expect(
      screen.queryByText("Latest validation run completed successfully."),
    ).not.toBeInTheDocument();
  });

  it("renders live validation summary copy", () => {
    render(
      <TaskValidationSummaryCard
        displaySummary={passedSummary()}
        isHistorical={false}
        usingLive
      />,
    );

    expect(screen.getByText("Live validation run")).toBeInTheDocument();
    expect(
      screen.getByText("Live task validation completed successfully."),
    ).toBeInTheDocument();
  });

  it("renders loading and unavailable states for validation evidence", () => {
    testState.summary = { data: undefined, isLoading: true, isError: false };

    const { rerender } = render(<TaskValidationSection taskId="task-1" />);

    expect(screen.getByText("Loading validation evidence")).toBeInTheDocument();

    testState.summary = { data: undefined, isLoading: false, isError: true };

    rerender(<TaskValidationSection taskId="task-1" />);

    expect(screen.getByText("Validation evidence unavailable")).toBeInTheDocument();
  });

  it("renders disabled and legacy validation evidence summaries", () => {
    const disabledSummary = {
      ...failedSummary(),
      policy_enabled: false,
      disabled_reason: "Validation disabled for this project.",
      latest_run: null,
      commands: [],
    };
    testState.summary = {
      data: disabledSummary,
      isLoading: false,
      isError: false,
    };
    testState.display = disabledSummary;

    const { rerender } = render(<TaskValidationSection taskId="task-1" />);

    expect(screen.getByText("Disabled")).toBeInTheDocument();
    expect(
      screen.getByText("Validation disabled for this project."),
    ).toBeInTheDocument();

    const legacySummary = {
      ...failedSummary(),
      latest_run: null,
      commands: [],
      legacy_validation_cache: {
        hint_message: "Legacy task cache passed before backend validation.",
      },
    };
    testState.summary = { data: legacySummary, isLoading: false, isError: false };
    testState.display = legacySummary;

    rerender(<TaskValidationSection taskId="task-1" />);

    expect(screen.getByText("Legacy Evidence")).toBeInTheDocument();
    expect(
      screen.getByText("Legacy task cache passed before backend validation."),
    ).toBeInTheDocument();
  });

  it.each([
    ["skipped", "Latest validation run did not execute commands."],
    ["cancelled", "Latest validation run was cancelled."],
  ] as const)("renders %s validation run copy", (status, message) => {
    const summary = summaryWithRunStatus(status);
    testState.summary = { data: summary, isLoading: false, isError: false };
    testState.display = summary;

    render(<TaskValidationSection taskId="task-1" />);

    expect(screen.getByText(message)).toBeInTheDocument();
  });

  it("updates running validation command elapsed time", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-06T11:00:02.000Z"));
    try {
      const summary = runningSummary();
      testState.summary = { data: summary, isLoading: false, isError: false };
      testState.display = summary;

      const { unmount } = render(<TaskValidationSection taskId="task-1" />);

      expect(screen.getByText("Running")).toBeInTheDocument();
      expect(screen.getByText("Validation Commands")).toBeInTheDocument();

      act(() => {
        vi.advanceTimersByTime(1_000);
      });
      unmount();
    } finally {
      vi.useRealTimers();
    }
  });
});
