import { Loader2, X } from "lucide-react";

import type { AgentConversationWorkspacePublicationEvent } from "@/api/chat";

/**
 * Single source of truth for which publication events the History tab actually
 * renders. Used by both the timeline and the History tab badge so the count
 * never disagrees with the rendered rows.
 */
// eslint-disable-next-line react-refresh/only-export-components -- shared with the History tab badge
export function selectPublishHistory(
  events: AgentConversationWorkspacePublicationEvent[],
  isPublishing: boolean,
  currentAttemptId: string | null = null,
): {
  activeStartedEventId: string | null;
  visibleEvents: AgentConversationWorkspacePublicationEvent[];
} {
  const attemptEvents = currentAttemptId
    ? events.filter(
        (event) => event.attemptId === currentAttemptId || event.attemptId === null,
      )
    : events;
  const currentAttemptEvents = currentAttemptId
    ? events.filter((event) => event.attemptId === currentAttemptId)
    : attemptEvents;
  const activeStartedEventId =
    isPublishing && currentAttemptEvents.length > 0
      ? ([...currentAttemptEvents]
          .reverse()
          .find((event) => event.status === "started")?.id ?? null)
      : null;
  const visibleEvents = attemptEvents
    .filter(
      (event) =>
        event.status === "failed" ||
        event.status === "succeeded" ||
        event.status === "needs_agent" ||
        event.id === activeStartedEventId,
    )
    .slice(-6)
    .reverse();
  return { activeStartedEventId, visibleEvents };
}

export function PublishEventLog({
  events,
  isLoading,
  isPublishing,
  currentAttemptId = null,
}: {
  events: AgentConversationWorkspacePublicationEvent[];
  isLoading: boolean;
  isPublishing: boolean;
  currentAttemptId?: string | null;
}) {
  if (isLoading && events.length === 0) {
    return (
      <div className="text-xs text-[var(--text-muted)]">
        Loading publish history…
      </div>
    );
  }

  if (events.length === 0) {
    return null;
  }

  const { activeStartedEventId, visibleEvents } = selectPublishHistory(
    events,
    isPublishing,
    currentAttemptId,
  );

  if (visibleEvents.length === 0) {
    return null;
  }

  return (
    <div className="space-y-3" data-testid="agents-publish-events">
      <div className="flex items-center gap-2 text-[0.6875rem] font-medium uppercase tracking-wide text-[var(--text-muted)]">
        <span>Publish history</span>
        <span className="text-[0.625rem] font-normal normal-case text-[var(--text-muted)]">
          {visibleEvents.length}
        </span>
      </div>
      <ol className="space-y-3">
        {visibleEvents.map((event) => {
          const eventState =
            event.status === "failed" || event.status === "succeeded"
              ? event.status
              : event.id === activeStartedEventId
                ? "active"
                : "history";
          const meta = [
            event.step.replace(/_/g, " "),
            event.classification
              ? event.classification.replace(/_/g, " ")
              : null,
            event.createdAt ? formatPublishEventTime(event.createdAt) : null,
          ].filter(Boolean);
          return (
            <li
              key={event.id}
              className="flex items-start gap-3 text-xs"
              data-testid={`agents-publish-event-${event.step}`}
            >
              <span
                className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full"
                data-state={eventState}
                data-testid={`agents-publish-event-icon-${event.id}`}
                style={{
                  backgroundColor:
                    eventState === "failed"
                      ? "var(--status-error, #d55e00)"
                      : eventState === "active"
                        ? "var(--accent-primary)"
                        : "var(--bg-elevated, #232329)",
                  borderColor:
                    eventState === "failed"
                      ? "var(--status-error, #d55e00)"
                      : eventState === "active"
                        ? "var(--accent-primary)"
                        : "var(--border-default, #393940)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                }}
              >
                {eventState === "failed" ? (
                  <X className="h-2.5 w-2.5 text-[var(--bg-base,#151519)]" />
                ) : eventState === "active" ? (
                  <Loader2 className="h-2.5 w-2.5 animate-spin text-[var(--bg-base,#151519)]" />
                ) : (
                  <span className="h-1.5 w-1.5 rounded-full bg-[var(--text-muted,#8e8e93)]" />
                )}
              </span>
              <div className="min-w-0 space-y-0.5">
                <div className="font-medium text-[var(--text-secondary)]">
                  {event.summary}
                </div>
                {meta.length > 0 ? (
                  <div className="text-[0.6875rem] capitalize text-[var(--text-muted)]">
                    {meta.join(" · ")}
                  </div>
                ) : null}
              </div>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
function formatPublishEventTime(createdAt: string): string {
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
