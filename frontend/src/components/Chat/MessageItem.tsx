/**
 * MessageItem - Shared chat message component
 *
 * Renders a single chat message with support for:
 * - Interleaved text and tool calls (content blocks)
 * - Legacy rendering fallback (tool calls first, then text)
 * - User vs assistant styling
 * - Markdown rendering for assistant messages
 * - Code blocks with copy functionality
 */

import React, { useCallback, useMemo, useState } from "react";
import { Bot, Check, Copy } from "lucide-react";
import { cn } from "@/lib/utils";
import { ToolCallIndicator, type ToolCall } from "./ToolCallIndicator";
import { shouldHideCompletedProjectOrchestrationToolCall } from "./tool-widgets/ProjectOrchestrationWidget.utils";
import { TextBubble } from "./TextBubble";
import { formatTimestamp, formatTimestampTitle } from "./MessageItem.utils";
import { isTaskToolCall } from "./DiffToolCallView.utils";
import { MessageAttachments, type MessageAttachment } from "./MessageAttachments";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  formatMessageAttributionTooltip,
  formatProviderModelEffortLabel,
  formatProviderHarnessLabel,
  getProviderHarnessBadgeStyle,
} from "./provider-harness";
import {
  normalizeToolCallTranscriptPayload,
} from "./verification-tool-calls";

// ============================================================================
// Types
// ============================================================================

/**
 * Content block item - represents either text or a tool use
 */
export interface ContentBlockItem {
  type: "text" | "tool_use";
  text?: string;
  id?: string;
  name?: string;
  arguments?: unknown;
  result?: unknown;
  resultPreviewTruncated?: boolean;
  resultPreviewOriginalBytes?: number;
  resultPreviewLineCount?: number;
  resultPreviewOmittedLines?: number;
  detailRef?: ToolCall["detailRef"];
  parentToolUseId?: string;
  /** Diff context for Edit/Write tool calls (old file content for computing diffs) */
  diffContext?: {
    oldContent?: string;
    filePath: string;
  };
}

export interface MessageItemProps {
  role: string;
  content: string;
  createdAt: string;
  /** Optional pre-rendered message body for live content that is not yet persisted as content blocks. */
  children?: React.ReactNode | undefined;
  isLastInList?: boolean | undefined;
  /** Pre-parsed tool calls array (parsed at API layer) */
  toolCalls?: ToolCall[] | null;
  /** Pre-parsed content blocks array (parsed at API layer) */
  contentBlocks?: ContentBlockItem[] | null;
  /** File attachments for user messages */
  attachments?: MessageAttachment[];
  /** Teammate name for team mode messages */
  teammateName?: string | null | undefined;
  /** Teammate color for left-border indicator */
  teammateColor?: string | null | undefined;
  providerHarness?: string | null | undefined;
  providerSessionId?: string | null | undefined;
  upstreamProvider?: string | null | undefined;
  providerProfile?: string | null | undefined;
  logicalModel?: string | null | undefined;
  effectiveModelId?: string | null | undefined;
  logicalEffort?: string | null | undefined;
  effectiveEffort?: string | null | undefined;
  inputTokens?: number | null | undefined;
  outputTokens?: number | null | undefined;
  cacheCreationTokens?: number | null | undefined;
  cacheReadTokens?: number | null | undefined;
  estimatedUsd?: number | null | undefined;
  showAssistantIcon?: boolean | undefined;
  hideMeta?: boolean | undefined;
}

export interface MessageMetaProps {
  createdAt: string;
  copyableText?: string | undefined;
  isUser?: boolean | undefined;
}

export function MessageMeta({
  createdAt,
  copyableText = "",
  isUser = false,
}: MessageMetaProps) {
  const [copied, setCopied] = useState(false);
  const showInlineCopy = copyableText.trim().length > 0;
  const handleCopy = useCallback(async () => {
    if (!showInlineCopy) {
      return;
    }

    try {
      await navigator.clipboard.writeText(copyableText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Silently fail
    }
  }, [copyableText, showInlineCopy]);

  return (
    <div
      className={cn(
        "flex items-center gap-1.5 px-1 pb-[10px] text-[0.625rem] text-text-primary/40",
        isUser ? "justify-end" : "justify-start"
      )}
      data-testid="message-meta"
    >
      <span title={formatTimestampTitle(createdAt) || undefined}>
        {formatTimestamp(createdAt)}
      </span>
      {showInlineCopy && (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              onClick={() => void handleCopy()}
              className="h-4 w-4 rounded-sm p-0 text-text-primary/40 hover:bg-[var(--overlay-moderate)] hover:text-text-primary/70"
              aria-label={copied ? "Copied" : "Copy message"}
              data-testid="message-copy-button"
              data-theme-button-skip="true"
            >
              {copied ? (
                <Check className="h-3 w-3 text-[var(--status-success)]" />
              ) : (
                <Copy className="h-3 w-3" />
              )}
            </Button>
          </TooltipTrigger>
          <TooltipContent side="top" className="text-xs">
            {copied ? "Copied" : "Copy message"}
          </TooltipContent>
        </Tooltip>
      )}
    </div>
  );
}

// ============================================================================
// Message Component
// ============================================================================

export const MessageItem = React.memo(function MessageItem({
  role,
  content,
  createdAt,
  children,
  isLastInList = false,
  toolCalls,
  contentBlocks,
  attachments,
  teammateName,
  teammateColor,
  providerHarness,
  providerSessionId,
  upstreamProvider,
  providerProfile,
  logicalModel,
  effectiveModelId,
  logicalEffort,
  effectiveEffort,
  inputTokens,
  outputTokens,
  cacheCreationTokens,
  cacheReadTokens,
  estimatedUsd,
  showAssistantIcon = true,
  hideMeta = false,
}: MessageItemProps) {
  const isUser = role === "user";
  const [copied, setCopied] = useState(false);
  const providerHarnessLabel = formatProviderHarnessLabel(providerHarness);
  const providerHarnessStyle = getProviderHarnessBadgeStyle(providerHarness);
  const modelEffortLabel = formatProviderModelEffortLabel({
    logicalModel,
    effectiveModelId,
    logicalEffort,
    effectiveEffort,
  });
  const providerTooltip = formatMessageAttributionTooltip({
    providerHarness,
    providerSessionId,
    upstreamProvider,
    providerProfile,
    logicalModel,
    effectiveModelId,
    logicalEffort,
    effectiveEffort,
    inputTokens,
    outputTokens,
    cacheCreationTokens,
    cacheReadTokens,
    estimatedUsd,
  });
  const showProviderMeta =
    !isUser &&
    !teammateName &&
    (providerHarnessLabel !== null || modelEffortLabel !== null);

  // Use pre-parsed data directly (parsing now happens at API layer)
  const { contentBlocks: parsedContentBlocks, toolCalls: parsedToolCalls } = useMemo(
    () => normalizeToolCallTranscriptPayload({
      contentBlocks,
      toolCalls,
    }),
    [contentBlocks, toolCalls],
  );
  const visibleParsedToolCalls = useMemo(
    () => parsedToolCalls.filter((tc) => !shouldHideCompletedProjectOrchestrationToolCall(tc)),
    [parsedToolCalls],
  );
  const hasContentBlocks = parsedContentBlocks.length > 0;
  const copyableText = useMemo(() => {
    if (hasContentBlocks) {
      return parsedContentBlocks
        .filter((block) => block.type === "text" && typeof block.text === "string")
        .map((block) => block.text?.trim() ?? "")
        .filter((text) => text.length > 0)
        .join("\n\n");
    }

    return content.trim();
  }, [content, hasContentBlocks, parsedContentBlocks]);
  const showInlineCopy = copyableText.length > 0;
  const handleCopy = useCallback(async () => {
    if (!showInlineCopy) {
      return;
    }

    try {
      await navigator.clipboard.writeText(copyableText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Silently fail
    }
  }, [copyableText, showInlineCopy]);

  // Collect IDs of child tool calls that belong to Task subagents.
  // These are embedded in Task result content blocks and should NOT render as top-level cards.
  const childToolCallIds = useMemo(() => {
    const blocks = parsedContentBlocks;
    if (blocks.length === 0) return new Set<string>();
    const ids = new Set<string>();
    for (const block of blocks) {
      if (block.type === "tool_use" && block.name && isTaskToolCall(block.name) && block.result) {
        // Task result may be an array of content blocks containing child tool_use/tool_result
        const result = block.result;
        if (Array.isArray(result)) {
          for (const child of result) {
            if (child && typeof child === "object") {
              const c = child as Record<string, unknown>;
              if (c.type === "tool_use" && typeof c.id === "string") {
                ids.add(c.id);
              } else if (c.type === "tool_result" && typeof c.tool_use_id === "string") {
                ids.add(c.tool_use_id);
              }
            }
          }
        }
      }
    }
    return ids;
  }, [parsedContentBlocks]);

  return (
    <div
      className={cn(
        "flex min-w-0",
        isLastInList ? "mb-0" : "mb-5",
        isUser ? "justify-end" : "justify-start"
      )}
      data-chat-message-item="true"
      style={teammateColor ? { borderLeft: `2px solid ${teammateColor}`, paddingLeft: "8px" } : undefined}
    >
      {/* Agent indicator for assistant messages */}
      {!isUser && !teammateName && showAssistantIcon && (
        <Bot className={cn("w-3.5 h-3.5 mr-2 shrink-0 text-text-primary/40", showProviderMeta ? "mt-0.5" : "mt-2")} />
      )}
      {/* Teammate name badge */}
      {!isUser && teammateName && (
        <div className="flex items-center gap-1 mt-2 mr-2 shrink-0">
          {teammateColor && (
            <span className="w-2 h-2 rounded-full" style={{ backgroundColor: teammateColor }} />
          )}
          <span className="text-[0.625rem] font-medium" style={{ color: teammateColor ?? "var(--text-muted)" }}>
            {teammateName}
          </span>
        </div>
      )}

      <div className="flex flex-col gap-3 min-w-0 w-full">
        {showProviderMeta && (
          <div
            className="flex items-center gap-2 min-w-0"
            data-testid="message-provider-meta"
          >
            <span
              className="rounded-full px-1.5 py-0.5 text-[0.5625rem] font-semibold uppercase tracking-[0.08em]"
              style={providerHarnessStyle}
              title={providerTooltip ?? undefined}
              aria-label={providerTooltip ?? providerHarnessLabel ?? undefined}
              data-testid="message-provider-badge"
            >
              {providerHarnessLabel}
            </span>
            {modelEffortLabel && (
              <span
                className="text-[0.625rem] min-w-0 truncate text-text-primary/50"
                title={providerTooltip ?? undefined}
                data-testid="message-model-effort"
              >
                {modelEffortLabel}
              </span>
            )}
          </div>
        )}

        {/* Render attachments for user messages */}
        {isUser && attachments && attachments.length > 0 && (
          <MessageAttachments attachments={attachments} />
        )}

        {hasCustomBody ? (
          children
        ) : hasContentBlocks ? (
          // Render content blocks in order (interleaved text and tool calls)
          // Skip child tool calls that belong to Task subagents (they render inside TaskToolCallCard)
          parsedContentBlocks.map((block, index) => {
            if (block.type === "text" && block.text) {
              return (
                <TextBubble
                  key={`block-${index}`}
                  text={block.text}
                  isUser={isUser}
                />
              );
            } else if (block.type === "tool_use" && block.name) {
              // Skip child tool calls — they're rendered inside their parent TaskToolCallCard
              if (block.id && childToolCallIds.has(block.id)) {
                return null;
              }
              const toolCall: ToolCall = {
                id: block.id || `tool-${index}`,
                name: block.name,
                arguments: block.arguments,
                result: block.result,
              };
              if (block.resultPreviewTruncated) {
                toolCall.resultPreviewTruncated = block.resultPreviewTruncated;
              }
              if (block.resultPreviewOriginalBytes != null) {
                toolCall.resultPreviewOriginalBytes = block.resultPreviewOriginalBytes;
              }
              if (block.resultPreviewLineCount != null) {
                toolCall.resultPreviewLineCount = block.resultPreviewLineCount;
              }
              if (block.resultPreviewOmittedLines != null) {
                toolCall.resultPreviewOmittedLines = block.resultPreviewOmittedLines;
              }
              if (block.detailRef) {
                toolCall.detailRef = block.detailRef;
              }
              if (block.diffContext) {
                toolCall.diffContext = block.diffContext;
              }
              if (shouldHideCompletedProjectOrchestrationToolCall(toolCall)) {
                return null;
              }
              return <ToolCallIndicator key={`block-${index}`} toolCall={toolCall} />;
            }
            return null;
          })
        ) : (
          // Legacy rendering: tool calls first, then content
          <>
            {!isUser && visibleParsedToolCalls.length > 0 && (
              <div className="space-y-1.5 overflow-hidden">
                {visibleParsedToolCalls.map((tc) => (
                  <ToolCallIndicator key={tc.id} toolCall={tc} />
                ))}
              </div>
            )}
            {/* Skip empty/whitespace-only bubbles for assistant messages
                (backend pre-creates empty assistant msg before streaming starts) */}
            {(isUser || content.trim().length > 0) && (
              <TextBubble text={content} isUser={isUser} />
            )}
          </>
        )}

        <div
          className={cn(
            "flex items-center gap-1.5 px-1 pb-[10px] text-[10px] text-text-primary/40",
            isUser ? "justify-end" : "justify-start"
          )}
          data-testid="message-meta"
        >
          <span title={formatTimestampTitle(createdAt) || undefined}>
            {formatTimestamp(createdAt)}
          </span>
          {showInlineCopy && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  onClick={() => void handleCopy()}
                  className="h-4 w-4 rounded-sm p-0 text-text-primary/40 hover:bg-[var(--overlay-moderate)] hover:text-text-primary/70"
                  aria-label={copied ? "Copied" : "Copy message"}
                  data-testid="message-copy-button"
                >
                  {copied ? (
                    <Check className="h-3 w-3 text-[var(--status-success)]" />
                  ) : (
                    <Copy className="h-3 w-3" />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent side="top" className="text-xs">
                {copied ? "Copied" : "Copy message"}
              </TooltipContent>
            </Tooltip>
          )}
        </div>
      </div>
    </div>
  );
}, (prev, next) => {
  // Custom equality function - only re-render if these props change
  // For arrays, compare by reference (they're parsed once at API layer)
  return prev.role === next.role
    && prev.content === next.content
    && prev.createdAt === next.createdAt
    && prev.children === next.children
    && prev.isLastInList === next.isLastInList
    && prev.toolCalls === next.toolCalls
    && prev.contentBlocks === next.contentBlocks
    && prev.attachments === next.attachments
    && prev.teammateName === next.teammateName
    && prev.teammateColor === next.teammateColor
    && prev.providerHarness === next.providerHarness
    && prev.providerSessionId === next.providerSessionId
    && prev.upstreamProvider === next.upstreamProvider
    && prev.providerProfile === next.providerProfile
    && prev.logicalModel === next.logicalModel
    && prev.effectiveModelId === next.effectiveModelId
    && prev.logicalEffort === next.logicalEffort
    && prev.effectiveEffort === next.effectiveEffort
    && prev.inputTokens === next.inputTokens
    && prev.outputTokens === next.outputTokens
    && prev.cacheCreationTokens === next.cacheCreationTokens
    && prev.cacheReadTokens === next.cacheReadTokens
    && prev.estimatedUsd === next.estimatedUsd
    && prev.showAssistantIcon === next.showAssistantIcon
    && prev.hideMeta === next.hideMeta;
});
