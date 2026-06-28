import { lazy, Suspense, useState } from "react";
import { CheckCircle2, ChevronDown, ChevronRight } from "lucide-react";
import { useQuery } from "@tanstack/react-query";

import type {
  PullRequestDetail,
  PullRequestIssueComment,
  PullRequestReviewThreadComment,
} from "@/api/github";
import type { PrDiffAnnotation } from "@/api/diff";
import { diffApi } from "@/api/diff";
import { Button } from "@/components/ui/button";

import {
  DetailSkeleton,
  PrCommentCard,
  PrSection,
} from "./PullRequestDetailPrimitives";

const LazyIntegratedChatPanel = lazy(() =>
  import("@/components/Chat/IntegratedChatPanel").then((module) => ({
    default: module.IntegratedChatPanel,
  })),
);

export function PrCommentsSection({
  comments,
  loading,
}: {
  comments: PullRequestIssueComment[];
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
    </PrSection>
  );
}

export function PrReviewThreadSection({
  reviewThread,
  loading,
}: {
  reviewThread: PullRequestReviewThreadComment[];
  loading: boolean;
}) {
  return (
    <PrSection title="Conversation" count={reviewThread.length}>
      {reviewThread.length > 0 ? (
        <div className="space-y-2">
          {reviewThread.map((comment) => {
            const location = [
              comment.path,
              comment.line != null ? `L${comment.line}` : null,
              comment.isOutdated ? "outdated" : null,
            ]
              .filter(Boolean)
              .join(" · ");
            return (
              <PrCommentCard
                key={comment.id}
                author={comment.author}
                createdAt={comment.createdAt}
                body={comment.body}
                meta={location || undefined}
              />
            );
          })}
        </div>
      ) : loading ? (
        <DetailSkeleton lines={2} />
      ) : (
        <p className="text-sm text-[var(--text-secondary)]">No review thread yet.</p>
      )}
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

export function PrChecksSection({
  detail,
  conversationId,
}: {
  detail: PullRequestDetail | null;
  conversationId: string | null | undefined;
}) {
  const [expanded, setExpanded] = useState(false);
  const annotationsQuery = useQuery({
    queryKey: ["github-pr", "annotations", conversationId],
    queryFn: () => diffApi.getAgentConversationWorkspacePrAnnotations(conversationId!),
    enabled: expanded && Boolean(conversationId),
    staleTime: 30_000,
  });
  const annotations = annotationsQuery.data?.annotations ?? [];
  const unavailable = annotationsQuery.data?.sourcesUnavailable ?? [];

  return (
    <PrSection title="Checks" count={detail?.checks.length ?? 0}>
      {detail?.checks.length ? (
        <div className="space-y-2">
          {detail.checks.map((check) => {
            const result = (check.conclusion ?? check.status ?? "").toLowerCase();
            const healthy = result === "success";
            return (
              <div
                key={`${check.name}:${check.detailsUrl ?? ""}`}
                className="flex items-center justify-between gap-3 rounded-md px-3 py-2 text-sm"
                style={{
                  backgroundColor: "var(--bg-surface)",
                  borderColor: "var(--border-subtle)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                }}
              >
                <span className="flex min-w-0 items-center gap-2">
                  <CheckCircle2
                    className="h-4 w-4 shrink-0"
                    aria-hidden="true"
                    style={{ color: healthy ? "var(--status-success)" : "var(--text-muted)" }}
                  />
                  <span className="truncate text-[var(--text-primary)]">{check.name}</span>
                </span>
                <span className="shrink-0 text-xs text-[var(--text-muted)]">
                  {check.conclusion ?? check.status ?? "pending"}
                </span>
              </div>
            );
          })}
        </div>
      ) : (
        <p className="text-sm text-[var(--text-secondary)]">No check runs yet.</p>
      )}

      <Button
        type="button"
        variant="outline"
        size="sm"
        className="gap-1.5"
        onClick={() => setExpanded((current) => !current)}
        disabled={!conversationId}
        aria-expanded={expanded}
      >
        {expanded ? (
          <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5" aria-hidden="true" />
        )}
        Annotations
      </Button>
      {expanded ? (
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
