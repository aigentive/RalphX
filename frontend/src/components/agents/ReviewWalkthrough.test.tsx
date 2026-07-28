import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DIFF_ANNOTATION_LEVEL_LEGEND } from "@/components/diff/diffRenderHelpers";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ReviewWalkthrough, type ReviewWalkthroughFinding } from "./ReviewWalkthrough";

const findings: ReviewWalkthroughFinding[] = [
  {
    id: "workspace:one",
    path: "src/one.ts",
    hunkHeader: "@@ -1,1 +1,1 @@",
    title: "First finding",
    message: "The first review note.",
    level: "warning",
    sourceLabel: "Workspace review",
    hunkStatus: "ready",
    hunk: {
      oldStart: 1,
      oldLines: 1,
      newStart: 1,
      newLines: 1,
      header: "@@ -1,1 +1,1 @@",
      lines: [{ kind: "addition", content: "const one = 1;", oldLineNum: null, newLineNum: 1 }],
    },
  },
  {
    id: "pr:two",
    path: "src/two.ts",
    hunkHeader: "@@ -2,1 +2,1 @@",
    title: "Second finding",
    message: "The second review note.",
    level: "failure",
    sourceLabel: "CI check",
    hunkStatus: "ready",
    hunk: {
      oldStart: 2,
      oldLines: 1,
      newStart: 2,
      newLines: 1,
      header: "@@ -2,1 +2,1 @@",
      lines: [{ kind: "addition", content: "const two = 2;", oldLineNum: null, newLineNum: 2 }],
    },
  },
];

function renderWalkthrough(onExit = vi.fn()) {
  return render(
    <TooltipProvider>
      <ReviewWalkthrough findings={findings} onExit={onExit} />
    </TooltipProvider>,
  );
}

describe("ReviewWalkthrough", () => {
  it("renders the current finding, attached hunk, and required progress controls", () => {
    renderWalkthrough();

    expect(screen.getByTestId("publish-review-walkthrough")).toBeInTheDocument();
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 1 of 2",
    );
    expect(screen.getByTestId("publish-review-walkthrough-card")).toHaveTextContent(
      "First finding",
    );
    expect(screen.getByTestId("publish-review-walkthrough-hunk")).toHaveTextContent(
      "const one = 1;",
    );
    expect(screen.getByTestId("publish-review-walkthrough-dot-0")).toHaveAttribute(
      "aria-current",
      "step",
    );
  });

  it("jumps through dots and supports J/K keyboard navigation", async () => {
    const user = userEvent.setup();
    renderWalkthrough();

    await user.click(screen.getByTestId("publish-review-walkthrough-dot-1"));
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 2 of 2",
    );

    await user.keyboard("k");
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 1 of 2",
    );
    await user.keyboard("j");
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 2 of 2",
    );
  });

  it("marks reviewed, auto-advances, and completes on the last finding", async () => {
    const user = userEvent.setup();
    renderWalkthrough();

    await user.click(screen.getByTestId("publish-review-walkthrough-mark"));
    expect(screen.getByTestId("publish-review-walkthrough-progress")).toHaveTextContent(
      "1 of 2 reviewed",
    );
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 2 of 2",
    );

    await user.click(screen.getByTestId("publish-review-walkthrough-mark"));
    expect(screen.getByTestId("publish-review-walkthrough-done")).toHaveTextContent(
      "All findings reviewed",
    );
  });

  it("shows the completion screen when next advances past the last finding and restarts", async () => {
    const user = userEvent.setup();
    renderWalkthrough();

    await user.click(screen.getByTestId("publish-review-walkthrough-next"));
    await user.click(screen.getByTestId("publish-review-walkthrough-next"));
    expect(screen.getByTestId("publish-review-walkthrough-done")).toBeInTheDocument();

    await user.click(screen.getByTestId("publish-review-walkthrough-restart"));
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 1 of 2",
    );
    expect(screen.getByTestId("publish-review-walkthrough-progress")).toHaveTextContent(
      "0 of 2 reviewed",
    );
  });

  it("paints the accent-filled completion action with the on-accent text token", async () => {
    const user = userEvent.setup();
    renderWalkthrough();

    await user.click(screen.getByTestId("publish-review-walkthrough-next"));
    await user.click(screen.getByTestId("publish-review-walkthrough-next"));

    expect(screen.getByTestId("publish-review-walkthrough-done-exit")).toHaveStyle({
      backgroundColor: "var(--accent-primary)",
      color: "var(--text-on-accent)",
    });
  });

  it("counts blocking findings from the shared legend rather than a private level list", async () => {
    const user = userEvent.setup();
    const blockingLevels = DIFF_ANNOTATION_LEVEL_LEGEND[0]!.levels.split(", ");
    const levels = [...blockingLevels, "warning", "notice"];
    const levelFindings = levels.map((level, index) => ({
      ...findings[0]!,
      id: `workspace:level-${index}`,
      hunk: undefined,
      hunkStatus: "unavailable" as const,
      level,
    }));

    render(
      <TooltipProvider>
        <ReviewWalkthrough findings={levelFindings} onExit={vi.fn()} />
      </TooltipProvider>,
    );

    for (let step = 0; step < levels.length; step += 1) {
      await user.click(screen.getByTestId("publish-review-walkthrough-next"));
    }

    expect(screen.getByTestId("publish-review-walkthrough-done")).toHaveTextContent(
      `${blockingLevels.length} blocking findings`,
    );
  });

  it("steps back from the completion screen to the last finding without losing reviewed marks", async () => {
    const user = userEvent.setup();
    renderWalkthrough();

    await user.click(screen.getByTestId("publish-review-walkthrough-next"));
    await user.click(screen.getByTestId("publish-review-walkthrough-mark"));
    expect(screen.getByTestId("publish-review-walkthrough-done")).toBeInTheDocument();

    await user.keyboard("k");
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 2 of 2",
    );
    expect(screen.getByTestId("publish-review-walkthrough-progress")).toHaveTextContent(
      "1 of 2 reviewed",
    );
    expect(screen.getByTestId("publish-review-walkthrough-mark")).toHaveTextContent(
      "Reviewed",
    );
  });

  it("exits back to the full changes list", async () => {
    const user = userEvent.setup();
    const onExit = vi.fn();
    renderWalkthrough(onExit);

    await user.click(screen.getByTestId("publish-review-walkthrough-exit"));
    expect(onExit).toHaveBeenCalledOnce();
  });

  it("keeps an exit route when the finding set empties out mid-walkthrough", async () => {
    const user = userEvent.setup();
    const onExit = vi.fn();
    render(
      <TooltipProvider>
        <ReviewWalkthrough findings={[]} onExit={onExit} />
      </TooltipProvider>,
    );

    expect(screen.getByTestId("publish-review-walkthrough")).toHaveTextContent(
      "No review findings are available",
    );
    await user.click(screen.getByTestId("publish-review-walkthrough-exit"));
    expect(onExit).toHaveBeenCalledOnce();
  });

  it("does not swallow modified J/K shortcuts such as Cmd+J and Ctrl+K", async () => {
    const user = userEvent.setup();
    renderWalkthrough();

    await user.keyboard("{Meta>}j{/Meta}");
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 1 of 2",
    );

    await user.click(screen.getByTestId("publish-review-walkthrough-dot-1"));
    await user.keyboard("{Control>}k{/Control}");
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 2 of 2",
    );

    await user.keyboard("{Alt>}j{/Alt}");
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 2 of 2",
    );
  });

  it("ignores shifted J/K so Shift-modified shortcuts stay available to the page", async () => {
    const user = userEvent.setup();
    renderWalkthrough();

    await user.keyboard("{Shift>}J{/Shift}");
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 1 of 2",
    );

    await user.click(screen.getByTestId("publish-review-walkthrough-dot-1"));
    await user.keyboard("{Shift>}K{/Shift}");
    expect(screen.getByTestId("publish-review-walkthrough-position")).toHaveTextContent(
      "Finding 2 of 2",
    );
  });

  describe("attached hunk states", () => {
    function renderWithHunkState(
      finding: Partial<ReviewWalkthroughFinding>,
      onRetryHunk = vi.fn(),
    ) {
      render(
        <TooltipProvider>
          <ReviewWalkthrough
            findings={[{ ...findings[0]!, hunk: undefined, ...finding }]}
            onExit={vi.fn()}
            onRetryHunk={onRetryHunk}
          />
        </TooltipProvider>,
      );
      return onRetryHunk;
    }

    it("shows loading copy only while the diff is still being fetched", () => {
      renderWithHunkState({ hunkStatus: "loading" });

      expect(
        screen.getByTestId("publish-review-walkthrough-hunk-loading"),
      ).toBeInTheDocument();
      expect(
        screen.queryByTestId("publish-review-walkthrough-hunk-error"),
      ).not.toBeInTheDocument();
    });

    it("shows an error state with a retry action when the diff fetch fails", async () => {
      const user = userEvent.setup();
      const onRetryHunk = renderWithHunkState({ hunkStatus: "error" });

      expect(
        screen.queryByTestId("publish-review-walkthrough-hunk-loading"),
      ).not.toBeInTheDocument();
      const error = screen.getByTestId("publish-review-walkthrough-hunk-error");
      expect(error).toHaveTextContent("Could not load the attached hunk");

      await user.click(screen.getByTestId("publish-review-walkthrough-hunk-retry"));
      expect(onRetryHunk).toHaveBeenCalledWith("src/one.ts");
    });

    it("reports an unavailable hunk instead of loading when the diff loaded without a match", () => {
      renderWithHunkState({ hunkStatus: "unavailable" });

      expect(
        screen.getByTestId("publish-review-walkthrough-hunk-unavailable"),
      ).toHaveTextContent("no longer present");
      expect(
        screen.queryByTestId("publish-review-walkthrough-hunk-loading"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByTestId("publish-review-walkthrough-hunk-retry"),
      ).not.toBeInTheDocument();
    });
  });
});
