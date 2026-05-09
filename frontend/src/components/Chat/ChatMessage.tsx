/**
 * ChatMessage - Individual chat message display component
 *
 * Design spec: specs/design/pages/chat-panel.md
 * - Refined Studio aesthetic with gradient bubbles
 * - Asymmetric corners (10px/10px/10px/4px)
 * - User: warm orange gradient, right-aligned
 * - Agent: layered dark gradient with subtle border
 * - Compact sizing for application UI
 */

import { useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ChatMessage as ChatMessageType, MessageRole } from "@/types/ideation";
import { AGENT_ORCHESTRATOR } from "@/constants/agents";
import { withAlpha } from "@/lib/theme-colors";
import { ToolCallIndicator, type ToolCall } from "./ToolCallIndicator";
import { shouldHideCompletedProjectOrchestrationToolCall } from "./tool-widgets/ProjectOrchestrationWidget.utils";
import { MarkdownLink } from "./MessageItem.markdown";
import { formatHumanTimestamp } from "@/lib/formatters";

// ============================================================================
// Types
// ============================================================================

interface ChatMessageProps {
  /** The message to display */
  message: ChatMessageType;
  /** @deprecated Chat timestamps now use the shared human timestamp format. */
  showFullTimestamp?: boolean;
  /** Compact mode with reduced spacing and no role indicator */
  compact?: boolean;
}

/**
 * Content block item - represents either text or a tool use
 */
interface ContentBlockItem {
  type: "text" | "tool_use";
  // For text type
  text?: string;
  // For tool_use type
  id?: string;
  name?: string;
  arguments?: unknown;
  result?: unknown;
}

// ============================================================================
// Helpers
// ============================================================================

function getRoleLabel(role: MessageRole): string {
  switch (role) {
    case "user":
      return "You";
    case AGENT_ORCHESTRATOR:
      return "Orchestrator";
    case "system":
      return "System";
    default:
      return "Unknown";
  }
}

function getAccessibleName(role: MessageRole): string {
  const roleLabel = getRoleLabel(role);
  return `Message from ${roleLabel}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === "object" && !Array.isArray(value);
}

function normalizeToolCall(raw: unknown, index: number): ToolCall | null {
  if (!isRecord(raw)) return null;

  const name = typeof raw.name === "string" ? raw.name : "";
  if (!name) return null;

  const toolCall: ToolCall = {
    id: typeof raw.id === "string" && raw.id ? raw.id : `tool-${index}`,
    name,
    // Older persisted rows used `input`; current widgets read `arguments`.
    arguments: raw.arguments ?? raw.input ?? {},
  };

  if ("result" in raw) {
    toolCall.result = raw.result;
  }
  if (typeof raw.error === "string") {
    toolCall.error = raw.error;
  }
  if (typeof raw.parentToolUseId === "string") {
    toolCall.parentToolUseId = raw.parentToolUseId;
  }
  if (isRecord(raw.diffContext) && typeof raw.diffContext.filePath === "string") {
    toolCall.diffContext = raw.diffContext as NonNullable<ToolCall["diffContext"]>;
  }
  if (isRecord(raw.stats)) {
    toolCall.stats = raw.stats as NonNullable<ToolCall["stats"]>;
  }

  return toolCall;
}

// ============================================================================
// Markdown Components
// ============================================================================

const markdownComponents = {
  // Style links
  a: MarkdownLink,
  // Style inline code
  code: ({ className, children, ...props }: React.HTMLAttributes<HTMLElement>) => {
    const isBlock = className?.includes("language-");
    if (isBlock) {
      return (
        <code
          className={`block p-3 rounded text-sm overflow-x-auto ${className || ""}`}
          style={{ backgroundColor: "var(--bg-elevated)" }}
          {...props}
        >
          {children}
        </code>
      );
    }
    return (
      <code
        className="px-1 py-0.5 rounded text-sm"
        style={{ backgroundColor: "var(--bg-elevated)" }}
        {...props}
      >
        {children}
      </code>
    );
  },
  // Style code blocks container
  pre: ({ children, ...props }: React.HTMLAttributes<HTMLPreElement>) => (
    <pre
      className="my-2 rounded overflow-hidden"
      style={{ backgroundColor: "var(--bg-elevated)" }}
      {...props}
    >
      {children}
    </pre>
  ),
  // Style paragraphs
  p: ({ children, ...props }: React.HTMLAttributes<HTMLParagraphElement>) => (
    <p className="mb-2 last:mb-0" {...props}>
      {children}
    </p>
  ),
  // Style lists
  ul: ({ children, ...props }: React.HTMLAttributes<HTMLUListElement>) => (
    <ul className="list-disc list-inside mb-2" {...props}>
      {children}
    </ul>
  ),
  ol: ({ children, ...props }: React.HTMLAttributes<HTMLOListElement>) => (
    <ol className="list-decimal list-inside mb-2" {...props}>
      {children}
    </ol>
  ),
  // Style list items
  li: ({ children, ...props }: React.LiHTMLAttributes<HTMLLIElement>) => (
    <li className="mb-1" {...props}>
      {children}
    </li>
  ),
  // Style strong/bold
  strong: ({ children, ...props }: React.HTMLAttributes<HTMLElement>) => (
    <strong className="font-semibold" {...props}>
      {children}
    </strong>
  ),
  // Style emphasis/italic
  em: ({ children, ...props }: React.HTMLAttributes<HTMLElement>) => (
    <em className="italic" {...props}>
      {children}
    </em>
  ),
};

// ============================================================================
// Component
// ============================================================================

/**
 * TextBubble - Individual text content bubble
 */
function TextBubble({
  content,
  isUser,
  isFirst,
  isLast,
}: {
  content: string;
  isUser: boolean;
  isFirst: boolean;
  isLast: boolean;
}) {
  // Refined Studio bubble styles with gradients
  const bubbleStyle: React.CSSProperties = {
    background: isUser
      ? "linear-gradient(135deg, var(--accent-primary) 0%, var(--accent-hover) 100%)"
      : `linear-gradient(180deg, ${withAlpha("var(--bg-elevated)", 95)} 0%, ${withAlpha("var(--bg-surface)", 98)} 100%)`,
    color: isUser ? "var(--text-primary)" : "var(--text-primary)",
    border: isUser ? "none" : "1px solid var(--overlay-weak)",
    boxShadow: isUser
      ? `0 2px 8px ${withAlpha("var(--accent-primary)", 20)}`
      : `var(--shadow-xs)`,
  };

  // Corner radius varies based on position in sequence
  const getCornerRadius = () => {
    if (isUser) {
      // User bubbles: right-aligned
      if (isFirst && isLast) return "10px 10px 4px 10px";
      if (isFirst) return "10px 10px 4px 10px";
      if (isLast) return "10px 10px 4px 10px";
      return "10px 10px 4px 10px";
    } else {
      // Agent bubbles: left-aligned
      if (isFirst && isLast) return "10px 10px 10px 4px";
      if (isFirst) return "10px 10px 10px 4px";
      if (isLast) return "10px 10px 10px 4px";
      return "10px 10px 10px 4px";
    }
  };

  return (
    <div
      className="max-w-[85%] px-3 py-2 break-words"
      style={{
        ...bubbleStyle,
        borderRadius: getCornerRadius(),
      }}
    >
      <div className="text-[0.8125rem] leading-relaxed">
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
          {content}
        </ReactMarkdown>
      </div>
    </div>
  );
}

export function ChatMessage({
  message,
  compact = false,
}: ChatMessageProps) {
  const isUser = message.role === "user";
  const alignmentClass = isUser ? "items-end" : "items-start";
  const spacingClass = compact ? "mb-1" : "mb-3";

  const timestamp = useMemo(
    () => formatHumanTimestamp(message.createdAt),
    [message.createdAt]
  );

  const accessibleName = useMemo(
    () => getAccessibleName(message.role),
    [message.role]
  );

  // Parse content blocks if present (interleaved text and tool calls)
  const contentBlocks = useMemo<ContentBlockItem[]>(() => {
    if (!message.contentBlocks) return [];
    try {
      const parsed = JSON.parse(message.contentBlocks);
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  }, [message.contentBlocks]);

  // Parse tool calls if present (fallback for messages without contentBlocks)
  const toolCalls = useMemo<ToolCall[]>(() => {
    if (!message.toolCalls) return [];
    try {
      const parsed = JSON.parse(message.toolCalls);
      return Array.isArray(parsed)
        ? parsed
            .map((toolCall, index) => normalizeToolCall(toolCall, index))
            .filter((toolCall): toolCall is ToolCall => toolCall !== null)
        : [];
    } catch {
      return [];
    }
  }, [message.toolCalls]);
  const visibleToolCalls = useMemo(
    () => toolCalls.filter((toolCall) => !shouldHideCompletedProjectOrchestrationToolCall(toolCall)),
    [toolCalls],
  );

  // Use content blocks if available, otherwise fall back to legacy rendering
  const hasContentBlocks = contentBlocks.length > 0;

  return (
    <article
      data-testid={`chat-message-${message.id}`}
      className={`flex flex-col ${alignmentClass} ${spacingClass}`}
      aria-label={accessibleName}
    >
      {/* Role indicator (hidden in compact mode) */}
      {!compact && (
        <span
          data-testid="chat-message-role"
          className="text-xs font-medium mb-1 px-1"
          style={{ color: "var(--text-muted)" }}
        >
          {getRoleLabel(message.role)}
        </span>
      )}

      {/* Render content blocks in order (preserves interleaved text/tool_use sequence) */}
      {hasContentBlocks ? (
        <div className="flex flex-col gap-1.5 max-w-full">
          {contentBlocks.map((block, index) => {
            if (block.type === "text" && block.text) {
              return (
                <TextBubble
                  key={`block-${index}`}
                  content={block.text}
                  isUser={isUser}
                  isFirst={index === 0}
                  isLast={index === contentBlocks.length - 1}
                />
              );
            } else if (block.type === "tool_use" && block.name) {
              const toolCall: ToolCall = {
                id: block.id || `tool-${index}`,
                name: block.name,
                arguments: block.arguments,
                result: block.result,
              };
              if (shouldHideCompletedProjectOrchestrationToolCall(toolCall)) {
                return null;
              }
              return (
                <div key={`block-${index}`} className="max-w-[85%]">
                  <ToolCallIndicator toolCall={toolCall} />
                </div>
              );
            }
            return null;
          })}
        </div>
      ) : (
        <>
          {/* Legacy rendering: single bubble with content + tool calls */}
          <TextBubble
            content={message.content}
            isUser={isUser}
            isFirst={true}
            isLast={true}
          />

          {/* Tool calls (if any) */}
          {visibleToolCalls.length > 0 && (
            <div className="mt-1.5 max-w-[85%] space-y-1.5" data-testid="chat-message-tool-calls">
              {visibleToolCalls.map((toolCall) => (
                <ToolCallIndicator key={toolCall.id} toolCall={toolCall} />
              ))}
            </div>
          )}
        </>
      )}

      {/* Timestamp */}
      <time
        data-testid="chat-message-timestamp"
        dateTime={message.createdAt}
        title={timestamp.title || undefined}
        className="text-[10px] mt-1 px-1 text-text-primary/40"
        role="time"
      >
        {timestamp.label}
      </time>
    </article>
  );
}
