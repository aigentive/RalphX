import type { ReactNode } from "react";
import { TextBubble } from "./TextBubble";
import { ToolCallIndicator } from "./ToolCallIndicator";
import type { TaskCardTranscriptEntry } from "./TaskCardTranscript.utils";

function ActivityDots() {
  return (
    <div className="flex items-center gap-1" aria-hidden="true">
      <div
        className="w-1.5 h-1.5 rounded-full animate-pulse"
        style={{ backgroundColor: "var(--accent-primary)" }}
      />
      <div
        className="w-1.5 h-1.5 rounded-full animate-pulse"
        style={{ backgroundColor: "var(--accent-primary)", animationDelay: "0.15s" }}
      />
      <div
        className="w-1.5 h-1.5 rounded-full animate-pulse"
        style={{ backgroundColor: "var(--accent-primary)", animationDelay: "0.3s" }}
      />
    </div>
  );
}

export function TaskCardTranscriptView({
  entries,
  dataTestId,
  emptyState,
}: {
  entries: TaskCardTranscriptEntry[];
  dataTestId?: string | undefined;
  emptyState?: ReactNode;
}) {
  if (entries.length === 0) {
    return emptyState ?? null;
  }

  return (
    <div
      className="space-y-3"
      data-testid={dataTestId}
    >
      {entries.map((entry) => (
        <div key={entry.id} className="space-y-2" data-testid="task-card-transcript-message">
          {entry.speakerLabel && (
            <div
              className="text-[0.625rem] uppercase tracking-[0.08em]"
              style={{ color: "var(--text-muted)" }}
            >
              {entry.speakerLabel}
            </div>
          )}

          <div className="space-y-2">
            {entry.blocks.map((block, index) => {
              if (block.type === "text") {
                return (
                  <TextBubble
                    key={`${entry.id}-text-${index}`}
                    text={block.text}
                    isUser={entry.role === "user"}
                  />
                );
              }

              if (block.type === "tool_call") {
                return (
                  <ToolCallIndicator
                    key={block.toolCall.id}
                    compact
                    toolCall={block.toolCall}
                  />
                );
              }

              return (
                <div
                  key={`${entry.id}-activity-${index}`}
                  className="flex items-center gap-2 text-xs"
                  data-testid="task-card-transcript-activity"
                >
                  <ActivityDots />
                  <span style={{ color: "var(--text-muted)" }}>
                    {block.label}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}
