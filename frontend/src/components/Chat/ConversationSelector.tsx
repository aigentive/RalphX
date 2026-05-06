/**
 * ConversationSelector - Dropdown for switching between chat conversations
 *
 * Features:
 * - Lists all conversations for the current context (ideation/task/project)
 * - Shows conversation title, date, and message count
 * - Active conversation indicator (filled dot)
 * - "New Conversation" option at top
 * - Click to switch conversations
 */

import { useMemo } from "react";
import { formatDistanceToNow, format } from "date-fns";
import { History, Plus, Circle, CheckCircle2, XCircle, AlertCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { ChatConversation, ContextType, AgentRun, AgentRunStatus } from "@/types/chat-conversation";
import { useQueries, type Query } from "@tanstack/react-query";
import { chatApi } from "@/api/chat";
import { withAlpha } from "@/lib/theme-colors";
import {
  describeProviderLineage,
  formatProviderHarnessLabel,
  formatProviderSessionSnippet,
  getProviderHarnessBadgeStyle,
} from "./provider-harness";

// ============================================================================
// Types
// ============================================================================

export interface ConversationSelectorProps {
  /** Current context type */
  contextType: ContextType;
  /** Current context ID */
  contextId: string;
  /** All conversations for this context */
  conversations: ChatConversation[];
  /** ID of the active conversation */
  activeConversationId: string | null;
  /** Callback when a conversation is selected */
  onSelectConversation: (conversationId: string) => void;
  /** Callback when "New Conversation" is clicked */
  onNewConversation: () => void;
  /** Optional: show loading state */
  isLoading?: boolean;
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Format conversation date as relative time (e.g., "2 hours ago")
 * Falls back to "No messages" if no last message
 */
function formatConversationDate(lastMessageAt: string | null): string {
  if (!lastMessageAt) {
    return "No messages";
  }

  try {
    return formatDistanceToNow(new Date(lastMessageAt), { addSuffix: true });
  } catch {
    return "Unknown";
  }
}

/**
 * Generate a conversation title from first message or fallback
 */
function getConversationTitle(conversation: ChatConversation, index?: number): string {
  if (conversation.title) {
    return conversation.title;
  }

  // For task_execution context, show "Execution #N"
  if (conversation.contextType === "task_execution" && index !== undefined) {
    return `Execution #${index + 1}`;
  }

  // For review context, show "Review #N"
  if (conversation.contextType === "review" && index !== undefined) {
    return `Review #${index + 1}`;
  }

  // For merge context, show "Merge #N"
  if (conversation.contextType === "merge" && index !== undefined) {
    return `Merge #${index + 1}`;
  }

  // Fallback title
  if (conversation.messageCount === 0) {
    return "New conversation";
  }

  return `Conversation ${conversation.id.slice(0, 8)}`;
}

/**
 * Get status icon for execution conversations
 */
function getStatusIcon(status: AgentRunStatus | null) {
  switch (status) {
    case "running":
      return <Circle className="h-3 w-3 animate-pulse" style={{ color: "var(--accent-primary)", fill: "var(--accent-primary)" }} />;
    case "completed":
      return <CheckCircle2 className="h-3 w-3" style={{ color: "var(--status-success)" }} />;
    case "failed":
      return <XCircle className="h-3 w-3" style={{ color: "var(--status-error)" }} />;
    case "cancelled":
      return <AlertCircle className="h-3 w-3" style={{ color: "var(--status-warning)" }} />;
    default:
      return null;
  }
}

/**
 * Format execution date for display
 */
function formatExecutionDate(createdAt: string): string {
  try {
    return format(new Date(createdAt), "MMM d, h:mm a");
  } catch {
    return "Unknown";
  }
}

// ============================================================================
// Component
// ============================================================================

export function ConversationSelector({
  contextType,
  conversations,
  activeConversationId,
  onSelectConversation,
  onNewConversation,
  isLoading = false,
}: ConversationSelectorProps) {
  // Execution, review, and merge contexts share similar behavior (agent runs, status polling)
  const isAgentContext = contextType === "task_execution" || contextType === "review" || contextType === "merge";
  const isExecutionContext = contextType === "task_execution";

  // Sort conversations by creation date for agent contexts, last message date otherwise
  const sortedConversations = useMemo(() => {
    return [...conversations].sort((a, b) => {
      if (isAgentContext) {
        // For agent contexts (execution/review), sort by creation date DESC (most recent first)
        const aDate = new Date(a.createdAt).getTime();
        const bDate = new Date(b.createdAt).getTime();
        return bDate - aDate;
      } else {
        // For regular conversations, sort by last message date
        const aDate = a.lastMessageAt ? new Date(a.lastMessageAt).getTime() : 0;
        const bDate = b.lastMessageAt ? new Date(b.lastMessageAt).getTime() : 0;
        return bDate - aDate;
      }
    });
  }, [conversations, isAgentContext]);

  // Fetch agent run status for active agent context conversation only
  const statusQueries = useQueries({
    queries: sortedConversations.map((conv) => ({
      queryKey: ["agent-run", conv.id] as const,
      queryFn: () => chatApi.getAgentRunStatus(conv.id),
      enabled: isAgentContext && conv.id === activeConversationId,
      refetchInterval: (query: Query<AgentRun | null, Error>) =>
        query.state.data?.status === "running" ? 2000 : false,
      refetchIntervalInBackground: false,
    })),
  });

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8 text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition-colors"
          aria-label="Conversation history"
          data-testid="conversation-selector-trigger"
        >
          <History className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>

      <DropdownMenuContent
        align="end"
        className="w-[320px] rounded-xl overflow-y-auto"
        style={{
          /* macOS Tahoe: flat dark background, subtle border */
          backgroundColor: "var(--bg-elevated)",
          border: "1px solid var(--overlay-weak)",
          boxShadow: "var(--shadow-lg)",
          maxHeight: "400px",
        }}
        data-testid="conversation-selector-menu"
      >
        <DropdownMenuLabel
          className="text-[0.6875rem] font-medium tracking-wide uppercase px-3 py-2"
          style={{ color: "var(--text-muted)" }}
        >
          {isExecutionContext ? "Execution History" : contextType === "review" ? "Review History" : contextType === "merge" ? "Merge History" : "Conversation History"}
        </DropdownMenuLabel>

        {/* New Conversation Option - Only for non-agent contexts (not execution or review) */}
        {!isAgentContext && (
          <>
            <DropdownMenuItem
              onClick={onNewConversation}
              className="flex items-center gap-3 px-3 py-2.5 cursor-pointer transition-colors"
              style={{ color: "var(--text-primary)" }}
              data-testid="conversation-selector-new"
            >
              <Plus className="h-4 w-4" style={{ color: "var(--accent-primary)" }} />
              <span className="text-[0.8125rem] font-medium">New Conversation</span>
            </DropdownMenuItem>

            {/* Separator */}
            {sortedConversations.length > 0 && (
              <DropdownMenuSeparator style={{ backgroundColor: "var(--overlay-weak)", margin: "4px 0" }} />
            )}
          </>
        )}

        {/* Loading State */}
        {isLoading && (
          <div className="px-3 py-6 text-center text-[0.8125rem]" style={{ color: "var(--text-muted)" }}>
            Loading conversations...
          </div>
        )}

        {/* Empty State */}
        {!isLoading && sortedConversations.length === 0 && (
          <div className="px-3 py-6 text-center text-[0.8125rem]" style={{ color: "var(--text-muted)" }}>
            No conversations yet
          </div>
        )}

        {/* Conversation List */}
        {!isLoading &&
          sortedConversations.map((conversation, index) => {
            const isActive = conversation.id === activeConversationId;
            const title = getConversationTitle(conversation, index);
            const harnessLabel = formatProviderHarnessLabel(
              conversation.providerHarness,
            );
            const harnessBadgeStyle = getProviderHarnessBadgeStyle(
              conversation.providerHarness,
            );
            const providerLineage = describeProviderLineage(conversation, "selector");
            const providerSessionSnippet = formatProviderSessionSnippet(
              conversation.providerSessionId,
            );

            // Get agent run status for agent context conversations (execution/review)
            const agentRunStatus = isAgentContext && statusQueries[index]
              ? statusQueries[index].data?.status ?? null
              : null;

            if (isAgentContext) {
              // Agent context rendering (execution and review)
              const executionDate = formatExecutionDate(conversation.createdAt);
              const statusIcon = getStatusIcon(agentRunStatus);

              return (
                <DropdownMenuItem
                  key={conversation.id}
                  onClick={() => onSelectConversation(conversation.id)}
                  className="flex items-start gap-3 px-3 py-2.5 cursor-pointer transition-colors rounded-lg mx-1"
                  style={{
                    backgroundColor: isActive ? withAlpha("var(--accent-primary)", 15) : "transparent",
                  }}
                  data-testid={`conversation-item-${conversation.id}`}
                  data-active={isActive}
                >
                  {/* Status Icon */}
                  <div className="mt-1.5 flex-shrink-0">
                    {statusIcon || (
                      <Circle
                        className="h-2 w-2"
                        style={{
                          color: isActive ? "var(--accent-primary)" : "transparent",
                          fill: isActive ? "var(--accent-primary)" : "transparent",
                        }}
                      />
                    )}
                  </div>

                  {/* Content */}
                  <div className="flex-1 min-w-0">
                    {/* Title with status */}
                    <div className="flex items-center gap-2">
                      <div
                        className="text-[0.8125rem] font-medium truncate"
                        style={{ color: isActive ? "var(--text-primary)" : "var(--text-secondary)" }}
                      >
                        {title}
                      </div>
                      {harnessLabel && (
                        <span
                          className="rounded-full px-1.5 py-0.5 text-[0.5625rem] font-semibold uppercase tracking-[0.08em]"
                          style={harnessBadgeStyle}
                        >
                          {harnessLabel}
                        </span>
                      )}
                      {agentRunStatus === "running" && (
                        <span
                          className="text-[0.625rem] font-medium uppercase tracking-wide"
                          style={{ color: "var(--accent-primary)" }}
                        >
                          Running
                        </span>
                      )}
                    </div>

                    {/* Date and status */}
                    <div
                      className="flex items-center gap-2 mt-0.5 text-[0.6875rem]"
                      style={{ color: "var(--text-muted)" }}
                    >
                      <span>{executionDate}</span>
                      {agentRunStatus && agentRunStatus !== "running" && (
                        <>
                          <span>•</span>
                          <span style={{
                            color: agentRunStatus === "completed" ? "var(--status-success)"
                              : agentRunStatus === "failed" ? "var(--status-error)"
                              : agentRunStatus === "cancelled" ? "var(--status-warning)"
                              : undefined
                          }}>
                            {agentRunStatus.charAt(0).toUpperCase() + agentRunStatus.slice(1)}
                          </span>
                        </>
                      )}
                    </div>
                    <div
                      className="mt-1 flex items-center gap-2 text-[0.625rem]"
                      style={{ color: "var(--text-muted)" }}
                    >
                      <span>{providerLineage}</span>
                      {providerSessionSnippet && (
                        <>
                          <span>•</span>
                          <span className="font-mono">{providerSessionSnippet}</span>
                        </>
                      )}
                    </div>
                  </div>
                </DropdownMenuItem>
              );
            } else {
              // Regular conversation rendering
              const date = formatConversationDate(conversation.lastMessageAt);

              return (
                <DropdownMenuItem
                  key={conversation.id}
                  onClick={() => onSelectConversation(conversation.id)}
                  className="flex items-start gap-3 px-3 py-2.5 cursor-pointer transition-colors rounded-lg mx-1"
                  style={{
                    backgroundColor: isActive ? withAlpha("var(--accent-primary)", 15) : "transparent",
                  }}
                  data-testid={`conversation-item-${conversation.id}`}
                  data-active={isActive}
                >
                  {/* Active Indicator */}
                  <Circle
                    className="h-2 w-2 mt-1.5 flex-shrink-0"
                    style={{
                      color: isActive ? "var(--accent-primary)" : "transparent",
                      fill: isActive ? "var(--accent-primary)" : "transparent",
                    }}
                  />

                  {/* Content */}
                  <div className="flex-1 min-w-0">
                    {/* Title */}
                    <div
                      className="text-[0.8125rem] font-medium truncate"
                      style={{ color: isActive ? "var(--text-primary)" : "var(--text-secondary)" }}
                    >
                      <span>{title}</span>
                      {harnessLabel && (
                        <span
                          className="ml-2 rounded-full px-1.5 py-0.5 text-[0.5625rem] font-semibold uppercase tracking-[0.08em]"
                          style={harnessBadgeStyle}
                        >
                          {harnessLabel}
                        </span>
                      )}
                    </div>

                    {/* Date and Message Count */}
                    <div
                      className="flex items-center gap-2 mt-0.5 text-[0.6875rem]"
                      style={{ color: "var(--text-muted)" }}
                    >
                      <span>{date}</span>
                      <span>•</span>
                      <span>
                        {conversation.messageCount}{" "}
                        {conversation.messageCount === 1 ? "message" : "messages"}
                      </span>
                    </div>
                    <div
                      className="mt-1 flex items-center gap-2 text-[0.625rem]"
                      style={{ color: "var(--text-muted)" }}
                    >
                      <span>{providerLineage}</span>
                      {providerSessionSnippet && (
                        <>
                          <span>•</span>
                          <span className="font-mono">{providerSessionSnippet}</span>
                        </>
                      )}
                    </div>
                  </div>
                </DropdownMenuItem>
              );
            }
          })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
