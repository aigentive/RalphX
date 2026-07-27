import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
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
});
