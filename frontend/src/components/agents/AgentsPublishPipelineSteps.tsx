import { CheckCircle2, Loader2, X } from "lucide-react";

import { cn } from "@/lib/utils";

const PUBLISH_STEPS = [
  { id: "checking", label: "Check workspace" },
  { id: "committing", label: "Commit changes" },
  { id: "refreshing", label: "Refresh branch" },
  { id: "pushing", label: "Push branch" },
  { id: "pushed", label: "Open draft PR" },
] as const;

export function PublishPipelineSteps({
  className,
  status,
  isPublishing,
  testIdPrefix = "agents-publish",
}: {
  className?: string;
  status: string | null;
  isPublishing: boolean;
  testIdPrefix?: string;
}) {
  const normalizedStatus = status ?? "idle";
  const activeIndex = (() => {
    if (normalizedStatus === "pushed") {
      return PUBLISH_STEPS.length;
    }
    if (normalizedStatus === "pushing") {
      return 3;
    }
    if (normalizedStatus === "refreshed") {
      return 3;
    }
    if (normalizedStatus === "refreshing") {
      return 2;
    }
    if (normalizedStatus === "committing") {
      return 1;
    }
    return 0;
  })();
  const isRepairStatus = normalizedStatus === "needs_agent";
  const isTerminalFailure = normalizedStatus === "failed" || isRepairStatus;

  return (
    <div
      className={cn("mt-4 rounded-md border p-3", className)}
      style={{
        backgroundColor: "var(--bg-subtle)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid={`${testIdPrefix}-pipeline`}
    >
      <div className="mb-2 text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--text-muted)]">
        Publish pipeline
      </div>
      <div className="grid gap-2 sm:grid-cols-5">
        {PUBLISH_STEPS.map((step, index) => {
          const isDone = activeIndex > index;
          const isActive = isPublishing && activeIndex === index;
          const isFailed = isTerminalFailure && index === 0;
          return (
            <div
              key={step.id}
              className="grid min-w-0 grid-cols-[1.25rem_minmax(0,1fr)] items-center gap-2 text-xs"
              data-testid={`${testIdPrefix}-step-${step.id}`}
              style={{
                color:
                  isDone || isActive || isFailed
                    ? "var(--text-primary)"
                    : "var(--text-muted)",
              }}
            >
              <span
                className="flex h-5 w-5 items-center justify-center rounded-full border"
                style={{
                  borderColor: isFailed
                    ? "var(--status-danger)"
                    : isDone
                      ? "var(--status-success)"
                      : isActive
                        ? "var(--accent-primary)"
                        : "var(--overlay-weak)",
                  color: isFailed
                    ? "var(--status-danger)"
                    : isDone
                      ? "var(--status-success)"
                      : isActive
                        ? "var(--accent-primary)"
                        : "var(--text-muted)",
                }}
              >
                {isActive ? (
                  <Loader2 className="h-3 w-3 animate-spin" />
                ) : isDone ? (
                  <CheckCircle2 className="h-3 w-3" />
                ) : isFailed ? (
                  <X className="h-3 w-3" />
                ) : (
                  index + 1
                )}
              </span>
              <span className="min-w-0 leading-snug">{step.label}</span>
            </div>
          );
        })}
      </div>
      {isTerminalFailure && (
        <div className="mt-3 text-xs text-[var(--text-muted)]">
          {isRepairStatus
            ? "The latest publish attempt found a fixable issue and sent it back to the workspace agent."
            : "The latest publish attempt failed. Fixable errors are sent back to the workspace agent."}
        </div>
      )}
    </div>
  );
}
