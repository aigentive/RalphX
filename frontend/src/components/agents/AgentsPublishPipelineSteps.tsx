import { CheckCircle2, Loader2, X } from "lucide-react";

const PUBLISH_STEPS = [
  { id: "checking", label: "Check workspace" },
  { id: "committing", label: "Commit changes" },
  { id: "refreshing", label: "Refresh branch" },
  { id: "describing", label: "Draft PR description" },
  { id: "pushing", label: "Push branch" },
  { id: "pushed", label: "Open draft PR" },
] as const;

export function PublishPipelineSteps({
  status,
  isPublishing,
}: {
  status: string | null;
  isPublishing: boolean;
}) {
  const normalizedStatus = status ?? "idle";
  const activeIndex = (() => {
    if (normalizedStatus === "pushed") {
      return PUBLISH_STEPS.length;
    }
    if (normalizedStatus === "pushing") {
      return 4;
    }
    if (normalizedStatus === "describing") {
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
  const isDescriptionFailure = normalizedStatus === "description_failed";
  const isTerminalFailure = normalizedStatus === "failed" || isRepairStatus || isDescriptionFailure;
  const failureIndex = isDescriptionFailure ? 3 : 0;

  return (
    <div
      className="mt-4 rounded-md border p-3"
      style={{
        background: "var(--bg-subtle)",
        borderColor: "var(--border-subtle)",
      }}
      data-testid="agents-publish-pipeline"
    >
      <div className="mb-2 text-[0.6875rem] font-semibold uppercase tracking-[0.18em] text-[var(--text-muted)]">
        Publish pipeline
      </div>
      <div className="grid gap-2 sm:grid-cols-6">
        {PUBLISH_STEPS.map((step, index) => {
          const isDone = activeIndex > index;
          const isActive = isPublishing && activeIndex === index;
          const isFailed = isTerminalFailure && index === failureIndex;
          return (
            <div
              key={step.id}
              className="flex items-center gap-2 text-xs"
              data-testid={`agents-publish-step-${step.id}`}
              style={{
                color:
                  isDone || isActive || isFailed
                    ? "var(--text-primary)"
                    : "var(--text-muted)",
              }}
            >
              <span
                className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full border"
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
              <span>{step.label}</span>
            </div>
          );
        })}
      </div>
      {isTerminalFailure && (
        <div className="mt-3 text-xs text-[var(--text-muted)]">
          {isRepairStatus
            ? "The latest publish attempt found a fixable issue and sent it back to the workspace agent."
            : isDescriptionFailure
              ? "RalphX could not draft a PR description, so no pull request was opened."
            : "The latest publish attempt failed. Fixable errors are sent back to the workspace agent."}
        </div>
      )}
    </div>
  );
}
