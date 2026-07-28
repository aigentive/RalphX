import { SimpleDiffView } from "@/components/diff/SimpleDiffView";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { ReviewWalkthroughFinding } from "./ReviewWalkthrough";

interface ReviewWalkthroughNavigationButtonProps {
  testId: string;
  label: string;
  children: string;
  disabled?: boolean;
  onClick: () => void;
}

export function ReviewWalkthroughNavigationButton({
  testId,
  label,
  children,
  disabled,
  onClick,
}: ReviewWalkthroughNavigationButtonProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          data-testid={testId}
          aria-label={label}
          disabled={disabled}
          onClick={onClick}
          className="flex h-7 w-7 items-center justify-center rounded text-xs transition-colors hover:bg-[var(--bg-hover)] disabled:cursor-not-allowed disabled:opacity-40"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent side="top">
        <p>{label}</p>
      </TooltipContent>
    </Tooltip>
  );
}

interface ReviewWalkthroughActionButtonProps {
  testId: string;
  children: string;
  onClick: () => void;
}

function ReviewWalkthroughActionButton({
  testId,
  children,
  onClick,
}: ReviewWalkthroughActionButtonProps) {
  return (
    <button
      type="button"
      data-testid={testId}
      onClick={onClick}
      className="rounded px-2 py-0.5 text-xs font-medium hover:bg-[var(--bg-hover)]"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
        color: "var(--text-secondary)",
      }}
    >
      {children}
    </button>
  );
}

interface ReviewWalkthroughHunkProps {
  finding: ReviewWalkthroughFinding;
  onRetryHunk?: ((path: string) => void) | undefined;
  onLoadHunkAnyway?: ((path: string) => void) | undefined;
}

/**
 * Renders the finding's attached hunk, or the exact reason it is absent. Every
 * non-ready state is distinguishable so a failed or gated fetch never presents
 * as indefinite loading.
 */
export function ReviewWalkthroughHunk({
  finding,
  onRetryHunk,
  onLoadHunkAnyway,
}: ReviewWalkthroughHunkProps) {
  if (finding.hunk !== undefined) {
    return (
      <SimpleDiffView
        hunks={[finding.hunk]}
        oldTotalLines={finding.hunk.oldStart + finding.hunk.oldLines - 1}
        newTotalLines={finding.hunk.newStart + finding.hunk.newLines - 1}
        scrollContainer={false}
        stickyGutter={false}
        showContextGaps={false}
        showWrapToggle={false}
      />
    );
  }

  if (finding.hunkStatus === "error") {
    return (
      <div
        data-testid="publish-review-walkthrough-hunk-error"
        className="flex flex-wrap items-center gap-2 px-4 py-3 text-xs"
        style={{ color: "var(--status-error)" }}
      >
        <span>Could not load the attached hunk for this file.</span>
        {onRetryHunk !== undefined && (
          <ReviewWalkthroughActionButton
            testId="publish-review-walkthrough-hunk-retry"
            onClick={() => onRetryHunk(finding.path)}
          >
            ↻ Retry
          </ReviewWalkthroughActionButton>
        )}
      </div>
    );
  }

  if (finding.hunkStatus === "blocked") {
    return (
      <div
        data-testid="publish-review-walkthrough-hunk-blocked"
        className="flex flex-wrap items-center gap-2 px-4 py-3 text-xs"
        style={{ color: "var(--text-muted)" }}
      >
        <span>This is a generated file, so its diff is not loaded automatically.</span>
        {onLoadHunkAnyway !== undefined && (
          <ReviewWalkthroughActionButton
            testId="publish-review-walkthrough-hunk-load"
            onClick={() => onLoadHunkAnyway(finding.path)}
          >
            Load hunk anyway
          </ReviewWalkthroughActionButton>
        )}
      </div>
    );
  }

  if (finding.hunkStatus === "unavailable") {
    return (
      <div
        data-testid="publish-review-walkthrough-hunk-unavailable"
        className="px-4 py-3 text-xs"
        style={{ color: "var(--text-muted)" }}
      >
        This hunk is no longer present in the current diff. The finding may refer to
        code that has since changed.
      </div>
    );
  }

  return (
    <div
      data-testid="publish-review-walkthrough-hunk-loading"
      className="px-4 py-3 text-xs"
      style={{ color: "var(--text-muted)" }}
    >
      Loading attached hunk…
    </div>
  );
}
