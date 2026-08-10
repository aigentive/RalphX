import { useMemo } from "react";
import {
  AlertTriangle,
  ExternalLink,
  GitBranch,
  GitPullRequestArrow,
  MessageSquare,
} from "lucide-react";

import type { PullRequestDetailSelector } from "@/hooks/usePullRequestDetail";
import { usePullRequestDetail } from "@/hooks/usePullRequestDetail";
import { PrStatusBadge } from "@/components/tasks/detail-views/shared/PrStatusBadge";
import { Button } from "@/components/ui/button";
import { NoticeBanner } from "@/components/ui/notice-banner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { remoteErrorBannerProps } from "@/lib/remote/agent-gate";
import { openExternalTicketUrl } from "@/components/ticketing/ticketing-open-external";
import { useAfterPaint } from "@/components/ticketing/useAfterPaint";

import {
  DetailSkeleton,
  PrMarkdown,
  PrSection,
  PrStateNotice,
} from "./PullRequestDetailPrimitives";
import type { PullRequestShell } from "./PullRequestDetailShell";
import { normalizePrStatus } from "./PullRequestDetailUtils";
import {
  PrChecksSection,
  PrCommentsSection,
  PrRxConversationSection,
} from "./PullRequestDetailSections";
import { PullRequestReviewSection } from "./PullRequestReviewSection";
import { PullRequestStatusStrip } from "./PullRequestStatusStrip";
import { partitionIssueComments } from "./pullRequestComments";

interface PullRequestDetailBodyProps {
  selector: PullRequestDetailSelector | null;
  shell: PullRequestShell | null;
  className?: string | undefined;
  /**
   * Whether to embed the linked RalphX conversation. Defaults to true.
   * The agent-workspace PR tab sets this false because it already renders
   * inside that same conversation, making the embed redundant.
   */
  showRxConversation?: boolean | undefined;
  presentation?: "default" | "agentsWorkspace" | undefined;
}

export function PullRequestDetailBody({
  selector,
  shell,
  className,
  showRxConversation = true,
  presentation = "default",
}: PullRequestDetailBodyProps) {
  const compactAgentsHeader = presentation === "agentsWorkspace";
  const shouldFetchDetail = useAfterPaint(Boolean(selector));
  const detailQuery = usePullRequestDetail(selector, {
    enabled: shouldFetchDetail,
  });
  const detail = detailQuery.data ?? null;
  const remoteError = remoteErrorBannerProps(detailQuery.error);
  const description = detail?.description ?? null;
  const prNumber = description?.number ?? shell?.prNumber ?? selector?.prNumber ?? null;
  const title = description?.title ?? shell?.title ?? (prNumber ? `PR #${prNumber}` : "Pull request");
  const url = description?.url ?? shell?.url ?? null;
  const status = normalizePrStatus(
    description?.state ?? shell?.status ?? null,
    description?.isDraft,
  );
  const headRef = description?.headRefName ?? shell?.branch ?? selector?.branch ?? null;
  const baseRef = description?.baseRefName ?? null;
  const unavailable = detail?.sourcesUnavailable ?? [];
  const state = detail?.state ?? "loaded";
  const loading =
    detailQuery.isLoading ||
    (shouldFetchDetail && !detail && detailQuery.fetchStatus !== "idle");

  const reviewSummary = detail?.reviewSummary ?? null;
  const checks = detail?.checks ?? [];
  const checksUnavailable = unavailable.includes("checks");
  const issueComments = detail?.issueComments;
  const { human: humanComments, hiddenBotCount } = useMemo(
    () => partitionIssueComments(issueComments ?? []),
    [issueComments],
  );

  const meta = useMemo(
    () =>
      [
        prNumber ? `#${prNumber}` : null,
        headRef ? `head ${headRef}` : null,
        baseRef ? `base ${baseRef}` : null,
      ].filter(Boolean),
    [baseRef, headRef, prNumber],
  );

  return (
    <div className={cn("space-y-6 p-5", className)} data-testid="pull-request-detail-body">
      <header className={compactAgentsHeader ? undefined : "space-y-3"}>
        <div className="flex min-w-0 items-start justify-between gap-3">
          <div className="min-w-0">
            {!compactAgentsHeader ? (
              <div className="flex flex-wrap items-center gap-2">
                {status ? <PrStatusBadge status={status} /> : null}
                {meta.map((item) => (
                  <span key={item} className="text-xs text-[var(--text-muted)]">
                    {item}
                  </span>
                ))}
              </div>
            ) : null}
            <h2
              className={cn(
                "line-clamp-2 text-base font-semibold text-[var(--text-primary)]",
                !compactAgentsHeader && "mt-2",
              )}
            >
              {title}
            </h2>
          </div>
          {url && !compactAgentsHeader ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0"
                  aria-label="Open pull request in GitHub"
                  onClick={() => void openExternalTicketUrl(url)}
                >
                  <ExternalLink className="h-4 w-4" aria-hidden="true" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Open pull request in GitHub</TooltipContent>
            </Tooltip>
          ) : null}
        </div>
        {headRef && !compactAgentsHeader ? (
          <div className="flex items-center gap-1.5 text-xs text-[var(--text-muted)]">
            <GitBranch className="h-3.5 w-3.5" aria-hidden="true" />
            <span className="break-all">{headRef}</span>
          </div>
        ) : null}
      </header>

      {remoteError ? (
        <NoticeBanner tone={remoteError.tone} title={remoteError.title}>
          {remoteError.body}
        </NoticeBanner>
      ) : detailQuery.isError ? (
        <div
          className="flex items-start gap-2 rounded-md px-3 py-3 text-sm text-[var(--status-error)]"
          style={{
            backgroundColor: "var(--status-error-muted)",
            borderColor: "var(--status-error-border)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
          Could not load pull request details.
        </div>
      ) : null}

      <PrStateNotice state={state} />

      {!compactAgentsHeader ? (
        <PullRequestStatusStrip
          reviewSummary={reviewSummary}
          checks={checks}
          checksUnavailable={checksUnavailable}
          loading={loading}
        />
      ) : null}

      <PrSection title="Description">
        {description?.body ? (
          <PrMarkdown content={description.body} />
        ) : loading ? (
          <DetailSkeleton lines={4} />
        ) : (
          <p className="text-sm text-[var(--text-secondary)]">No description provided.</p>
        )}
      </PrSection>

      <PullRequestReviewSection
        reviewSummary={reviewSummary}
        reviewThread={detail?.reviewThread ?? []}
        loading={loading}
      />
      <PrChecksSection detail={detail} conversationId={shell?.conversationId} />
      {showRxConversation ? (
        <PrRxConversationSection
          projectId={shell?.projectId ?? selector?.projectId ?? ""}
          conversations={detail?.rxConversations ?? []}
          fallbackConversationId={shell?.conversationId}
        />
      ) : null}
      <PrCommentsSection
        comments={humanComments}
        hiddenBotCount={hiddenBotCount}
        loading={loading}
      />

      {unavailable.length > 0 ? (
        <div className="flex items-start gap-2 text-xs text-[var(--text-muted)]">
          <MessageSquare className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span>
            {unavailable.length} source{unavailable.length === 1 ? "" : "s"} unavailable.
          </span>
        </div>
      ) : null}
    </div>
  );
}

export function PullRequestShellIcon() {
  return <GitPullRequestArrow className="h-4 w-4" aria-hidden="true" />;
}
