import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskValidationSummary } from "@/hooks/useTaskValidationSummary";
import { TaskValidationSection } from "./TaskValidationEvidenceCard";

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

  it("labels historical task detail evidence as latest task validation", () => {
    const summary = failedSummary();
    testState.summary = { data: summary, isLoading: false, isError: false };
    testState.display = summary;

    render(<TaskValidationSection taskId="task-1" isHistorical />);

    expect(screen.getByText("Latest Task Validation")).toBeInTheDocument();
    expect(screen.getByText("Latest task validation run")).toBeInTheDocument();
  });
});
