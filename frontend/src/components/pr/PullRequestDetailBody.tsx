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
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { openExternalTicketUrl } from "@/components/ticketing/ticketing-open-external";
import { useAfterPaint } from "@/components/ticketing/useAfterPaint";

import {
  DetailSkeleton,
  normalizePrStatus,
  PrMarkdown,
  PrSection,
  PrStateNotice,
} from "./PullRequestDetailPrimitives";
import {
  PrChecksSection,
  PrCommentsSection,
  PrReviewThreadSection,
  PrRxConversationSection,
} from "./PullRequestDetailSections";

export interface PullRequestShell {
  projectId: string;
  prNumber?: number | null | undefined;
  title?: string | null | undefined;
  url?: string | null | undefined;
  status?: string | null | undefined;
  branch?: string | null | undefined;
  conversationId?: string | null | undefined;
}

interface PullRequestDetailBodyProps {
  selector: PullRequestDetailSelector | null;
  shell: PullRequestShell | null;
  className?: string | undefined;
}

export function PullRequestDetailBody({
  selector,
  shell,
  className,
}: PullRequestDetailBodyProps) {
  const shouldFetchDetail = useAfterPaint(Boolean(selector));
  const detailQuery = usePullRequestDetail(selector, { enabled: shouldFetchDetail });
  const detail = detailQuery.data ?? null;
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
      <header className="space-y-3">
        <div className="flex min-w-0 items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <PrStatusBadge status={status} />
              {meta.map((item) => (
                <span key={item} className="text-xs text-[var(--text-muted)]">
                  {item}
                </span>
              ))}
            </div>
            <h2 className="mt-2 line-clamp-2 text-base font-semibold text-[var(--text-primary)]">
              {title}
            </h2>
          </div>
          {url ? (
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
        {headRef ? (
          <div className="flex items-center gap-1.5 text-xs text-[var(--text-muted)]">
            <GitBranch className="h-3.5 w-3.5" aria-hidden="true" />
            <span className="break-all">{headRef}</span>
          </div>
        ) : null}
      </header>

      {detailQuery.isError ? (
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

      <PrSection title="Description">
        {description?.body ? (
          <PrMarkdown content={description.body} />
        ) : loading ? (
          <DetailSkeleton lines={4} />
        ) : (
          <p className="text-sm text-[var(--text-secondary)]">No description provided.</p>
        )}
      </PrSection>

      <PrCommentsSection comments={detail?.issueComments ?? []} loading={loading} />
      <PrReviewThreadSection reviewThread={detail?.reviewThread ?? []} loading={loading} />
      <PrRxConversationSection
        projectId={shell?.projectId ?? selector?.projectId ?? ""}
        conversations={detail?.rxConversations ?? []}
        fallbackConversationId={shell?.conversationId}
      />
      <PrChecksSection detail={detail} conversationId={shell?.conversationId} />

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

export function pullRequestShellFromTicket(input: {
  projectId: string;
  prNumber: number;
  prUrl?: string | null | undefined;
  prStatus?: string | null | undefined;
  title?: string | null | undefined;
}): PullRequestShell {
  return {
    projectId: input.projectId,
    prNumber: input.prNumber,
    url: input.prUrl,
    status: input.prStatus,
    title: input.title ?? `PR #${input.prNumber}`,
  };
}

export function hasPullRequestShell(shell: PullRequestShell | null): shell is PullRequestShell {
  return Boolean(shell?.projectId && (shell.prNumber != null || shell.branch));
}

export function PullRequestShellIcon() {
  return <GitPullRequestArrow className="h-4 w-4" aria-hidden="true" />;
}
