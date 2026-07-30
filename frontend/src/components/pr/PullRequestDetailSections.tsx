import { Suspense, useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Clock,
  ExternalLink,
  XCircle,
} from "lucide-react";
import { useQuery } from "@tanstack/react-query";

import type {
  PullRequestCheck,
  PullRequestDetail,
  PullRequestIssueComment,
} from "@/api/github";
import type { PrDiffAnnotation } from "@/api/diff";
import { diffApi } from "@/api/diff";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import { openExternalTicketUrl } from "@/components/ticketing/ticketing-open-external";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

import {
  DetailSkeleton,
  PrCommentCard,
  PrSection,
} from "./PullRequestDetailPrimitives";
import { bucketCheck, summarizeChecks } from "./pullRequestChecksSummary";

const LazyIntegratedChatPanel = lazyWithRetry(() =>
  import("@/components/Chat/IntegratedChatPanel").then((module) => ({
    default: module.IntegratedChatPanel,
  })),
);

export function PrCommentsSection({
  comments,
  hiddenBotCount = 0,
  loading,
}: {
  comments: PullRequestIssueComment[];
  hiddenBotCount?: number;
  loading: boolean;
}) {
  return (
    <PrSection title="Comments" count={comments.length}>
      {comments.length > 0 ? (
        <div className="space-y-2">
          {comments.map((comment) => (
            <PrCommentCard
              key={comment.id}
              author={comment.author}
              createdAt={comment.createdAt}
              body={comment.body}
              meta={comment.source === "evidence" ? "cached" : undefined}
            />
          ))}
        </div>
      ) : loading ? (
        <DetailSkeleton lines={2} />
      ) : (
        <p className="text-sm text-[var(--text-secondary)]">No comments yet.</p>
      )}
      {hiddenBotCount > 0 ? (
        <p className="text-xs text-[var(--text-muted)]">
          {hiddenBotCount} automated comment{hiddenBotCount === 1 ? "" : "s"} hidden.
        </p>
      ) : null}
    </PrSection>
  );
}

function annotationSummary(annotations: PrDiffAnnotation[]): string {
  if (annotations.length === 0) {
    return "No annotations returned.";
  }
  const failures = annotations.filter((annotation) =>
    /failure|error|high/i.test(annotation.level),
  ).length;
  if (failures > 0) {
    return `${failures} blocking annotation${failures === 1 ? "" : "s"}`;
  }
  return `${annotations.length} annotation${annotations.length === 1 ? "" : "s"}`;
}

export function CheckRow({ check }: { check: PullRequestCheck }) {
  const bucket = bucketCheck(check);
  const { color, Icon } =
    bucket === "passed"
      ? { color: "var(--status-success)", Icon: CheckCircle2 }
      : bucket === "failed"
        ? { color: "var(--status-error)", Icon: XCircle }
        : { color: "var(--status-warning)", Icon: Clock };
  return (
    <div
      className="flex items-center justify-between gap-3 rounded-md px-3 py-2 text-sm"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <span className="flex min-w-0 items-center gap-2">
        <Icon className="h-4 w-4 shrink-0" aria-hidden="true" style={{ color }} />
        <span className="truncate text-[var(--text-primary)]">{check.name}</span>
      </span>
      <span className="flex shrink-0 items-center gap-1.5">
        <span className="text-xs text-[var(--text-muted)]">
          {check.conclusion ?? check.status ?? "pending"}
        </span>
        {check.detailsUrl ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                aria-label={`Open ${check.name} check details`}
                onClick={() => void openExternalTicketUrl(check.detailsUrl!)}
              >
                <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>{`Open ${check.name} check details`}</TooltipContent>
          </Tooltip>
        ) : null}
      </span>
    </div>
  );
}

export function PrChecksSection({
  detail,
  conversationId,
}: {
  detail: PullRequestDetail | null;
  conversationId: string | null | undefined;
}) {
  const [showAll, setShowAll] = useState(false);
  const [annotationsOpen, setAnnotationsOpen] = useState(false);
  const annotationsQuery = useQuery({
    queryKey: ["github-pr", "annotations", conversationId],
    queryFn: () => diffApi.getAgentConversationWorkspacePrAnnotations(conversationId!),
    enabled: annotationsOpen && Boolean(conversationId),
    staleTime: 30_000,
  });
  const annotations = annotationsQuery.data?.annotations ?? [];
  const unavailable = annotationsQuery.data?.sourcesUnavailable ?? [];

  const checks = detail?.checks ?? [];
  const summary = summarizeChecks(checks);
  const visibleChecks = showAll ? checks : summary.failing;
  const hiddenCount = checks.length - summary.failing.length;

  return (
    <PrSection title="Checks" count={checks.length}>
      {checks.length > 0 ? (
        <div className="space-y-2">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs font-medium">
            {summary.passed > 0 ? (
              <span style={{ color: "var(--status-success)" }}>{summary.passed} passed</span>
            ) : null}
            {summary.failed > 0 ? (
              <span style={{ color: "var(--status-error)" }}>{summary.failed} failed</span>
            ) : null}
            {summary.pending > 0 ? (
              <span style={{ color: "var(--status-warning)" }}>{summary.pending} pending</span>
            ) : null}
          </div>
          {visibleChecks.length > 0 ? (
            <div className="space-y-2">
              {visibleChecks.map((check) => (
                <CheckRow key={`${check.name}:${check.detailsUrl ?? ""}`} check={check} />
              ))}
            </div>
          ) : null}
          {hiddenCount > 0 ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="gap-1.5"
              onClick={() => setShowAll((current) => !current)}
              aria-expanded={showAll}
            >
              {showAll ? (
                <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
              ) : (
                <ChevronRight className="h-3.5 w-3.5" aria-hidden="true" />
              )}
              {showAll ? "Show fewer checks" : `Show all ${checks.length} checks`}
            </Button>
          ) : null}
        </div>
      ) : (
        <p className="text-sm text-[var(--text-secondary)]">No check runs yet.</p>
      )}

      <Button
        type="button"
        variant="outline"
        size="sm"
        className="gap-1.5"
        onClick={() => setAnnotationsOpen((current) => !current)}
        disabled={!conversationId}
        aria-expanded={annotationsOpen}
      >
        {annotationsOpen ? (
          <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5" aria-hidden="true" />
        )}
        Annotations
      </Button>
      {annotationsOpen ? (
        <div
          className="rounded-md px-3 py-3 text-sm"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          {annotationsQuery.isLoading ? (
            <DetailSkeleton lines={2} />
          ) : annotationsQuery.isError ? (
            <p className="text-[var(--status-error)]">Could not load annotations.</p>
          ) : (
            <div className="space-y-2">
              <p className="font-medium text-[var(--text-primary)]">
                {annotationSummary(annotations)}
              </p>
              {annotations.slice(0, 5).map((annotation) => (
                <p key={annotation.id} className="text-xs text-[var(--text-secondary)]">
                  {annotation.path ?? "Repository"}: {annotation.title ?? annotation.message}
                </p>
              ))}
              {unavailable.length > 0 ? (
                <p className="text-xs text-[var(--text-muted)]">
                  {unavailable.length} source{unavailable.length === 1 ? "" : "s"} unavailable.
                </p>
              ) : null}
            </div>
          )}
        </div>
      ) : null}
    </PrSection>
  );
}

export function PrRxConversationSection({
  projectId,
  conversations,
  fallbackConversationId,
}: {
  projectId: string;
  conversations: PullRequestDetail["rxConversations"];
  fallbackConversationId?: string | null | undefined;
}) {
  const conversationId = conversations[0]?.conversationId ?? fallbackConversationId ?? null;
  return (
    <PrSection title="Conversation (RX)" count={conversations.length}>
      {conversationId ? (
        <div
          className="h-[360px] overflow-hidden rounded-md"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <Suspense fallback={<DetailSkeleton lines={3} />}>
            <LazyIntegratedChatPanel
              projectId={projectId}
              conversationIdOverride={conversationId}
              storeContextKeyOverride={`pr_detail:${conversationId}`}
              hideHeaderSessionControls
              hideSessionToolbar
              autoFocusInput={false}
              renderComposer={() => null}
              contentWidthClassName="max-w-none"
            />
          </Suspense>
        </div>
      ) : (
        <p className="text-sm text-[var(--text-secondary)]">No RalphX conversation attached.</p>
      )}
    </PrSection>
  );
}
