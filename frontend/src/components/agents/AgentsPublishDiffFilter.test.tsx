/**
 * AgentsPublishDiffFilter tests
 * Popover-based diff mode selector: "Workspace changes (N files)" radio + "Specific Commit" collapsible.
 * Includes Workspace changes, worktree buckets, All commits, and specific-commit modes.
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
            workspaceChangeCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toBeInTheDocument();
    });

    it("shows workspace changes label when mode is uncommitted", () => {
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={3}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Workspace changes");
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("3");
    });

    it("uses a custom workspace changes label for published branches", () => {
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={2}
            workspaceChangeLabel="Published changes"
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Published changes");
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("2");
    });

    it("shows short SHA label when a specific commit is selected", () => {
      const commit = makeCommit({ sha: "abc1234def5678", shortSha: "abc1234" });
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="abc1234def5678"
            workspaceChangeCount={0}
            commits={[commit]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("abc1234");
    });

    it("shows 'Staged (N files)' when mode is staged and stagedCount is provided", () => {
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="staged"
            workspaceChangeCount={0}
            stagedCount={4}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Staged");
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("4");
    });

    it("shows 'Staged' (no count) when mode is staged but stagedCount is undefined", () => {
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="staged"
            workspaceChangeCount={0}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Staged");
    });

    it("shows 'Unstaged (N files)' when mode is unstaged and unstagedCount is provided", () => {
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="unstaged"
            workspaceChangeCount={0}
            unstagedCount={2}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("Unstaged");
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("2");
    });

    it("shows 'All commits (N commits)' when mode is cumulative", () => {
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="cumulative"
            workspaceChangeCount={0}
            commits={[makeCommit(), makeCommit({ sha: "bbb222", shortSha: "bbb222" })]}
            onModeChange={onModeChange}
          />,
        ),
      );
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("All commits");
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent("2");
    });

    it.each(["Published changes", "Pull request changes"])(
      "uses count-free terminal history label %s in cumulative mode",
      (cumulativeModeLabel) => {
        render(
          withProviders(
            <AgentsPublishDiffFilter
              mode="cumulative"
              workspaceChangeCount={0}
              cumulativeModeLabel={cumulativeModeLabel}
              commits={[]}
              onModeChange={onModeChange}
            />,
          ),
        );

        expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent(
          cumulativeModeLabel,
        );
        expect(screen.getByTestId("diff-filter-trigger")).not.toHaveTextContent(
          "0 commits",
        );
      },
    );
  });

  describe("popover content", () => {
    it("opens popover when trigger is clicked", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-popover")).toBeInTheDocument();
    });

    it("shows workspace changes radio option in popover", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-option-uncommitted")).toBeInTheDocument();
      expect(screen.getByTestId("diff-filter-option-uncommitted")).toHaveTextContent(
        "Workspace changes",
      );
    });

    it("shows Specific Commit collapsible section", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            commits={[makeCommit()]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-commits-section")).toBeInTheDocument();
    });

    it("shows Unstaged radio option in popover", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-option-unstaged")).toBeInTheDocument();
    });

    it("shows Staged radio option in popover", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-option-staged")).toBeInTheDocument();
    });

    it("shows staged and unstaged counts in popover options when available", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            stagedCount={2}
            unstagedCount={3}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );

      await user.click(screen.getByTestId("diff-filter-trigger"));

      expect(screen.getByTestId("diff-filter-option-unstaged")).toHaveTextContent(
        "Unstaged (3 files)",
      );
      expect(screen.getByTestId("diff-filter-option-staged")).toHaveTextContent(
        "Staged (2 files)",
      );
    });

    it("shows conflicted count in popover options when available", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="conflicted"
            workspaceChangeCount={6}
            conflictedCount={2}
            stagedCount={1}
            unstagedCount={3}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );

      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent(
        "Conflicted (2 files)",
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-option-conflicted")).toHaveTextContent(
        "Conflicted (2 files)",
      );
    });

    it("shows All commits radio option in popover", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            commits={[makeCommit()]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.getByTestId("diff-filter-option-cumulative")).toBeInTheDocument();
    });

    it("uses the terminal history label in the cumulative radio option", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="cumulative"
            workspaceChangeCount={0}
            cumulativeModeLabel="Published changes"
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(
        screen.getByTestId("diff-filter-option-cumulative"),
      ).toHaveTextContent("Published changes");
      expect(
        screen.getByTestId("diff-filter-option-cumulative"),
      ).not.toHaveTextContent("0 commits");
    });

    it("hides worktree-only modes for read-only historical reviews", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="cumulative"
            workspaceChangeCount={5}
            commits={[makeCommit()]}
            supportsWorktreeModes={false}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      expect(screen.queryByTestId("diff-filter-option-uncommitted")).toBeNull();
      expect(screen.queryByTestId("diff-filter-option-staged")).toBeNull();
      expect(screen.queryByTestId("diff-filter-option-unstaged")).toBeNull();
      expect(screen.getByTestId("diff-filter-option-cumulative")).toBeInTheDocument();
    });
  });

  describe("mode selection", () => {
    it("calls onModeChange('uncommitted') when workspace changes radio is clicked", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="abc1234def5678"
            workspaceChangeCount={5}
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
            workspaceChangeCount={5}
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

    it("calls onModeChange('staged') when Staged radio is clicked", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      await user.click(screen.getByTestId("diff-filter-option-staged"));
      expect(onModeChange).toHaveBeenCalledWith("staged");
    });

    it("calls onModeChange('unstaged') when Unstaged radio is clicked", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            commits={[]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      await user.click(screen.getByTestId("diff-filter-option-unstaged"));
      expect(onModeChange).toHaveBeenCalledWith("unstaged");
    });

    it("calls onModeChange('cumulative') when All commits radio is clicked", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishDiffFilter
            mode="uncommitted"
            workspaceChangeCount={5}
            commits={[makeCommit()]}
            onModeChange={onModeChange}
          />,
        ),
      );
      await user.click(screen.getByTestId("diff-filter-trigger"));
      await user.click(screen.getByTestId("diff-filter-option-cumulative"));
      expect(onModeChange).toHaveBeenCalledWith("cumulative");
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
            workspaceChangeCount={5}
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
