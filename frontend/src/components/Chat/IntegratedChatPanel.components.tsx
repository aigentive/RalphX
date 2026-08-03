/**
 * IntegratedChatPanel.components - Sub-components for IntegratedChatPanel
 */

import { useEffect, useState } from "react";
import { Bot, MessageSquare, CheckSquare, FolderKanban, Hammer, Activity, X, History, GitMerge } from "lucide-react";
import { Button } from "@/components/ui/button";
import { withAlpha } from "@/lib/theme-colors";
import { cn } from "@/lib/utils";
import type { ChatContext } from "@/types/chat";
import { selectActiveAgentRunMeta, useChatStore } from "@/stores/chatStore";
import { elapsedLabel } from "./run-attribution";

// ============================================================================
// CSS Animations
// ============================================================================

export const animationStyles = `
@keyframes typingBounce {
  0%, 60%, 100% { transform: translateY(0); }
  30% { transform: translateY(-4px); }
}

.typing-dot {
  animation: typingBounce 1.4s ease-in-out infinite;
}

.typing-dot:nth-child(2) { animation-delay: 0.15s; }
.typing-dot:nth-child(3) { animation-delay: 0.3s; }
`;

// ============================================================================
// Sub-components
// ============================================================================

export function TypingIndicator({ label, storeKey }: { label?: string | undefined; storeKey?: string | undefined }) {
  const activeRunMeta = useChatStore(selectActiveAgentRunMeta(storeKey ?? ""));
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!activeRunMeta) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [activeRunMeta]);
  const liveLabel = activeRunMeta
    ? elapsedLabel(activeRunMeta.startedAt, activeRunMeta.launchRole, now)
    : label;
  return (
    <div
      data-testid="chat-typing-indicator"
      className="flex items-start gap-2 mb-2"
    >
      <Bot
        className="w-3.5 h-3.5 mt-2 shrink-0"
        style={{ color: "var(--text-muted)" }}
      />
      <div
        className="px-3 py-2 rounded-lg"
        style={{
          /* macOS Tahoe: flat solid color, no gradient, no border */
          backgroundColor: "var(--bg-elevated)",
        }}
      >
        <div className="flex items-center gap-2">
          {liveLabel ? (
            <span
              className="text-[0.75rem] font-medium"
              style={{ color: "var(--text-secondary)" }}
            >
              {liveLabel}
            </span>
          ) : null}
          <div
            className="typing-dot w-1.5 h-1.5 rounded-full"
            style={{ backgroundColor: "var(--text-muted)" }}
          />
          <div
            className="typing-dot w-1.5 h-1.5 rounded-full"
            style={{ backgroundColor: "var(--text-muted)" }}
          />
          <div
            className="typing-dot w-1.5 h-1.5 rounded-full"
            style={{ backgroundColor: "var(--text-muted)" }}
          />
        </div>
      </div>
    </div>
  );
}

export function EmptyState() {
  return (
    <div
      data-testid="chat-panel-empty"
      className="flex flex-col items-center justify-center h-full p-6 text-center"
    >
      <div
        className="w-12 h-12 rounded-lg flex items-center justify-center mb-3"
        style={{
          /* macOS Tahoe: subtle solid background, no gradient, no border */
          backgroundColor: "var(--bg-hover)",
        }}
      >
        <MessageSquare
          className="w-5 h-5"
          style={{ color: "var(--text-muted)" }}
        />
      </div>
      <p
        className="text-[0.8125rem] font-medium"
        style={{ color: "var(--text-primary)" }}
      >
        Start a conversation
      </p>
      <p
        className="text-xs mt-1"
        style={{ color: "var(--text-muted)" }}
      >
        Ask questions or get help with your tasks
      </p>
    </div>
  );
}

export function HistoryEmptyState() {
  return (
    <div
      data-testid="chat-panel-history-empty"
      className="flex flex-col items-center justify-center h-full p-6 text-center"
    >
      <div
        className="w-12 h-12 rounded-lg flex items-center justify-center mb-3"
        style={{
          backgroundColor: "var(--bg-hover)",
        }}
      >
        <MessageSquare
          className="w-5 h-5"
          style={{ color: "var(--text-muted)" }}
        />
      </div>
      <p
        className="text-[0.8125rem] font-medium"
        style={{ color: "var(--text-primary)" }}
      >
        No chat for this state
      </p>
      <p
        className="text-xs mt-1"
        style={{ color: "var(--text-muted)" }}
      >
        This historical state does not have a conversation attached.
      </p>
    </div>
  );
}

export function ConversationTranscriptPlaceholders({
  contentWidthClassName,
  className,
  testId = "chat-transcript-placeholders",
  ariaHidden = false,
}: {
  contentWidthClassName?: string | undefined;
  className?: string | undefined;
  testId?: string | undefined;
  ariaHidden?: boolean | undefined;
}) {
  const rows = [
    { side: "left", width: "72%", lines: ["86%", "64%"] },
    { side: "right", width: "58%", lines: ["100%"] },
    { side: "left", width: "78%", lines: ["92%", "76%", "48%"] },
    { side: "right", width: "46%", lines: ["100%"] },
  ] as const;

  return (
    <div
      className={cn("flex-1 overflow-hidden px-4 py-6", className)}
      data-testid={testId}
      aria-label={ariaHidden ? undefined : "Loading conversation"}
      aria-hidden={ariaHidden || undefined}
    >
      <div
        className={cn(
          "mx-auto flex w-full flex-col gap-5",
          contentWidthClassName,
        )}
      >
        {rows.map((row, index) => {
          const isUser = row.side === "right";
          return (
            <div
              key={`${row.side}-${index}`}
              className={cn("flex w-full", isUser ? "justify-end" : "justify-start")}
            >
              <div
                className={cn(
                  "flex max-w-[82%] gap-2",
                  isUser ? "flex-row-reverse" : "flex-row",
                )}
                style={{ width: row.width }}
              >
                {!isUser && (
                  <div
                    className="mt-1 h-7 w-7 shrink-0 rounded-md"
                    style={{
                      background: withAlpha("var(--accent-primary)", 12),
                      border: "1px solid var(--overlay-faint)",
                    }}
                  />
                )}
                <div
                  className="min-w-0 flex-1 rounded-lg px-3 py-2.5"
                  style={{
                    background: isUser
                      ? withAlpha("var(--accent-primary)", 14)
                      : "var(--bg-elevated)",
                    border: "1px solid var(--overlay-faint)",
                  }}
                >
                  <div className="flex flex-col gap-2">
                    {row.lines.map((width, lineIndex) => (
                      <div
                        key={`${width}-${lineIndex}`}
                        className="h-2.5 rounded-full"
                        style={{
                          width,
                          background: isUser
                            ? withAlpha("var(--accent-primary)", 24)
                            : "var(--overlay-weak)",
                        }}
                      />
                    ))}
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// LoadingState is now extracted to MessageListSkeleton.tsx
// Re-export for backwards compatibility
export { MessageListSkeleton as LoadingState } from "./MessageListSkeleton";

interface FailedRunBannerProps {
  errorMessage: string;
  onDismiss?: () => void;
}

export function FailedRunBanner({ errorMessage, onDismiss }: FailedRunBannerProps) {
  return (
    <div
      data-testid="failed-run-banner"
      className="flex items-start gap-2 px-3 py-2 mb-2 rounded-lg"
      style={{
        /* macOS Tahoe: subtle solid background with error tint, no gradient, no border */
        backgroundColor: "var(--status-error-muted)",
      }}
    >
      <Activity
        className="w-3.5 h-3.5 mt-0.5 shrink-0"
        style={{ color: "var(--status-error)" }}
      />
      <div className="flex-1 min-w-0">
        <span
          className="text-[0.8125rem] font-medium block"
          style={{ color: "var(--status-error)" }}
        >
          Agent run failed
        </span>
        <span
          className="text-[0.75rem] block mt-0.5 break-words"
          style={{ color: "var(--status-error)" }}
        >
          {errorMessage.slice(0, 200)}
          {errorMessage.length > 200 && "..."}
        </span>
      </div>
      {onDismiss && (
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onDismiss}
          className="shrink-0"
          style={{ color: "var(--status-error)" }}
          aria-label="Dismiss error"
        >
          <X className="w-3.5 h-3.5" />
        </Button>
      )}
    </div>
  );
}

interface PreviousRunBannerProps {
  agentRunStatus: string | null;
  contextType: string;
}

export function PreviousRunBanner({ agentRunStatus, contextType }: PreviousRunBannerProps) {
  const contextLabel = contextType === "merge" ? "merge agent"
    : contextType === "review" ? "reviewer"
    : contextType === "branch_update" ? "branch updater"
    : "worker";

  const statusLabel = agentRunStatus === "failed" ? "failed"
    : agentRunStatus === "cancelled" ? "cancelled"
    : agentRunStatus === "running" ? "in progress"
    : "completed";

  return (
    <div
      data-testid="previous-run-banner"
      className="px-3 py-1.5 flex items-center gap-2 shrink-0"
      style={{
        backgroundColor: withAlpha("var(--bg-elevated)", 80),
        borderBottom: "1px solid var(--overlay-weak)",
      }}
    >
      <History className="w-3 h-3" style={{ color: "var(--text-muted)" }} />
      <span className="text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
        Previous {contextLabel} run ({statusLabel})
      </span>
    </div>
  );
}

interface ContextIndicatorProps {
  context: ChatContext;
  isExecutionMode?: boolean;
  isReviewMode?: boolean;
  isMergeMode?: boolean;
}

export function ContextIndicator({
  context,
  isExecutionMode = false,
  isReviewMode = false,
  isMergeMode = false,
}: ContextIndicatorProps) {
  const getContextInfo = () => {
    if (isExecutionMode) {
      return { icon: Hammer, label: "Worker" };
    }
    if (isReviewMode) {
      return { icon: Bot, label: "AI Review" };
    }
    if (isMergeMode) {
      return { icon: GitMerge, label: "Merger" };
    }

    switch (context.view) {
      case "ideation":
        return { icon: MessageSquare, label: "Chat" };
      case "kanban":
        return context.selectedTaskId
          ? { icon: CheckSquare, label: "Task" }
          : { icon: FolderKanban, label: "Project" };
      case "task_detail":
        return { icon: CheckSquare, label: "Task" };
      case "activity":
        return { icon: MessageSquare, label: "Activity" };
      default:
        return { icon: MessageSquare, label: "Chat" };
    }
  };

  const { icon: Icon, label } = getContextInfo();

  return (
    <div className="flex items-center gap-2 min-w-0 flex-1">
      <Icon
        className="w-3.5 h-3.5 shrink-0"
        style={{ color: "var(--text-muted)" }}
      />
      <span
        className="text-[0.8125rem] font-medium truncate"
        style={{ color: "var(--text-primary)" }}
      >
        {label}
      </span>
    </div>
  );
}
