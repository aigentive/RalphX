import { Suspense } from "react";

import { lazyWithRetry } from "@/lib/lazy-with-retry";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { useArtifact } from "@/hooks/useArtifacts";

/**
 * Markdown modal for a run's plan artifact, opened from the plan icon on run
 * cards and goal-item rows. The dialog shell paints synchronously; the artifact
 * fetch is keyed on `open` and the markdown chunk (shared with
 * `AutomationSpecView`) mounts only after a paint boundary. Missing/failed
 * artifact reads render an explicit error state — never a blank success.
 */

const LazyPlanMarkdown = lazyWithRetry(async () => {
  const [{ default: ReactMarkdown }, { default: remarkGfm }, { markdownComponents }] =
    await Promise.all([
      import("react-markdown"),
      import("remark-gfm"),
      import("@/components/Chat/MessageItem.markdown"),
    ]);

  return {
    default: function PlanMarkdown({ text }: { text: string }) {
      return (
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
          {text}
        </ReactMarkdown>
      );
    },
  };
});

function PlainPlanContent({ text }: { text: string }) {
  return (
    <p
      className="whitespace-pre-wrap break-words text-xs leading-5"
      style={{ color: "var(--text-secondary, #c7c7cc)" }}
    >
      {text}
    </p>
  );
}

function PlanDialogBody({ planArtifactId }: { planArtifactId: string }) {
  // Mounted only while the dialog is open, so the query starts on open and the
  // markdown chunk hydrates after the shell has painted.
  const markdownMounted = useAfterPaintMounted(true);
  const artifact = useArtifact(planArtifactId);

  if (artifact.isLoading) {
    return (
      <div className="space-y-2" data-testid="automation-plan-dialog-loading">
        <Skeleton className="h-4 w-3/4" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-5/6" />
      </div>
    );
  }
  if (artifact.isError || !artifact.data) {
    return (
      <p
        className="text-sm"
        style={{ color: "var(--status-error)" }}
        data-testid="automation-plan-dialog-error"
      >
        Could not load the plan document. It may have been archived or removed.
      </p>
    );
  }

  const content = artifact.data.content ?? "";
  if (!content.trim()) {
    return (
      <p
        className="text-sm"
        style={{ color: "var(--text-muted, #8e8e96)" }}
        data-testid="automation-plan-dialog-empty"
      >
        The plan document has no content yet.
      </p>
    );
  }

  return (
    <div className="max-w-none text-sm" data-testid="automation-plan-dialog-markdown">
      {markdownMounted ? (
        <Suspense fallback={<PlainPlanContent text={content} />}>
          <LazyPlanMarkdown text={content} />
        </Suspense>
      ) : (
        <PlainPlanContent text={content} />
      )}
    </div>
  );
}

export function AutomationPlanDialog({
  planArtifactId,
  heading = "Run plan",
  title,
  open,
  onOpenChange,
}: {
  planArtifactId: string | null;
  /** Visible dialog heading; existing run-plan callers keep the default. */
  heading?: string;
  /** Context line, e.g. the goal item or run the plan belongs to. */
  title?: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-h-[80vh] max-w-2xl overflow-hidden"
        data-testid="automation-plan-dialog"
      >
        <DialogHeader className="flex-col items-start gap-1 pr-12">
          <DialogTitle>{heading}</DialogTitle>
          <DialogDescription>
            {title?.trim() || "Plan produced by this automation run."}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 overflow-y-auto px-6 py-4">
          {open && planArtifactId ? (
            <PlanDialogBody planArtifactId={planArtifactId} />
          ) : (
            <p
              className="text-sm"
              style={{ color: "var(--status-error)" }}
              data-testid="automation-plan-dialog-error"
            >
              No plan document is linked to this run.
            </p>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
