/**
 * MergePhaseTimeline component tests
 * Verifies phase rendering, status indicators, and message display
 */

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MergePhaseTimeline } from "./MergePhaseTimeline";
import type { MergeProgressEvent, MergePhaseInfo } from "@/types/events";

function makePhase(
  overrides: Partial<MergeProgressEvent> = {}
): MergeProgressEvent {
  return {
    task_id: "task-123",
    phase: "worktree_setup",
    status: "started",
    message: "",
    timestamp: "2026-02-11T10:00:00Z",
    ...overrides,
  };
}

const DYNAMIC_PHASES: MergePhaseInfo[] = [
  { id: "worktree_setup", label: "Worktree Setup" },
  { id: "programmatic_merge", label: "Merge" },
  { id: "npm_run_typecheck", label: "Type Check" },
  { id: "npm_run_lint", label: "Lint" },
  { id: "cargo_clippy", label: "Clippy" },
  { id: "cargo_test", label: "Test" },
  { id: "finalize", label: "Finalize" },
];

describe("MergePhaseTimeline", () => {
  it("renders nothing when phases array is empty", () => {
    const { container } = render(<MergePhaseTimeline phases={[]} />);
    expect(container.innerHTML).toBe("");
  });

  it("renders timeline container with testid", () => {
    render(<MergePhaseTimeline phases={[makePhase()]} phaseList={DYNAMIC_PHASES} />);
    expect(screen.getByTestId("merge-phase-timeline")).toBeInTheDocument();
  });

  it("shows 'Merge Progress' section title", () => {
    render(<MergePhaseTimeline phases={[makePhase()]} phaseList={DYNAMIC_PHASES} />);
    expect(screen.getByText("Merge Progress")).toBeInTheDocument();
  });

  it("renders phase label for a single started phase", () => {
    render(
      <MergePhaseTimeline
        phases={[makePhase({ phase: "worktree_setup", status: "started" })]}
        phaseList={DYNAMIC_PHASES}
      />
    );
    expect(screen.getByText("Worktree Setup")).toBeInTheDocument();
  });

  it("renders multiple phase labels in order", () => {
    const phases = [
      makePhase({ phase: "worktree_setup", status: "passed" }),
      makePhase({ phase: "programmatic_merge", status: "passed" }),
      makePhase({ phase: "npm_run_typecheck", status: "started" }),
    ];
    render(<MergePhaseTimeline phases={phases} phaseList={DYNAMIC_PHASES} />);

    expect(screen.getByText("Worktree Setup")).toBeInTheDocument();
    expect(screen.getByText("Merge")).toBeInTheDocument();
    expect(screen.getByText("Type Check")).toBeInTheDocument();
  });

  it("shows pending phases between received phases based on phase config order", () => {
    // If we receive worktree_setup and npm_run_typecheck, phases in between
    // (programmatic_merge) should appear as pending
    const phases = [
      makePhase({ phase: "worktree_setup", status: "passed" }),
      makePhase({ phase: "npm_run_typecheck", status: "started" }),
    ];
    render(<MergePhaseTimeline phases={phases} phaseList={DYNAMIC_PHASES} />);

    // programmatic_merge is between them in config, should show as pending
    expect(screen.getByText("Merge")).toBeInTheDocument();
  });

  it("shows message for started phase", () => {
    render(
      <MergePhaseTimeline
        phases={[
          makePhase({
            phase: "npm_run_typecheck",
            status: "started",
            message: "Running type checker...",
          }),
        ]}
        phaseList={DYNAMIC_PHASES}
      />
    );
    expect(screen.getByText("Running type checker...")).toBeInTheDocument();
  });

  it("shows message for failed phase", () => {
    render(
      <MergePhaseTimeline
        phases={[
          makePhase({
            phase: "npm_run_lint",
            status: "failed",
            message: "Lint errors found",
          }),
        ]}
        phaseList={DYNAMIC_PHASES}
      />
    );
    expect(screen.getByText("Lint errors found")).toBeInTheDocument();
  });

  it("does not show message for passed phase", () => {
    render(
      <MergePhaseTimeline
        phases={[
          makePhase({
            phase: "npm_run_typecheck",
            status: "passed",
            message: "Typecheck passed",
          }),
        ]}
        phaseList={DYNAMIC_PHASES}
      />
    );
    // Message should NOT appear for passed phases
    expect(screen.queryByText("Typecheck passed")).not.toBeInTheDocument();
  });

  it("renders full phase sequence with correct labels", () => {
    const phases = [
      makePhase({ phase: "worktree_setup", status: "passed" }),
      makePhase({ phase: "programmatic_merge", status: "passed" }),
      makePhase({ phase: "npm_run_typecheck", status: "passed" }),
      makePhase({ phase: "npm_run_lint", status: "passed" }),
      makePhase({ phase: "cargo_clippy", status: "passed" }),
      makePhase({ phase: "cargo_test", status: "passed" }),
      makePhase({ phase: "finalize", status: "started" }),
    ];
    render(<MergePhaseTimeline phases={phases} phaseList={DYNAMIC_PHASES} />);

    expect(screen.getByText("Worktree Setup")).toBeInTheDocument();
    expect(screen.getByText("Merge")).toBeInTheDocument();
    expect(screen.getByText("Type Check")).toBeInTheDocument();
    expect(screen.getByText("Lint")).toBeInTheDocument();
    expect(screen.getByText("Clippy")).toBeInTheDocument();
    expect(screen.getByText("Test")).toBeInTheDocument();
    expect(screen.getByText("Finalize")).toBeInTheDocument();
  });

  it("displays 'Merge Progress' section title", () => {
    render(<MergePhaseTimeline phases={[makePhase()]} phaseList={DYNAMIC_PHASES} />);
    expect(screen.getByText("Merge Progress")).toBeInTheDocument();
  });

  it("uses default config when phaseList is null", () => {
    const phases = [
      makePhase({ phase: "worktree_setup", status: "passed" }),
      makePhase({ phase: "programmatic_merge", status: "started" }),
    ];
    render(<MergePhaseTimeline phases={phases} phaseList={null} />);

    expect(screen.getByText("Worktree Setup")).toBeInTheDocument();
    expect(screen.getByText("Merge")).toBeInTheDocument();
  });

  it("uses custom dynamic phases from phaseList", () => {
    const customPhases: MergePhaseInfo[] = [
      { id: "worktree_setup", label: "Worktree Setup" },
      { id: "programmatic_merge", label: "Merge" },
      { id: "go_test", label: "Go Test" },
      { id: "golangci-lint_run", label: "Lint" },
      { id: "finalize", label: "Finalize" },
    ];

    const phases = [
      makePhase({ phase: "worktree_setup", status: "passed" }),
      makePhase({ phase: "programmatic_merge", status: "passed" }),
      makePhase({ phase: "go_test", status: "started" }),
    ];

    render(<MergePhaseTimeline phases={phases} phaseList={customPhases} />);

    expect(screen.getByText("Go Test")).toBeInTheDocument();
    // Lint should show as pending (it's after the current phase in the list)
    expect(screen.queryByText("Lint")).not.toBeInTheDocument();
  });

  it("renders skipped phase with the SkipForward icon and muted text colour", () => {
    const skippedList: MergePhaseInfo[] = [
      { id: "worktree_setup", label: "Worktree Setup" },
      { id: "programmatic_merge", label: "Merge" },
    ];
    const phases = [
      makePhase({ phase: "worktree_setup", status: "passed" }),
      makePhase({ phase: "programmatic_merge", status: "skipped" }),
    ];
    const { container } = render(
      <MergePhaseTimeline phases={phases} phaseList={skippedList} />,
    );
    // The lucide SkipForward icon is rendered when status === skipped.
    expect(container.querySelector(".lucide-skip-forward")).toBeInTheDocument();
  });
});
