import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, Clock3, MoreHorizontal, PauseCircle } from "lucide-react";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspacePublicationEvent,
} from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { StatusPill } from "@/components/ui/status-pill";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useAfterPaintMounted } from "./agentDeferredFrame";
import {
  getAgentWorkspaceHoldPresentation,
  getAgentWorkspacePrAutofixFingerprintSpendPresentation,
  type AgentWorkspaceHoldActionKind,
} from "./agentWorkspacePublishState";

const HOLD_DRAWER_TIMELINE_LIMIT = 20;
const HOLD_DRAWER_TIMELINE_STALE_MS = 30_000;

const AUTO_REPAIR_EXPLAINER =
  "When a check fails on this PR, RalphX diagnoses the failure and can push a fix automatically. It pauses like this — instead of spending another attempt — whenever it needs new evidence from GitHub or a decision from you.";

/**
 * Timeline events, kept regardless of status, for the hold-card Details drawer.
 * NON-NEGOTIABLE: do NOT reuse selectPublishHistory here — its failed|succeeded|needs_agent
 * status filter empties it on exactly the hold states this card exists for (hold events
 * carry status "blocked").
 */
// eslint-disable-next-line react-refresh/only-export-components -- shared with the drawer's own test suite
export function selectHoldDrawerTimeline(
  events: AgentConversationWorkspacePublicationEvent[],
): AgentConversationWorkspacePublicationEvent[] {
  const seenEventIds = new Set<string>();
  const deduped = events.filter((event) => {
    if (seenEventIds.has(event.id)) {
      return false;
    }
    seenEventIds.add(event.id);
    return true;
  });
  return deduped.slice(-HOLD_DRAWER_TIMELINE_LIMIT).reverse();
}

function resolveHoldActionHandler(
  kind: AgentWorkspaceHoldActionKind,
  handlers: {
    onRecheck: () => void;
    onRetry: () => void;
    onRetryPublication: () => void;
    onRerunChecks: () => void;
    onStop: () => void;
  },
): () => void {
  switch (kind) {
    case "recheck":
      return handlers.onRecheck;
    case "retryRepair":
      return handlers.onRetry;
    case "retryPublication":
      return handlers.onRetryPublication;
    case "rerunChecks":
      return handlers.onRerunChecks;
    case "stop":
      return handlers.onStop;
  }
}

function formatHoldTimelineEventTime(createdAt: string): string {
  const date = new Date(createdAt);
  if (Number.isNaN(date.getTime())) {
    return createdAt;
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

export function AgentsPublishHoldCard({
  workspace,
  onRecheck,
  onRetry,
  onRetryPublication,
  onRerunChecks,
  onStop,
  isPending = false,
}: {
  workspace: AgentConversationWorkspace;
  onRecheck: () => void;
  onRetry: () => void;
  onRetryPublication: () => void;
  onRerunChecks: () => void;
  onStop: () => void;
  isPending?: boolean;
}) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [hasExpandedOnce, setHasExpandedOnce] = useState(false);
  const afterFirstExpandPaint = useAfterPaintMounted(hasExpandedOnce);
  const timelineQuery = useQuery({
    queryKey: ["agents-publish-hold-timeline", workspace.conversationId],
    queryFn: () =>
      chatApi.listAgentConversationWorkspacePublicationEvents(workspace.conversationId),
    enabled: afterFirstExpandPaint,
    staleTime: HOLD_DRAWER_TIMELINE_STALE_MS,
  });

  const hold = getAgentWorkspaceHoldPresentation(workspace);
  const spend = getAgentWorkspacePrAutofixFingerprintSpendPresentation(workspace);
  if (!hold) {
    return null;
  }

  const actionHandlers = { onRecheck, onRetry, onRetryPublication, onRerunChecks, onStop };
  const timeline = selectHoldDrawerTimeline(timelineQuery.data ?? []);

  return (
    <section
      className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto rounded-lg p-4"
      data-testid="agents-publish-hold-card"
      style={{
        backgroundColor: "var(--bg-surface, #1c1c22)",
        borderColor: "var(--status-warning-border, #8a5a16)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {/* Layer 1 — pill + court chip + spend, one line */}
      <div
        className="flex flex-wrap items-center gap-2"
        data-testid="agents-publish-hold-strip"
      >
        <StatusPill
          label={hold.pill}
          tone="warning"
          variant="tinted"
          icon={<PauseCircle aria-hidden="true" className="h-3 w-3" />}
          testId="agents-publish-hold-pill"
        />
        <StatusPill
          label={hold.courtChip.label}
          tone={hold.courtChip.autoResumes ? "accent" : "neutral"}
          testId="agents-publish-hold-court-chip"
        />
        {spend && (
          <span
            className="inline-flex min-w-0 items-center gap-1 text-[0.6875rem] text-[var(--text-muted)]"
            data-testid="agents-publish-hold-spend"
          >
            <Clock3 aria-hidden="true" className="h-3 w-3 shrink-0" />
            <span className="truncate">
              {spend.summary}
              {spend.exhausted ? " · budget exhausted" : ""}
            </span>
          </span>
        )}
      </div>

      {/* Layer 2 — headline + single paragraph */}
      <div className="min-w-0">
        <h3 className="text-sm font-semibold text-[var(--text-primary)]">{hold.headline}</h3>
        <p className="mt-1 text-xs leading-relaxed text-[var(--text-secondary)]">
          {hold.paragraph}
        </p>
      </div>

      {/* Layer 3 — primary + caption, secondary, More menu */}
      <div
        className="flex flex-wrap items-start gap-3 border-t pt-3"
        style={{
          borderColor: "var(--border-subtle, #3b3b43)",
          borderStyle: "solid",
          borderWidth: "1px 0 0",
        }}
      >
        <div className="flex min-w-0 flex-col gap-1">
          <Button
            type="button"
            onClick={resolveHoldActionHandler(hold.primary.kind, actionHandlers)}
            disabled={isPending}
            data-testid="agents-publish-hold-primary"
          >
            {hold.primary.label}
          </Button>
          <p
            className="max-w-xs text-[0.6875rem] leading-snug text-[var(--text-muted)]"
            data-testid="agents-publish-hold-primary-caption"
          >
            {hold.primary.caption}
          </p>
        </div>

        {hold.secondary && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="outline"
                onClick={resolveHoldActionHandler(hold.secondary.kind, actionHandlers)}
                disabled={isPending}
                data-testid="agents-publish-hold-secondary"
              >
                {hold.secondary.label}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{hold.secondary.tooltip}</TooltipContent>
          </Tooltip>
        )}

        {hold.more.length > 0 && (
          <DropdownMenu>
            <Tooltip>
              <TooltipTrigger asChild>
                <DropdownMenuTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    className="h-8 w-8 border-0 bg-transparent p-0 hover:bg-[var(--bg-hover)]"
                    disabled={isPending}
                    aria-label="More repair actions"
                    data-testid="agents-publish-hold-more-trigger"
                  >
                    <MoreHorizontal className="h-3.5 w-3.5" aria-hidden="true" />
                  </Button>
                </DropdownMenuTrigger>
              </TooltipTrigger>
              <TooltipContent>More repair actions</TooltipContent>
            </Tooltip>
            <DropdownMenuContent align="end" className="min-w-[220px]">
              {hold.more.map((item) => (
                <DropdownMenuItem
                  key={item.kind}
                  onClick={resolveHoldActionHandler(item.kind, actionHandlers)}
                  disabled={isPending}
                  data-testid={`agents-publish-hold-more-${item.kind}`}
                  className="flex-col items-start gap-0.5 py-2"
                >
                  <span className="text-xs font-medium text-[var(--text-primary)]">
                    {item.label}
                  </span>
                  <span className="text-[0.6875rem] leading-snug text-[var(--text-muted)]">
                    {item.caption}
                  </span>
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>

      {/* Layer 4 — Details disclosure, collapsed by default */}
      <Collapsible
        open={detailsOpen}
        onOpenChange={(open) => {
          setDetailsOpen(open);
          if (open) {
            setHasExpandedOnce(true);
          }
        }}
      >
        <CollapsibleTrigger
          type="button"
          className="flex w-full items-center justify-between gap-2 rounded-[4px] px-1.5 py-1 text-left text-xs font-medium text-[var(--text-secondary)] outline-none transition-colors duration-[120ms] hover:bg-[var(--overlay-weak)]"
          data-testid="agents-publish-hold-details-trigger"
        >
          <span>Details</span>
          <ChevronDown
            className="h-3.5 w-3.5 shrink-0 transition-transform duration-[120ms]"
            aria-hidden="true"
            style={{ transform: detailsOpen ? "rotate(180deg)" : "rotate(0deg)" }}
          />
        </CollapsibleTrigger>
        <CollapsibleContent data-testid="agents-publish-hold-details-content">
          <div className="grid gap-4 pt-3 @lg:grid-cols-2">
            <div className="min-w-0 space-y-2" data-testid="agents-publish-hold-timeline">
              <div className="text-[0.6875rem] font-medium uppercase tracking-wide text-[var(--text-muted)]">
                Timeline
              </div>
              {timelineQuery.isLoading ? (
                <p className="text-xs text-[var(--text-muted)]">Loading timeline…</p>
              ) : timeline.length === 0 ? (
                <p className="text-xs text-[var(--text-muted)]">
                  No repair events recorded yet.
                </p>
              ) : (
                <ol className="space-y-2">
                  {timeline.map((event) => (
                    <li
                      key={event.id}
                      className="text-xs"
                      data-testid={`agents-publish-hold-timeline-event-${event.id}`}
                    >
                      <div className="font-medium text-[var(--text-secondary)]">
                        {event.summary}
                      </div>
                      <div className="text-[0.6875rem] capitalize text-[var(--text-muted)]">
                        {event.step.replace(/_/g, " ")} · {event.status.replace(/_/g, " ")}
                        {event.createdAt
                          ? ` · ${formatHoldTimelineEventTime(event.createdAt)}`
                          : ""}
                      </div>
                    </li>
                  ))}
                </ol>
              )}
            </div>

            <div className="min-w-0 space-y-3">
              <div
                className="space-y-1.5"
                data-testid="agents-publish-hold-release-conditions"
              >
                <div className="text-[0.6875rem] font-medium uppercase tracking-wide text-[var(--text-muted)]">
                  Resumes when
                </div>
                <ul className="space-y-1 text-xs leading-relaxed text-[var(--text-secondary)]">
                  {hold.releaseConditions.map((condition) => (
                    <li key={condition}>{condition}</li>
                  ))}
                </ul>
              </div>
              <div className="space-y-1.5" data-testid="agents-publish-hold-explainer">
                <div className="text-[0.6875rem] font-medium uppercase tracking-wide text-[var(--text-muted)]">
                  What is auto-repair?
                </div>
                <p className="text-xs leading-relaxed text-[var(--text-secondary)]">
                  {AUTO_REPAIR_EXPLAINER}
                </p>
              </div>
              {hold.technicalDetails !== null && (
                <div
                  className="space-y-1.5"
                  data-testid="agents-publish-hold-technical-details"
                >
                  <div className="text-[0.6875rem] font-medium uppercase tracking-wide text-[var(--text-muted)]">
                    Technical details
                  </div>
                  <p className="font-mono text-xs leading-relaxed text-[var(--text-secondary)]">
                    {hold.technicalDetails}
                  </p>
                </div>
              )}
            </div>
          </div>
        </CollapsibleContent>
      </Collapsible>
    </section>
  );
}
