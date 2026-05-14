/**
 * AgentsPublishDiffFilter tests
 * Popover-based diff mode selector: "Uncommitted (N files)" radio + "Specific Commit" collapsible.
 * "All commits" mode is intentionally omitted in v1.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { TooltipProvider } from "@/components/ui/tooltip";
import { AgentsPublishDiffFilter } from "./AgentsPublishDiffFilter";
import type { Commit as DiffViewerCommit } from "@/components/diff";

function withProviders(node: React.ReactNode) {
  return <TooltipProvider delayDuration={0}>{node}</TooltipProvider>;
}

const makeCommit = (overrides: Partial<DiffViewerCommit> = {}): DiffViewerCommit => ({
  sha: "abc1234def5678",
  shortSha: "abc1234",
  message: "feat: add feature",
  author: "Alice",
  date: new Date("2026-01-01T00:00:00Z"),
  ...overrides,
});

describe("AgentsPublishDiffFilter", () => {
  const onModeChange = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("trigger button", () => {
    it("renders a trigger button", () => {
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            uncommittedCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toBeInTheDocument();
    });

    it("shows 'Uncommitted (N files)' label when mode is uncommitted", () => {
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            uncommittedCount={3}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Uncommitted");
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("3");
    });

    it("shows short SHA label when a specific commit is selected", () => {
      const commit = makeCommit({ sha: "abc1234def5678", shortSha: "abc1234" });
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="abc1234def5678"
            uncommittedCount={0}
            commits={[commit]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("abc1234");
    });
  });

  describe("popover content", () => {
    it("opens popover when trigger is clicked", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            uncommittedCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-popover")).toBeInTheDocument();
    });

    it("shows Uncommitted radio option in popover", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            uncommittedCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-option-uncommitted")).toBeInTheDocument();
    });

    it("shows Specific Commit collapsible section", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            uncommittedCount={5}
            commits={[makeCommit()]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-commits-section")).toBeInTheDocument();
    });
  });

  describe("mode selection", () => {
    it("calls onModeChange('uncommitted') when Uncommitted radio is clicked", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="abc1234def5678"
            uncommittedCount={5}
            commits={[makeCommit({ sha: "abc1234def5678", shortSha: "abc1234" })]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      await user.click(screen.getByTestId("diff-filter-option-uncommitted"));
      expect(onModeChange).toHaveBeenCalledWith("uncommitted");
    });

    it("calls onModeChange with SHA when a commit option is clicked", async () => {
      const user = userEvent.setup();
      const commit = makeCommit({ sha: "abc1234def5678", shortSha: "abc1234" });
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            uncommittedCount={5}
            commits={[commit]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      // Open the specific commit section
      await user.click(screen.getByTestId("diff-filter-commits-section-trigger"));
      await user.click(screen.getByTestId("diff-filter-commit-abc1234def5678"));
      expect(onModeChange).toHaveBeenCalledWith("abc1234def5678");
    });

    it("filters commit list with filter input", async () => {
      const user = userEvent.setup();
      const commits = [
        makeCommit({ sha: "aaa111", shortSha: "aaa111", message: "feat: alpha" }),
        makeCommit({ sha: "bbb222", shortSha: "bbb222", message: "fix: beta bug" }),
      ];
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            uncommittedCount={5}
            commits={commits}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      await user.click(screen.getByTestId("diff-filter-commits-section-trigger"));
      await user.type(screen.getByTestId("diff-filter-commit-search"), "alpha");
      expect(screen.getByTestId("diff-filter-commit-aaa111")).toBeInTheDocument();
      expect(screen.queryByTestId("diff-filter-commit-bbb222")).toBeNull();
    });
  });
});
