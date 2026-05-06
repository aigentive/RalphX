import { Loader2, X } from "lucide-react";
import { useState } from "react";

import type { AgentConversationWorkspacePublicationEvent } from "@/api/chat";

export function PublishEventLog({
  events,
  isLoading,
  isPublishing,
}: {
  events: AgentConversationWorkspacePublicationEvent[];
  isLoading: boolean;
  isPublishing: boolean;
}) {
  const [isExpanded, setIsExpanded] = useState(false);
  if (isLoading && events.length === 0) {
    return (
      <div className="text-xs text-[var(--text-muted)]">
        Loading publish history...
      </div>
    );
  }

  if (events.length === 0) {
    return null;
  }

  const activeStartedEventId =
    isPublishing && events.length > 0
      ? [...events].reverse().find((event) => event.status === "started")?.id
      : null;
  const visibleEvents = events
    .filter((event) =>
      event.status === "failed" ||
      event.status === "succeeded" ||
      event.status === "needs_agent" ||
      event.id === activeStartedEventId
    )
    .slice(-6)
    .reverse();

  if (visibleEvents.length === 0) {
    return null;
  }

  return (
    <div className="px-1" data-testid="agents-publish-events">
      <button
        type="button"
        className="flex items-center gap-2 bg-transparent p-0 text-[0.6875rem] font-medium text-[var(--text-muted)] transition-colors hover:text-[var(--text-secondary)]"
        onClick={() => setIsExpanded((current) => !current)}
        data-theme-button-skip="true"
        data-testid="agents-publish-history-toggle"
      >
        <span>{isExpanded ? "Hide publish history" : "Show publish history"}</span>
        <span className="text-[0.625rem] text-[var(--text-muted)]">
          {visibleEvents.length}
        </span>
      </button>
      {isExpanded && (
        <div
          className="mt-3 space-y-2 border-l pl-3"
          style={{ borderColor: "var(--overlay-weak)" }}
        >
          {visibleEvents.map((event) => {
            const eventState =
              event.status === "failed" || event.status === "succeeded"
                ? event.status
                : event.id === activeStartedEventId
                  ? "active"
                  : "history";
            return (
              <div
                key={event.id}
                className="flex items-start gap-2 text-xs"
                data-testid={`agents-publish-event-${event.step}`}
              >
                <span
                  className="mt-1 flex h-3 w-3 shrink-0 items-center justify-center rounded-full"
                  data-state={eventState}
                  data-testid={`agents-publish-event-icon-${event.id}`}
                  style={{
                    background:
                      eventState === "failed"
                        ? "var(--status-danger)"
                        : eventState === "active"
                          ? "var(--accent-primary)"
                          : "var(--overlay-weak)",
                    color:
                      eventState === "failed"
                        ? "var(--status-danger)"
                        : eventState === "active"
                          ? "var(--accent-primary)"
                          : "var(--text-muted)",
                  }}
                >
                  {eventState === "failed" ? (
                    <X className="h-2.5 w-2.5 text-[var(--bg-base)]" />
                  ) : eventState === "active" ? (
                    <Loader2 className="h-2.5 w-2.5 animate-spin text-[var(--bg-base)]" />
                  ) : (
                    <span className="h-1.5 w-1.5 rounded-full bg-[var(--bg-base)]" />
                  )}
                </span>
                <div className="min-w-0">
                  <div className="font-medium text-[var(--text-secondary)]">
                    {event.summary}
                  </div>
                  <div className="mt-0.5 text-[0.6875rem] capitalize text-[var(--text-muted)]">
                    {event.step.replace(/_/g, " ")}
                    {event.classification
                      ? ` / ${event.classification.replace(/_/g, " ")}`
                      : ""}
                    {event.createdAt ? ` / ${formatPublishEventTime(event.createdAt)}` : ""}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
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
