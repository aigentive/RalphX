import { useEffect, useMemo } from "react";
import { ExternalLink } from "lucide-react";
import { SimpleDiffView } from "@/components/diff/SimpleDiffView";
import {
  DIFF_ANNOTATION_LEVEL_LEGEND,
  annotationLevelColor,
} from "@/components/diff/diffRenderHelpers";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { DiffHunk } from "@/api/diff";
import { ReviewWalkthroughNavigationButton } from "./ReviewWalkthroughControls";
import { useReviewWalkthrough } from "./useReviewWalkthrough";

export interface ReviewWalkthroughFinding {
  id: string;
  path: string;
  hunkHeader: string;
  title: string;
  message: string;
  level: string;
  sourceLabel: string;
  hunk?: DiffHunk | undefined;
}

interface ReviewWalkthroughProps {
  findings: ReviewWalkthroughFinding[];
  onExit: () => void;
  onOpenFile?: ((path: string) => void) | undefined;
  onCurrentFindingChange?: ((findingId: string | null) => void) | undefined;
}

function severityLabel(level: string): string {
  const normalized = level.toLowerCase();
  return (
    DIFF_ANNOTATION_LEVEL_LEGEND.find((item) =>
      item.levels.split(", ").includes(normalized),
    )?.label ?? "Other"
  );
}

export function ReviewWalkthrough({
  findings,
  onExit,
  onOpenFile,
  onCurrentFindingChange,
}: ReviewWalkthroughProps) {
  const findingIds = useMemo(
    () => findings.map((finding) => finding.id),
    [findings],
  );
  const {
    currentIndex,
    isComplete,
    reviewedIds,
    goTo,
    next,
    previous,
    restart,
    toggleReviewed,
  } = useReviewWalkthrough({ findingIds, onCurrentFindingChange });
  const currentFinding = findings[currentIndex];

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
      ) {
        return;
      }
      if (event.key.toLowerCase() === "j") {
        event.preventDefault();
        next();
      } else if (event.key.toLowerCase() === "k") {
        event.preventDefault();
        previous();
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [next, previous]);

  const reviewedCount = reviewedIds.size;
  const blockingCount = findings.filter((finding) =>
    ["failure", "error", "critical", "high"].includes(finding.level.toLowerCase()),
  ).length;

  if (findings.length === 0) {
    return (
      <section data-testid="publish-review-walkthrough" className="p-3">
        <div
          className="rounded-md p-4 text-sm"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
        >
          No review findings are available for these changes.
        </div>
      </section>
    );
  }

  if (isComplete || currentFinding === undefined) {
    return (
      <section data-testid="publish-review-walkthrough" className="p-3">
        <div className="mb-3 flex items-center justify-between gap-3">
          <button
            type="button"
            data-testid="publish-review-walkthrough-exit"
            onClick={onExit}
            className="text-xs font-medium hover:underline"
            style={{ color: "var(--text-secondary)" }}
          >
            ← Back to all changes
          </button>
          <span
            data-testid="publish-review-walkthrough-progress"
            className="text-xs"
            style={{ color: "var(--text-muted)" }}
          >
            {reviewedCount} of {findings.length} reviewed
          </span>
        </div>
        <div
          data-testid="publish-review-walkthrough-done"
          className="rounded-md px-5 py-8 text-center"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <p className="text-2xl" style={{ color: "var(--accent-primary)" }}>
            {reviewedCount === findings.length ? "✓" : "◔"}
          </p>
          <p
            className="mt-2 text-base font-semibold"
            style={{ color: "var(--text-primary)" }}
          >
            {reviewedCount === findings.length
              ? "All findings reviewed"
              : `${reviewedCount} of ${findings.length} reviewed`}
          </p>
          <p className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
            {blockingCount} blocking{" "}
            {blockingCount === 1 ? "finding" : "findings"} in this review.
          </p>
          <div className="mt-4 flex justify-center gap-2">
            <button
              type="button"
              data-testid="publish-review-walkthrough-restart"
              onClick={restart}
              className="rounded px-2.5 py-1.5 text-xs font-medium hover:bg-[var(--bg-hover)]"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-secondary)",
              }}
            >
              ↺ Start over
            </button>
            <button
              type="button"
              onClick={onExit}
              className="rounded px-2.5 py-1.5 text-xs font-medium"
              style={{
                backgroundColor: "var(--accent-primary)",
                borderColor: "var(--accent-primary)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "#ffffff",
              }}
            >
              ← Back to all changes
            </button>
          </div>
        </div>
      </section>
    );
  }

  const isReviewed = reviewedIds.has(currentFinding.id);
  const severityColor = annotationLevelColor(currentFinding.level);
  return (
    <section data-testid="publish-review-walkthrough" className="p-3">
      <div className="mb-3 flex items-center justify-between gap-3">
        <button
          type="button"
          data-testid="publish-review-walkthrough-exit"
          onClick={onExit}
          className="text-xs font-medium hover:underline"
          style={{ color: "var(--text-secondary)" }}
        >
          ← Back to all changes
        </button>
        <span
          data-testid="publish-review-walkthrough-progress"
          className="text-xs"
          style={{ color: "var(--text-muted)" }}
        >
          {reviewedCount} of {findings.length} reviewed
        </span>
      </div>
      <div className="mb-2 flex items-center justify-between gap-3">
        <span
          data-testid="publish-review-walkthrough-position"
          className="text-xs font-semibold"
          style={{ color: "var(--text-secondary)" }}
        >
          Finding {currentIndex + 1} of {findings.length}
        </span>
        <div className="flex items-center gap-1">
          {findings.map((finding, index) => (
            <Tooltip key={finding.id}>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  data-testid={`publish-review-walkthrough-dot-${index}`}
                  aria-label={`Go to finding ${index + 1}`}
                  aria-current={index === currentIndex ? "step" : undefined}
                  onClick={() => goTo(index)}
                  className="h-2.5 w-6 rounded-full"
                  style={{
                    backgroundColor:
                      index === currentIndex
                        ? "var(--accent-primary)"
                        : reviewedIds.has(finding.id)
                          ? "var(--status-success)"
                          : "var(--border-subtle)",
                    borderColor: "transparent",
                    borderStyle: "solid",
                    borderWidth: "1px",
                  }}
                />
              </TooltipTrigger>
              <TooltipContent side="top">
                <p>Finding {index + 1}</p>
              </TooltipContent>
            </Tooltip>
          ))}
        </div>
      </div>
      <article
        data-testid="publish-review-walkthrough-card"
        className="overflow-hidden rounded-md"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <div
          className="border-l-4 px-4 py-3"
          style={{ borderLeftColor: severityColor }}
        >
          <div className="flex items-center gap-2 text-[0.6875rem]">
            <span
              className="rounded-full px-2 py-0.5 font-semibold"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: severityColor,
                borderStyle: "solid",
                borderWidth: "1px",
                color: severityColor,
              }}
            >
              {severityLabel(currentFinding.level)}
            </span>
            <span style={{ color: "var(--text-muted)" }}>
              {currentFinding.sourceLabel}
            </span>
          </div>
          <h3
            className="mt-2 text-sm font-semibold"
            style={{ color: "var(--text-primary)" }}
          >
            {currentFinding.title}
          </h3>
          <p
            className="mt-1 text-xs leading-5"
            style={{ color: "var(--text-secondary)" }}
          >
            {currentFinding.message}
          </p>
        </div>
        <div
          className="flex items-center gap-3 px-4 py-2 text-xs"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderTopColor: "var(--border-subtle)",
            borderTopStyle: "solid",
            borderTopWidth: "1px",
          }}
        >
          <span
            className="min-w-0 truncate font-mono"
            style={{ color: "var(--text-secondary)" }}
          >
            {currentFinding.path}
          </span>
          <span
            className="min-w-0 truncate font-mono"
            style={{ color: "var(--text-muted)" }}
          >
            {currentFinding.hunkHeader}
          </span>
          {onOpenFile !== undefined && (
            <button
              type="button"
              onClick={() => onOpenFile(currentFinding.path)}
              className="ml-auto inline-flex shrink-0 items-center gap-1 text-xs hover:underline"
              style={{ color: "var(--text-secondary)" }}
            >
              <ExternalLink className="h-3 w-3" aria-hidden="true" />
              Open file
            </button>
          )}
        </div>
        <div
          data-testid="publish-review-walkthrough-hunk"
          className="min-h-12"
          style={{
            borderTopColor: "var(--border-subtle)",
            borderTopStyle: "solid",
            borderTopWidth: "1px",
          }}
        >
          {currentFinding.hunk === undefined ? (
            <div className="px-4 py-3 text-xs" style={{ color: "var(--text-muted)" }}>
              Loading attached hunk…
            </div>
          ) : (
            <SimpleDiffView
              hunks={[currentFinding.hunk]}
              oldTotalLines={
                currentFinding.hunk.oldStart + currentFinding.hunk.oldLines - 1
              }
              newTotalLines={
                currentFinding.hunk.newStart + currentFinding.hunk.newLines - 1
              }
              scrollContainer={false}
              stickyGutter={false}
              showContextGaps={false}
              showWrapToggle={false}
            />
          )}
        </div>
      </article>
      <div className="mt-3 flex items-center justify-between gap-3">
        <div className="flex items-center gap-1.5">
          <ReviewWalkthroughNavigationButton
            testId="publish-review-walkthrough-prev"
            label="Previous finding"
            disabled={currentIndex === 0}
            onClick={previous}
          >
            ◀
          </ReviewWalkthroughNavigationButton>
          <ReviewWalkthroughNavigationButton
            testId="publish-review-walkthrough-next"
            label="Next finding"
            onClick={next}
          >
            ▶
          </ReviewWalkthroughNavigationButton>
          <span
            className="ml-1 text-[0.6875rem]"
            style={{ color: "var(--text-muted)" }}
          >
            K previous · J next
          </span>
        </div>
        <button
          type="button"
          data-testid="publish-review-walkthrough-mark"
          onClick={toggleReviewed}
          className="rounded px-2.5 py-1.5 text-xs font-semibold"
          style={{
            backgroundColor: isReviewed
              ? "var(--status-success-muted)"
              : "var(--bg-elevated)",
            borderColor: isReviewed
              ? "var(--status-success)"
              : "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: isReviewed ? "var(--status-success)" : "var(--text-secondary)",
          }}
        >
          {isReviewed ? "✓ Reviewed" : "○ Mark reviewed"}
        </button>
      </div>
    </section>
  );
}
