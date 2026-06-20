/**
 * ChatMessageList - Virtualized message list for chat panels
 *
 * Wraps react-virtuoso with chat-specific rendering:
 * - Auto-scroll to bottom
 * - Failed run banner header
 * - Worker executing indicator
 * - Streaming tool calls / typing indicator footer
 */

import React, { forwardRef, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useImperativeHandle } from "react";
import { Virtuoso, type ListRange, type ScrollerProps, type VirtuosoHandle } from "react-virtuoso";
import { MessageItem, MessageMeta } from "./MessageItem";
import { parseComposerReferencesFromMetadata } from "./MessageReferences.parse";
import { HookEventMessage } from "./HookEventMessage";
import { AutoVerificationCard } from "./AutoVerificationCard";
import { VerificationResultCard } from "./VerificationResultCard";
import { AUTO_VERIFICATION_KEY, VERIFICATION_RESULT_KEY } from "@/types/ideation";
import {
  ConversationTranscriptPlaceholders,
  TypingIndicator,
  FailedRunBanner,
} from "./IntegratedChatPanel.components";
import { TextBubble } from "./TextBubble";
import { ToolCallIndicator } from "./ToolCallIndicator";
import type { ToolCall } from "./ToolCallIndicator";
import type { StreamingTask, StreamingContentBlock } from "@/types/streaming-task";
import type { ContentBlockItem } from "./MessageItem";
import type { HookEvent, HookStartedEvent } from "@/types/hook-event";
import { isDiffToolCall } from "./DiffToolCallView.utils";
import { DiffToolCallView } from "./DiffToolCallView";
import { TaskSubagentCard } from "./TaskSubagentCard";
import { useChatAutoScroll } from "@/hooks/useChatAutoScroll";
import { shouldUseWebkitSafeScrollBehavior } from "@/lib/platform-quirks";
import { logger } from "@/lib/logger";
import { useMessageAttachments } from "@/hooks/useMessageAttachments";
import { ChevronDown } from "lucide-react";
import type { MessageAttachment } from "./MessageAttachments";
import { useTeamStore, selectTeammateByName, selectTeamMessages, EMPTY_TEAM_MESSAGES } from "@/stores/teamStore";
import { ToolCallStoreKeyContext } from "./tool-widgets/ToolCallStoreKeyContext";
import { shouldHideCompletedProjectOrchestrationToolCall } from "./tool-widgets/ProjectOrchestrationWidget.utils";
import type { TeamMessage } from "@/stores/teamStore";
import { TeamMessageBubble } from "./TeamMessageBubble";
import { isProviderRole } from "@/lib/chat/provider-role";
import { normalizeStreamingVerificationContentBlocks } from "./verification-tool-calls";
import { cn } from "@/lib/utils";
import { isTranscriptRootReadyForReveal } from "./ChatMessageList.readiness";
import {
  getScrollBottomDelta,
  isScrollElementVisuallyAtBottom,
  scrollElementToTrueBottom,
  shouldShowScrollToBottomControl,
  shouldStickToBottom,
  shouldTreatScrollTopDecreaseAsUserAway,
  VISUAL_BOTTOM_EPSILON_PX,
} from "./ChatMessageList.scroll";
import {
  buildStreamingTranscriptWindow,
  EMPTY_STREAMING_TRANSCRIPT_WINDOW,
  getNextStreamingTranscriptWindow,
  type StreamingTranscriptWindow,
} from "./ChatMessageList.streamingWindow";

// ============================================================================
// Constants
// ============================================================================

/** Delay for markdown content to render and expand before scroll correction */
const MARKDOWN_RENDER_DELAY_MS = 300;

/** Shared bottom-detection threshold — used by both Virtuoso atBottomThreshold prop and rAF DOM reconciliation.
 *  Must match exactly so both agree on what "at bottom" means. */
export const AT_BOTTOM_THRESHOLD = 150;

/** Final-pixel settle guard for native wheel/scrollbar bottom attempts. */
const TRUE_BOTTOM_SETTLE_THRESHOLD_PX = 32;
const BOTTOM_SCROLL_INTENT_WINDOW_MS = 800;
const TOOL_GROUP_SCROLL_ADJUSTMENT_WINDOW_MS = 800;
const MAX_TRUE_BOTTOM_SETTLE_ATTEMPTS = 2;

/** Bucket size for text length change detection during streaming.
 *  ~2 visible lines per trigger (average line ~80 chars at standard chat width → 2 lines × 80 = 160, rounded to 150). */
export const TEXT_LENGTH_BUCKET_SIZE = 150;

const INITIAL_TRANSCRIPT_PAINT_MAX_FRAMES = 240;
const INITIAL_TRANSCRIPT_PAINT_MAX_MS = 2_500;

/** Shared styles for content containers to handle long text */
const contentContainerStyle: React.CSSProperties = {
  maxWidth: "100%",
  overflowWrap: "break-word",
  wordBreak: "break-word",
  overflowAnchor: "none",
};

type ChatVirtuosoScrollerProps = ScrollerProps & {
  context?: unknown;
};

const ChatVirtuosoScroller = forwardRef<HTMLDivElement, ChatVirtuosoScrollerProps>(
  function ChatVirtuosoScroller({ context: _context, style, ...props }, ref) {
    return (
      <div
        {...props}
        ref={ref}
        data-chat-virtuoso-scroller="true"
        style={{
          ...style,
          overflowAnchor: "none",
        }}
      />
    );
  },
);

function ContentShell({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string | undefined;
}) {
  return (
    <div
      className={cn("w-full", className ? ["mx-auto", className] : undefined)}
      data-testid="chat-message-content-shell"
    >
      {children}
    </div>
  );
}

function ScrollToBottomControl({
  visible,
  onClick,
  onWheel,
}: {
  visible: boolean;
  onClick: () => void;
  onWheel: React.WheelEventHandler<HTMLButtonElement>;
}) {
  return (
    <div
      data-testid="chat-scroll-to-bottom-control"
      aria-hidden={!visible}
      className={cn(
        "absolute bottom-4 left-0 right-0 z-10 flex justify-center pointer-events-none",
        visible ? "opacity-100" : "opacity-0",
      )}
      style={{
        contain: "layout paint style",
      }}
    >
      <button
        type="button"
        data-testid="chat-scroll-to-bottom-button"
        onClick={onClick}
        onWheel={onWheel}
        disabled={!visible}
        tabIndex={visible ? 0 : -1}
        className={cn(
          "inline-flex h-8 items-center gap-1.5 rounded-md border px-3 text-xs font-medium",
          "bg-[color-mix(in_srgb,var(--bg-surface)_72%,var(--bg-base))]",
          "border-[color-mix(in_srgb,var(--border-subtle)_45%,var(--text-muted))]",
          "text-[var(--text-primary)] hover:bg-[color-mix(in_srgb,var(--bg-surface)_58%,var(--bg-base))]",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]",
          visible ? "pointer-events-auto cursor-pointer" : "pointer-events-none cursor-default",
        )}
      >
        <span>Scroll to bottom</span>
        <ChevronDown className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
      </button>
    </div>
  );
}

function scrollElementByDelta(element: HTMLElement, deltaX: number, deltaY: number) {
  if (typeof element.scrollBy === "function") {
    element.scrollBy({
      left: deltaX,
      top: deltaY,
      behavior: "auto",
    });
    return;
  }

  element.scrollLeft += deltaX;
  element.scrollTop += deltaY;
}

/** Stable empty arrays — avoids new refs on each render when props are omitted */
const EMPTY_HOOK_EVENTS: HookEvent[] = [];
const EMPTY_ACTIVE_HOOKS: HookStartedEvent[] = [];

// ============================================================================
// Types
// ============================================================================

export interface ChatMessageData {
  id: string;
  role: string;
  content: string;
  createdAt: string;
  parentMessageId?: string | null;
  toolCalls?: ToolCall[] | null;
  contentBlocks?: ContentBlockItem[] | null;
  attachments?: MessageAttachment[];
  sender?: string | null;
  metadata?: string | null;
  providerHarness?: string | null;
  providerSessionId?: string | null;
  upstreamProvider?: string | null;
  providerProfile?: string | null;
  logicalModel?: string | null;
  effectiveModelId?: string | null;
  logicalEffort?: string | null;
  effectiveEffort?: string | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cacheCreationTokens?: number | null;
  cacheReadTokens?: number | null;
  estimatedUsd?: number | null;
  timelineSequence?: number | null;
}

type ToolCallGroupMarker = {
  key: string;
  count: number;
  position: "toggle" | "covered";
};

type ToolCallGroupScrollAnchor = {
  groupKey: string;
  anchorTop: number | null;
  scrollTop: number;
  bottomDelta: number;
  wasVisuallyAtBottom: boolean;
};

type TimelineMessageItem = {
  kind: "message";
  data: ChatMessageData;
  sortTime: number;
  toolCallGroup?: ToolCallGroupMarker;
};

type StreamingToolUseBlock = Extract<StreamingContentBlock, { type: "tool_use" }>;
type StreamingToolGroupEntry = {
  block: StreamingToolUseBlock;
  index: number;
};

/** Discriminated union for timeline items when hook events are interleaved */
type TimelineItem =
  | TimelineMessageItem
  | { kind: "hook"; data: HookEvent | HookStartedEvent; sortTime: number }
  | { kind: "team_event"; data: TeamMessage; sortTime: number }
  | { kind: "streaming"; sortTime: number };

function parseMessageMetadata(metadata: string | null | undefined): Record<string, unknown> | null {
  if (!metadata) return null;
  try {
    return JSON.parse(metadata) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function renderSystemCard(
  metadata: Record<string, unknown> | null,
  content: string,
  createdAt: string,
) {
  if (!metadata) return null;

  if (metadata[AUTO_VERIFICATION_KEY]) {
    return <AutoVerificationCard content={content} createdAt={createdAt} />;
  }

  if (metadata[VERIFICATION_RESULT_KEY]) {
    const blockers = Array.isArray(metadata.top_blockers)
      ? metadata.top_blockers
          .filter((item): item is { severity?: unknown; description?: unknown } => (
            item != null && typeof item === "object"
          ))
          .map((item) => ({
            severity: typeof item.severity === "string" ? item.severity : "unknown",
            description: typeof item.description === "string" ? item.description : "",
          }))
          .filter((item) => item.description.length > 0)
      : [];

    return (
      <VerificationResultCard
        summary={typeof metadata.summary === "string" ? metadata.summary : content}
        convergenceReason={typeof metadata.convergence_reason === "string" ? metadata.convergence_reason : null}
        currentRound={typeof metadata.current_round === "number" ? metadata.current_round : null}
        maxRounds={typeof metadata.max_rounds === "number" ? metadata.max_rounds : null}
        recommendedNextAction={
          typeof metadata.recommended_next_action === "string"
            ? metadata.recommended_next_action
            : null
        }
        blockers={blockers}
        actionableForParent={metadata.actionable_for_parent === true}
      />
    );
  }

  return null;
}

function hasSystemCardMetadata(metadata: Record<string, unknown> | null) {
  return Boolean(metadata?.[AUTO_VERIFICATION_KEY] || metadata?.[VERIFICATION_RESULT_KEY]);
}

function isPersistedTimelineToolCallMessage(message: ChatMessageData): boolean {
  if (!isProviderRole(message.role) || message.timelineSequence == null) {
    return false;
  }
  const blocks = message.contentBlocks;
  return blocks?.length === 1 && blocks[0]?.type === "tool_use";
}

function sameToolGroupSurface(left: ChatMessageData, right: ChatMessageData): boolean {
  return left.role === right.role
    && (left.sender ?? null) === (right.sender ?? null)
    && (left.providerHarness ?? null) === (right.providerHarness ?? null)
    && (left.providerSessionId ?? null) === (right.providerSessionId ?? null)
    && (left.upstreamProvider ?? null) === (right.upstreamProvider ?? null)
    && (left.providerProfile ?? null) === (right.providerProfile ?? null);
}

function canContinueToolCallGroup(
  first: ChatMessageData,
  previous: ChatMessageData,
  next: ChatMessageData,
): boolean {
  if (!isPersistedTimelineToolCallMessage(next) || !sameToolGroupSurface(first, next)) {
    return false;
  }
  if (first.parentMessageId || next.parentMessageId) {
    if (!first.parentMessageId || first.parentMessageId !== next.parentMessageId) {
      return false;
    }
  }
  if (
    previous.timelineSequence != null
    && next.timelineSequence != null
    && next.timelineSequence !== previous.timelineSequence + 1
  ) {
    return false;
  }
  return true;
}

function collectToolCallGroupRun(
  messages: ChatMessageData[],
  startIndex: number,
): ChatMessageData[] | null {
  const first = messages[startIndex];
  if (!first || !isPersistedTimelineToolCallMessage(first)) {
    return null;
  }

  const group = [first];
  let previous = first;
  for (let index = startIndex + 1; index < messages.length; index += 1) {
    const next = messages[index];
    if (!next || !canContinueToolCallGroup(first, previous, next)) {
      break;
    }
    group.push(next);
    previous = next;
  }

  return group.length >= 1 ? group : null;
}

function toolCallGroupKey(messages: ChatMessageData[]): string {
  const first = messages[0];
  const last = messages[messages.length - 1];
  if (!first || !last) {
    return "tool-call-group:empty";
  }
  const firstSequence = first.timelineSequence ?? first.id;
  const lastSequence = last.timelineSequence ?? last.id;
  return [
    "tool-call-group",
    first.parentMessageId ?? first.id,
    firstSequence,
    lastSequence,
    messages.length,
  ].join(":");
}

function streamingToolGroupKey(entries: StreamingToolGroupEntry[]): string {
  const first = entries[0];
  if (!first) {
    return "streaming-tool-group:empty";
  }
  return [
    "streaming-tool-group",
    first.block.toolCall.id || first.block.seq || first.index,
  ].join(":");
}

function isCollapsedToolCallGroupCoveredItem(
  item: TimelineItem,
  expandedToolGroupKeys: Set<string>,
): boolean {
  return item.kind === "message"
    && item.toolCallGroup?.position === "covered"
    && !expandedToolGroupKeys.has(item.toolCallGroup.key);
}

function isVisibleTimelineItem(
  item: TimelineItem,
  expandedToolGroupKeys: Set<string>,
): boolean {
  return !isCollapsedToolCallGroupCoveredItem(item, expandedToolGroupKeys);
}

function findToolCallGroupToggleElement(root: ParentNode, groupKey: string): HTMLElement | null {
  const candidates = root.querySelectorAll<HTMLElement>("[data-chat-tool-call-group-key]");
  for (const candidate of candidates) {
    if (candidate.dataset.chatToolCallGroupKey === groupKey) {
      return candidate;
    }
  }
  return null;
}

function clampScrollTop(element: HTMLElement, scrollTop: number): number {
  const maxScrollTop = Math.max(0, element.scrollHeight - element.clientHeight);
  return Math.max(0, Math.min(maxScrollTop, scrollTop));
}

function ToolCallGroupToggle({
  groupKey,
  count,
  isExpanded,
  onToggle,
}: {
  groupKey: string;
  count: number;
  isExpanded: boolean;
  onToggle: React.MouseEventHandler<HTMLButtonElement>;
}) {
  const label = isExpanded ? `Hide ${count} tool call${count === 1 ? "" : "s"}` : `Agent called ${count} tool${count === 1 ? "" : "s"}`;
  return (
    <button
      type="button"
      data-testid="tool-call-group-toggle"
      data-chat-tool-call-group-key={groupKey}
      aria-expanded={isExpanded}
      aria-label={label}
      onClick={onToggle}
      className="inline-flex max-w-full items-center rounded-md px-2 py-1 text-[0.6875rem] font-medium transition-opacity hover:opacity-80"
      style={{
        backgroundColor: "var(--bg-elevated)",
        color: "var(--text-secondary)",
      }}
    >
      {label}
    </button>
  );
}

function senderGroupPart(value: string | null | undefined) {
  const trimmed = value?.trim();
  return trimmed && trimmed.length > 0 ? trimmed : "";
}

function assistantSenderGroupKeyForMessage(message: ChatMessageData): string | null {
  if (!isProviderRole(message.role)) {
    return null;
  }
  const metadata = parseMessageMetadata(message.metadata);
  if (hasSystemCardMetadata(metadata)) {
    return null;
  }
  return [
    "assistant",
    senderGroupPart(message.sender),
    senderGroupPart(message.providerHarness),
    senderGroupPart(message.providerSessionId),
    senderGroupPart(message.upstreamProvider),
    senderGroupPart(message.providerProfile),
  ].join("\u0000");
}

function assistantSenderGroupKeyForTimelineItem(
  item: TimelineItem,
  fallbackProviderHarness: string | null | undefined,
  fallbackProviderSessionId: string | null | undefined,
): string | null {
  if (item.kind === "message") {
    return assistantSenderGroupKeyForMessage(item.data);
  }
  if (item.kind === "streaming") {
    return [
      "assistant",
      "",
      senderGroupPart(fallbackProviderHarness),
      senderGroupPart(fallbackProviderSessionId),
      "",
      "",
    ].join("\u0000");
  }
  return null;
}

const DEFAULT_ASSISTANT_GROUP_STATE = {
  reserveAssistantGutter: false,
  showSenderHeader: true,
};

function ToolCallGroupToggleRow({
  msg,
  marker,
  senderGroupState,
  isLastInList,
  isExpanded,
  teammateName,
  teammateColor,
  onToggle,
  contentWidthClassName,
  rowRef,
}: {
  msg: ChatMessageData;
  marker: ToolCallGroupMarker;
  senderGroupState: typeof DEFAULT_ASSISTANT_GROUP_STATE;
  isLastInList: boolean;
  isExpanded: boolean;
  teammateName: string | null;
  teammateColor: string | null;
  onToggle: React.MouseEventHandler<HTMLButtonElement>;
  contentWidthClassName?: string | undefined;
  rowRef?: React.Ref<HTMLDivElement> | undefined;
}) {
  return (
    <div
      ref={rowRef}
      className="px-3 w-full"
      data-chat-last-rendered-row={isLastInList ? "true" : undefined}
      style={contentContainerStyle}
    >
      <ContentShell className={contentWidthClassName}>
        <MessageItem
          role={msg.role}
          content=""
          createdAt={msg.createdAt}
          isLastInList={isLastInList}
          toolCalls={null}
          contentBlocks={null}
          teammateName={teammateName}
          teammateColor={teammateColor}
          providerHarness={msg.providerHarness}
          providerSessionId={msg.providerSessionId}
          upstreamProvider={msg.upstreamProvider}
          providerProfile={msg.providerProfile}
          logicalModel={msg.logicalModel}
          effectiveModelId={msg.effectiveModelId}
          logicalEffort={msg.logicalEffort}
          effectiveEffort={msg.effectiveEffort}
          inputTokens={msg.inputTokens}
          outputTokens={msg.outputTokens}
          cacheCreationTokens={msg.cacheCreationTokens}
          cacheReadTokens={msg.cacheReadTokens}
          estimatedUsd={msg.estimatedUsd}
          showAssistantIcon={senderGroupState.showSenderHeader}
          reserveAssistantIconSpace={senderGroupState.reserveAssistantGutter}
          showProviderMeta={senderGroupState.showSenderHeader}
          hideMeta
        >
          <ToolCallGroupToggle
            groupKey={marker.key}
            count={marker.count}
            isExpanded={isExpanded}
            onToggle={onToggle}
          />
        </MessageItem>
      </ContentShell>
    </div>
  );
}

function isMessageAtOrAfter(candidate: ChatMessageData, marker: ChatMessageData) {
  const candidateTime = new Date(candidate.createdAt).getTime();
  const markerTime = new Date(marker.createdAt).getTime();
  return candidateTime > markerTime || (candidateTime === markerTime && candidate.id >= marker.id);
}

function latestMessageByCreatedAt(
  messages: ChatMessageData[],
  predicate: (message: ChatMessageData) => boolean,
) {
  let latest: ChatMessageData | null = null;
  let latestTime = -Infinity;

  for (const message of messages) {
    if (!predicate(message)) {
      continue;
    }
    const time = new Date(message.createdAt).getTime();
    if (
      latest === null ||
      time > latestTime ||
      (time === latestTime && message.id > latest.id)
    ) {
      latest = message;
      latestTime = time;
    }
  }

  return latest;
}

function hasRenderablePersistedContent(message: ChatMessageData) {
  if (message.content.trim().length > 0) {
    return true;
  }
  if ((message.toolCalls?.length ?? 0) > 0) {
    return true;
  }
  return (message.contentBlocks?.length ?? 0) > 0;
}

function countCompletedToolCalls(toolCalls: Iterable<ToolCall>): number {
  let count = 0;
  for (const toolCall of toolCalls) {
    if (toolCall.result != null || toolCall.error != null) {
      count += 1;
    }
  }
  return count;
}

function buildStreamingTaskResultSignature(
  streamingTasks: Map<string, StreamingTask> | undefined
): string {
  if (!streamingTasks || streamingTasks.size === 0) {
    return "";
  }

  return Array.from(streamingTasks.values())
    .map((task) => [
      task.toolUseId,
      task.status,
      task.completedAt ?? "",
      task.totalDurationMs ?? "",
      task.totalTokens ?? "",
      task.totalToolUseCount ?? "",
      task.childToolCalls.length,
      countCompletedToolCalls(task.childToolCalls),
    ].join(":"))
    .join("|");
}

function getCurrentTurnProviderMessageId(
  messages: ChatMessageData[],
  {
    hasActiveStreaming,
    isAgentRunning,
    isFinalizing,
  }: {
    hasActiveStreaming: boolean;
    isAgentRunning: boolean;
    isFinalizing: boolean;
  },
) {
  const shouldSuppressActiveTurnSnapshot = hasActiveStreaming || isFinalizing;
  const shouldSuppressEmptyCurrentTurnSnapshot = isAgentRunning;
  if (!shouldSuppressActiveTurnSnapshot && !shouldSuppressEmptyCurrentTurnSnapshot) {
    return null;
  }

  const latestUserMessage = latestMessageByCreatedAt(
    messages,
    (message) => message.role === "user",
  );
  const latestProviderMessage = latestMessageByCreatedAt(
    messages,
    (message) => {
      if (!isProviderRole(message.role)) {
        return false;
      }
      if (!latestUserMessage) {
        return true;
      }
      return isMessageAtOrAfter(message, latestUserMessage);
    },
  );

  if (!latestProviderMessage) {
    return null;
  }

  const isEmptySnapshot = !hasRenderablePersistedContent(latestProviderMessage);
  const belongsToCurrentTurn = latestUserMessage
    ? isMessageAtOrAfter(latestProviderMessage, latestUserMessage)
    : isEmptySnapshot;

  if (!belongsToCurrentTurn) {
    return null;
  }

  if (shouldSuppressActiveTurnSnapshot) {
    return latestProviderMessage.id;
  }

  return isEmptySnapshot ? latestProviderMessage.id : null;
}

interface ChatMessageListProps {
  messages: ChatMessageData[];
  /** Conversation ID - used as key to force remount on conversation switch */
  conversationId: string | null;
  /** Absolute index of the first loaded message in the full conversation timeline */
  firstItemIndex?: number;
  /** Show failed run banner */
  failedRun?: { id: string; errorMessage: string } | null;
  /** Callback when failed run banner is dismissed */
  onDismissFailedRun?: (runId: string) => void;
  /** Is agent currently sending/responding */
  isSending: boolean;
  isAgentRunning: boolean;
  /** Short label shown beside the active typing indicator */
  typingIndicatorLabel?: string | null | undefined;
  /** Streaming tool calls to display */
  streamingToolCalls: ToolCall[];
  /** Streaming subagent tasks — Map keyed by tool_use_id */
  streamingTasks?: Map<string, StreamingTask>;
  /** Streaming content blocks (text and tool calls interleaved) */
  streamingContentBlocks?: StreamingContentBlock[];
  /** Optional timestamp to scroll to (for history mode) - scrolls to first message at or after this time */
  scrollToTimestamp?: string | null;
  /** Resolved hook events (completed + blocks) — optional, interleaved chronologically */
  hookEvents?: HookEvent[];
  /** Currently running hooks — optional, interleaved chronologically */
  activeHooks?: HookStartedEvent[];
  /** Whether the conversation is finalizing (between message_created and query refetch) */
  isFinalizing?: boolean;
  /** Team filter for message filtering (team mode) */
  teamFilter?: "lead" | string | undefined;
  /** Context key for team store lookup (team mode) */
  contextKey?: string | undefined;
  /** Provider metadata for the active conversation */
  providerHarness?: string | null | undefined;
  providerSessionId?: string | null | undefined;
  contentWidthClassName?: string | undefined;
  topInsetClassName?: string | undefined;
  hasOlderMessages?: boolean;
  isFetchingOlderMessages?: boolean;
  onLoadOlderMessages?: (() => void | Promise<void>) | undefined;
  /** Incremented by the host when sibling chrome below the transcript changes size. */
  externalLayoutVersion?: number | undefined;
  initialPaintCoverKey?: string | null | undefined;
  onInitialPaintReady?: ((key: string) => void) | undefined;
}

// ============================================================================
// Component
// ============================================================================

export const ChatMessageList = forwardRef<VirtuosoHandle, ChatMessageListProps>(
  function ChatMessageList(
    {
      messages,
      conversationId,
      firstItemIndex = 0,
      failedRun,
      onDismissFailedRun,
      isSending,
      isAgentRunning,
      typingIndicatorLabel,
      streamingToolCalls,
      streamingTasks,
      streamingContentBlocks,
      scrollToTimestamp,
      hookEvents = EMPTY_HOOK_EVENTS,
      activeHooks = EMPTY_ACTIVE_HOOKS,
      isFinalizing = false,
      teamFilter,
      contextKey,
      providerHarness,
      providerSessionId,
      contentWidthClassName,
      topInsetClassName,
      hasOlderMessages = false,
      isFetchingOlderMessages = false,
      onLoadOlderMessages,
      externalLayoutVersion = 0,
      initialPaintCoverKey = null,
      onInitialPaintReady,
    },
    ref
  ) {
    const preferredScrollBehavior = shouldUseWebkitSafeScrollBehavior()
      ? "auto"
      : "smooth";
    const lastMessage = messages[messages.length - 1] ?? null;
    const lastUserMessageId = lastMessage?.role === "user" ? lastMessage.id : null;

    // Internal ref for scroll operations
    const virtuosoRef = useRef<VirtuosoHandle>(null);
    const hasScrolledRef = useRef<string | null>(null);
    const previousLastItemIndexRef = useRef<number | null>(null);
    // Track previous shouldFilterLastAssistant to detect false→true→false transition
    const prevShouldFilterRef = useRef(false);
    const bottomPinRafIdsRef = useRef<number[]>([]);
    const bottomPinTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const lastUserMessageIdRef = useRef<string | null>(lastUserMessageId);
    const agentRunningRef = useRef(isAgentRunning);
    const conversationLastUserMessageIdRef = useRef<string | null>(lastUserMessageId);
    const conversationAgentRunningRef = useRef(isAgentRunning);
    const scrollToTimestampRef = useRef(scrollToTimestamp);
    // rAF reconciliation refs — used to keep isAtBottom accurate when footer grows
    const scrollerElRef = useRef<HTMLElement | null>(null);
    const lastObservedScrollTopRef = useRef<number | null>(null);
    const reconcileRafRef = useRef<number | null>(null);
    const scrollerResizeObserverRef = useRef<ResizeObserver | null>(null);
    const scrollerResizeRafRef = useRef<number | null>(null);
    const bottomScrollIntentUntilRef = useRef<number | null>(null);
    const isUserScrollingAwayFromBottomRef = useRef(false);
    const hasUserScrollInputRef = useRef(false);
    const userScrollAwayVersionRef = useRef(0);
    const virtuosoAtBottomSettleRafRef = useRef<number | null>(null);
    const bottomSettleAttemptCountRef = useRef(0);
    const transcriptRootResizeObserverRef = useRef<ResizeObserver | null>(null);
    const transcriptRootResizeRafRef = useRef<number | null>(null);
    const totalListHeightRafRef = useRef<number | null>(null);
    const previousTotalListHeightRef = useRef<number>(-1);
    const pendingToolGroupScrollAnchorRef = useRef<ToolCallGroupScrollAnchor | null>(null);
    const toolGroupScrollAdjustmentUntilRef = useRef<number | null>(null);
    const transcriptRootPrevHeightRef = useRef<number>(-1);
    const transcriptRootMountedRef = useRef(false);
    const isTestEnv = import.meta.env.VITEST;
    const [isVisuallyAtBottom, setIsVisuallyAtBottomState] = useState(true);
    const isVisuallyAtBottomRef = useRef(true);
    const [hasScrollerElement, setHasScrollerElement] = useState(false);
    const [hasScrollableOverflow, setHasScrollableOverflow] = useState(false);
    const [isLastItemVisible, setIsLastItemVisible] = useState<boolean | null>(true);
    const [expandedToolGroupKeys, setExpandedToolGroupKeys] = useState<Set<string>>(
      () => new Set(),
    );
    const expandedToolGroupConversationRef = useRef<string | undefined>(conversationId);
    const isLastItemVisibleRef = useRef<boolean | null>(true);

    // Footer ResizeObserver refs — for height-driven auto-scroll (G2 fix)
    const footerElRef = useRef<HTMLDivElement | null>(null);
    const footerResizeRafRef = useRef<number | null>(null);
    const footerObserverRef = useRef<ResizeObserver | null>(null);
    const footerPrevHeightRef = useRef<number>(-1); // -1 = uninitialized sentinel
    const footerMountedRef = useRef(false); // H2 fix: skip initial mount observation
    const hasFooterStreamingContentRef = useRef(false);
    const lastRenderedRowElRef = useRef<HTMLDivElement | null>(null);
    const lastRenderedRowObserverRef = useRef<ResizeObserver | null>(null);
    const lastRenderedRowResizeRafRef = useRef<number | null>(null);
    const lastRenderedRowPrevHeightRef = useRef<number>(-1);
    const lastRenderedRowMountedRef = useRef(false);
    const transcriptRootRef = useRef<HTMLDivElement | null>(null);
    const initialPaintReadyFrameRef = useRef<number | null>(null);
    const initialPaintReadyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    useEffect(() => {
      if (expandedToolGroupConversationRef.current === conversationId) {
        return;
      }
      expandedToolGroupConversationRef.current = conversationId;
      setExpandedToolGroupKeys(new Set());
    }, [conversationId]);

    const getToolGroupScrollContainer = useCallback((): HTMLElement | null => {
      return scrollerElRef.current ?? (isTestEnv ? transcriptRootRef.current : null);
    }, [isTestEnv]);

    const captureToolGroupScrollAnchor = useCallback(
      (groupKey: string, toggleElement: HTMLElement | null): ToolCallGroupScrollAnchor | null => {
        const scroller = getToolGroupScrollContainer();
        /* c8 ignore next 3 -- the scroller can detach between click capture and state commit. */
        if (!scroller) {
          return null;
        }
        const anchorElement =
          toggleElement ?? findToolCallGroupToggleElement(scroller, groupKey);
        return {
          groupKey,
          anchorTop: anchorElement ? anchorElement.getBoundingClientRect().top : null,
          scrollTop: scroller.scrollTop,
          bottomDelta: getScrollBottomDelta(scroller),
          wasVisuallyAtBottom: isScrollElementVisuallyAtBottom(scroller),
        };
      },
      [getToolGroupScrollContainer],
    );

    const toggleToolCallGroup = useCallback((groupKey: string, toggleElement?: HTMLElement | null) => {
      const anchor = captureToolGroupScrollAnchor(groupKey, toggleElement ?? null);
      if (anchor) {
        pendingToolGroupScrollAnchorRef.current = anchor;
        toolGroupScrollAdjustmentUntilRef.current =
          performance.now() + TOOL_GROUP_SCROLL_ADJUSTMENT_WINDOW_MS;
        if (!anchor.wasVisuallyAtBottom) {
          isUserScrollingAwayFromBottomRef.current = true;
          userScrollAwayVersionRef.current += 1;
        }
      }
      setExpandedToolGroupKeys((current) => {
        const next = new Set(current);
        if (next.has(groupKey)) {
          next.delete(groupKey);
        } else {
          next.add(groupKey);
        }
        return next;
      });
    }, [captureToolGroupScrollAnchor]);
    const initialPaintReadyFallbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const initialPaintReadyAttemptRef = useRef(0);
    const initialPendingPaintCoverKey =
      initialPaintCoverKey && messages.length > 0 ? initialPaintCoverKey : null;
    const pendingInitialPaintCoverKeyRef = useRef<string | null>(initialPendingPaintCoverKey);
    const completedInitialPaintCoverKeyRef = useRef<string | null>(null);
    const [pendingInitialPaintCoverKey, setPendingInitialPaintCoverKeyState] =
      useState<string | null>(() => initialPendingPaintCoverKey);
    const shouldShowInitialPaintCover =
      pendingInitialPaintCoverKey !== null && messages.length > 0;

    const setPendingInitialPaintCoverKey = useCallback((nextKey: string | null) => {
      pendingInitialPaintCoverKeyRef.current = nextKey;
      setPendingInitialPaintCoverKeyState(nextKey);
    }, []);

    const setIsVisuallyAtBottom = useCallback((nextValue: boolean) => {
      if (isVisuallyAtBottomRef.current === nextValue) {
        return;
      }
      isVisuallyAtBottomRef.current = nextValue;
      setIsVisuallyAtBottomState(nextValue);
    }, []);

    const cancelInitialPaintReadyJob = useCallback(() => {
      if (initialPaintReadyFrameRef.current !== null) {
        cancelAnimationFrame(initialPaintReadyFrameRef.current);
        initialPaintReadyFrameRef.current = null;
      }
      if (initialPaintReadyTimerRef.current !== null) {
        clearTimeout(initialPaintReadyTimerRef.current);
        initialPaintReadyTimerRef.current = null;
      }
      if (initialPaintReadyFallbackTimerRef.current !== null) {
        clearTimeout(initialPaintReadyFallbackTimerRef.current);
        initialPaintReadyFallbackTimerRef.current = null;
      }
      initialPaintReadyAttemptRef.current = 0;
    }, []);

    useEffect(
      () => () => cancelInitialPaintReadyJob(),
      [cancelInitialPaintReadyJob],
    );

    useEffect(() => {
      const nextKey = initialPaintCoverKey && messages.length > 0 ? initialPaintCoverKey : null;

      if (!nextKey) {
        completedInitialPaintCoverKeyRef.current = null;
        if (pendingInitialPaintCoverKeyRef.current !== null) {
          cancelInitialPaintReadyJob();
          setPendingInitialPaintCoverKey(null);
        }
        return;
      }

      if (completedInitialPaintCoverKeyRef.current === nextKey) {
        if (pendingInitialPaintCoverKeyRef.current !== null) {
          cancelInitialPaintReadyJob();
          setPendingInitialPaintCoverKey(null);
        }
        return;
      }

      if (pendingInitialPaintCoverKeyRef.current === nextKey) {
        return;
      }

      cancelInitialPaintReadyJob();
      setPendingInitialPaintCoverKey(nextKey);
    }, [cancelInitialPaintReadyJob, initialPaintCoverKey, messages.length, setPendingInitialPaintCoverKey]);

    const isTranscriptDomReady = useCallback(() => {
      return isTranscriptRootReadyForReveal(transcriptRootRef.current);
    }, []);

    const scheduleInitialPaintReadyCheck = useCallback(() => {
      if (!pendingInitialPaintCoverKey) {
        return;
      }
      if (initialPaintReadyFrameRef.current !== null || initialPaintReadyTimerRef.current !== null) {
        return;
      }

      const complete = () => {
        const readyKey = pendingInitialPaintCoverKeyRef.current;
        if (!readyKey) {
          return;
        }
        if (initialPaintReadyFrameRef.current !== null) {
          cancelAnimationFrame(initialPaintReadyFrameRef.current);
          initialPaintReadyFrameRef.current = null;
        }
        if (initialPaintReadyFallbackTimerRef.current !== null) {
          clearTimeout(initialPaintReadyFallbackTimerRef.current);
          initialPaintReadyFallbackTimerRef.current = null;
        }
        initialPaintReadyTimerRef.current = null;
        initialPaintReadyAttemptRef.current = 0;
        completedInitialPaintCoverKeyRef.current = readyKey;
        setPendingInitialPaintCoverKey(null);
        onInitialPaintReady?.(readyKey);
      };

      const check = () => {
        initialPaintReadyFrameRef.current = null;
        initialPaintReadyAttemptRef.current += 1;

        if (
          !isTranscriptDomReady() &&
          initialPaintReadyAttemptRef.current < INITIAL_TRANSCRIPT_PAINT_MAX_FRAMES
        ) {
          initialPaintReadyFrameRef.current = requestAnimationFrame(check);
          return;
        }

        initialPaintReadyTimerRef.current = setTimeout(complete, 0);
      };

      initialPaintReadyFrameRef.current = requestAnimationFrame(check);
      initialPaintReadyFallbackTimerRef.current = setTimeout(
        complete,
        INITIAL_TRANSCRIPT_PAINT_MAX_MS,
      );
    }, [isTranscriptDomReady, onInitialPaintReady, pendingInitialPaintCoverKey, setPendingInitialPaintCoverKey]);

    useEffect(() => {
      conversationLastUserMessageIdRef.current = lastUserMessageId;
      conversationAgentRunningRef.current = isAgentRunning;
    }, [isAgentRunning, lastUserMessageId]);

    // Forward the ref to parent
    useImperativeHandle(ref, () => virtuosoRef.current!, []);

    // Team system messages for inline display
    const teamMsgSelector = useMemo(
      () => contextKey ? selectTeamMessages(contextKey) : () => EMPTY_TEAM_MESSAGES,
      [contextKey],
    );
    const teamMessages = useTeamStore(teamMsgSelector);

    const { data: attachmentsMap } = useMessageAttachments(messages, conversationId, {
      enabled: !shouldShowInitialPaintCover,
    });
    const normalizedStreamingContentBlocks = useMemo(
      () => normalizeStreamingVerificationContentBlocks(streamingContentBlocks),
      [streamingContentBlocks],
    );
    const liveStreamingTranscriptWindow = useMemo(
      () => buildStreamingTranscriptWindow(normalizedStreamingContentBlocks, streamingTasks),
      [normalizedStreamingContentBlocks, streamingTasks],
    );
    const [streamingTranscriptWindow, setStreamingTranscriptWindow] =
      useState<StreamingTranscriptWindow>(EMPTY_STREAMING_TRANSCRIPT_WINDOW);

    const renderedStreamingContentBlocks = streamingTranscriptWindow.contentBlocks;

    // Footer content hash — drives the streaming auto-scroll useEffect below.
    // NOTE: Virtuoso's followOutput does NOT react to context/Footer changes,
    // only to totalCount changes. We use autoscrollToBottom() imperatively instead.
    const totalChildCalls = useMemo(() => {
      if (!streamingTasks || streamingTasks.size === 0) return 0;
      let count = 0;
      for (const task of streamingTasks.values()) {
        count += task.childToolCalls.length;
      }
      return count;
    }, [streamingTasks]);
    const totalChildToolResults = useMemo(() => {
      if (!streamingTasks || streamingTasks.size === 0) return 0;
      let count = 0;
      for (const task of streamingTasks.values()) {
        count += countCompletedToolCalls(task.childToolCalls);
      }
      return count;
    }, [streamingTasks]);
    const streamingTaskResultSignature = useMemo(
      () => buildStreamingTaskResultSignature(streamingTasks),
      [streamingTasks],
    );

    // Tracks running max of text length across all streaming blocks.
    // State (not a ref) so changes propagate to footerContentHash and trigger autoscroll.
    // Math.max(prev, total) ensures the bucket never decreases mid-stream — prevents
    // bucket regression when tool_use blocks are inserted between text blocks.
    const [cumulativeTextLength, setCumulativeTextLength] = useState(0);

    // Recompute cumulative text length whenever streaming blocks change.
    // Resets to 0 when streaming ends (no blocks) so the next stream starts fresh.
    useEffect(() => {
      if (!renderedStreamingContentBlocks.length) {
        setCumulativeTextLength(0);
        return;
      }
      const total = renderedStreamingContentBlocks.reduce(
        (sum, block) => block.type === "text" ? sum + block.text.length : sum, 0
      );
      setCumulativeTextLength(prev => Math.max(prev, total));
    }, [renderedStreamingContentBlocks]);

    const hasRenderableStreamingBlocks = useMemo(
      () =>
        renderedStreamingContentBlocks.some((block) => {
          if (block.type === "text") {
            return block.text.trim().length > 0;
          }
          if (block.type === "task") {
            return Boolean(streamingTasks?.get(block.toolUseId));
          }
          return true;
        }),
      [renderedStreamingContentBlocks, streamingTasks],
    );
    const hasRenderableStreamingWidgets = useMemo(
      () =>
        renderedStreamingContentBlocks.some((block) => {
          if (block.type === "text") {
            return false;
          }
          if (block.type === "task") {
            return Boolean(streamingTasks?.get(block.toolUseId));
          }
          return !shouldHideCompletedProjectOrchestrationToolCall(block.toolCall);
        }),
      [renderedStreamingContentBlocks, streamingTasks],
    );

    const shouldShowActiveTypingIndicator = isSending || isAgentRunning;
    const activeTypingIndicatorLabel =
      typingIndicatorLabel?.trim()
        || (isSending ? "Starting agent" : isAgentRunning ? "Agent working" : undefined);
    const hasLiveStreamingBlocks =
      normalizedStreamingContentBlocks.length > 0 || Boolean(streamingTasks && streamingTasks.size > 0);
    const shouldShowFooterFallback =
      (isSending || isAgentRunning) && !hasRenderableStreamingBlocks && !hasLiveStreamingBlocks;
    const hasFooterStreamingContent = hasRenderableStreamingBlocks || shouldShowFooterFallback;
    const hasVisiblePendingToolFallback =
      shouldShowFooterFallback &&
      streamingToolCalls.some((tc) => !shouldHideCompletedProjectOrchestrationToolCall(tc));
    const hasRenderableStreamingText =
      renderedStreamingContentBlocks.some(
        (block) => block.type === "text" && block.text.trim().length > 0
      );
    const shouldRenderStreamingContentGroup =
      hasRenderableStreamingText || hasRenderableStreamingWidgets || hasVisiblePendingToolFallback;
    const streamingMessageCreatedAt = useMemo(
      () => hasFooterStreamingContent ? new Date().toISOString() : "",
      [hasFooterStreamingContent],
    );

    useEffect(() => {
      hasFooterStreamingContentRef.current = hasFooterStreamingContent;
    }, [hasFooterStreamingContent]);

    const footerContentHash = useMemo(() => ({
      toolCallCount: streamingToolCalls.length,
      // G1 fix: results update existing blocks (count unchanged) — track result arrivals separately
      toolResultCount: streamingToolCalls.filter(tc => tc.result != null || tc.error != null).length,
      childCallCount: totalChildCalls,
      childResultCount: totalChildToolResults,
      taskCount: streamingTasks?.size ?? 0,
      taskResultSignature: streamingTaskResultSignature,
      contentBlockCount: renderedStreamingContentBlocks.length,
      textLengthBucket: Math.floor(cumulativeTextLength / TEXT_LENGTH_BUCKET_SIZE),
    }), [
      streamingToolCalls,
      totalChildCalls,
      totalChildToolResults,
      streamingTasks?.size,
      streamingTaskResultSignature,
      renderedStreamingContentBlocks.length,
      cumulativeTextLength,
    ]);

    // Build timeline data for Virtuoso. Always wraps messages as TimelineItem
    // for consistent typing. When hook events exist, they're interleaved and sorted.
    const hasHookEvents = hookEvents.length > 0 || activeHooks.length > 0;

    // Filter logic: during active streaming OR when conversation is finalizing (between
    // message_created clearing state and query refetch completing), exclude only the
    // provider snapshot for the current turn to prevent duplication with live content.
    //
    // isFinalizing is set to true (in the same React batch as clearing streaming state)
    // by useChatEvents on agent:message_created, and reset to false after 500ms. This
    // keeps the filter active through the timing window where streaming state is cleared
    // but the query refetch hasn't completed yet.
    //
    // Additionally, when isAgentRunning but no streaming content exists yet (the window
    // between DB empty-message creation and the first streaming event), filter the last
    // assistant message if its content is empty/whitespace — prevents the empty "pill" flash.
    const hasActiveStreaming = normalizedStreamingContentBlocks.length > 0 ||
                              Boolean(streamingTasks && streamingTasks.size > 0);
    const suppressedProviderMessageId = useMemo(
      () => getCurrentTurnProviderMessageId(messages, {
        hasActiveStreaming,
        isAgentRunning,
        isFinalizing,
      }),
      [hasActiveStreaming, isAgentRunning, isFinalizing, messages],
    );
    const shouldFilterCurrentProviderMessage = suppressedProviderMessageId !== null;

    const timeline = useMemo((): TimelineItem[] => {
      const items: TimelineItem[] = [];

      const suppressedProviderMessage = suppressedProviderMessageId
        ? messages.find((msg) => msg.id === suppressedProviderMessageId)
        : null;
      const suppressedTimelineParentId =
        suppressedProviderMessage?.timelineSequence != null
          ? suppressedProviderMessage.parentMessageId
          : null;
      const filteredMessages = suppressedProviderMessageId
        ? messages.filter((msg) => {
          if (msg.id === suppressedProviderMessageId) {
            return false;
          }
          return !(
            suppressedTimelineParentId &&
            msg.timelineSequence != null &&
            msg.parentMessageId === suppressedTimelineParentId
          );
        })
        : messages;

      // Team filter: each tab (lead/teammate) loads its own conversation's messages via
      // useConversation, so all messages in the data set belong to that conversation.
      // No per-message filtering needed — the conversation switch handles the scoping.
      const teamFilteredMessages = filteredMessages;

      const pushMessageItem = (
        msg: ChatMessageData,
        toolCallGroup?: ToolCallGroupMarker,
      ) => {
        // Enrich message with attachments if available
        const attachments = attachmentsMap?.get(msg.id);
        const enrichedMsg = attachments
          ? { ...msg, attachments }
          : msg;

        items.push({
          kind: "message",
          data: enrichedMsg,
          sortTime: new Date(msg.createdAt).getTime(),
          ...(toolCallGroup ? { toolCallGroup } : {}),
        });
      };

      for (let index = 0; index < teamFilteredMessages.length; index += 1) {
        const toolCallGroup = collectToolCallGroupRun(teamFilteredMessages, index);
        if (toolCallGroup) {
          const key = toolCallGroupKey(toolCallGroup);
          toolCallGroup.forEach((msg, groupIndex) => {
            pushMessageItem(msg, {
              key,
              count: toolCallGroup.length,
              position: groupIndex === 0 ? "toggle" : "covered",
            });
          });
          index += toolCallGroup.length - 1;
          continue;
        }

        const msg = teamFilteredMessages[index];
        if (msg) {
          pushMessageItem(msg);
        }
      }

      if (hasHookEvents) {
        for (const ev of hookEvents) {
          items.push({ kind: "hook", data: ev, sortTime: ev.timestamp });
        }
        for (const ev of activeHooks) {
          items.push({ kind: "hook", data: ev, sortTime: ev.timestamp });
        }
      }

      // Interleave team system messages (filtered by teammate tab)
      if (teamMessages.length > 0) {
        const filteredTeamMsgs = teamFilter
          ? teamMessages.filter((msg) => {
              if (teamFilter === "lead") {
                // Lead sees ALL team messages (lead is the orchestrator)
                return true;
              }
              return msg.from === teamFilter || msg.to === teamFilter || msg.to === "*";
            })
          : teamMessages;

        for (const msg of filteredTeamMsgs) {
          items.push({
            kind: "team_event",
            data: msg,
            sortTime: new Date(msg.timestamp).getTime(),
          });
        }
      }

      if (hasFooterStreamingContent) {
        items.push({
          kind: "streaming",
          sortTime: Number.MAX_SAFE_INTEGER,
        });
      }

      // Sort if we interleaved any non-message items
      if (hasHookEvents || teamMessages.length > 0 || hasFooterStreamingContent) {
        items.sort((a, b) => a.sortTime - b.sortTime);
      }

      return items;
    }, [messages, suppressedProviderMessageId, hookEvents, activeHooks, hasHookEvents, attachmentsMap, teamFilter, teamMessages, hasFooterStreamingContent]);

    const timelineSenderGroups = useMemo(() => {
      let previousGroupKey: string | null = null;
      return timeline.map((item) => {
        const groupKey = assistantSenderGroupKeyForTimelineItem(
          item,
          providerHarness,
          providerSessionId,
        );
        const isContinuation = groupKey !== null && groupKey === previousGroupKey;
        previousGroupKey = groupKey;
        return {
          reserveAssistantGutter: groupKey !== null,
          showSenderHeader: !isContinuation,
        };
      });
    }, [providerHarness, providerSessionId, timeline]);

    const lastVisibleTimelineIndex = useMemo(() => {
      for (let index = timeline.length - 1; index >= 0; index -= 1) {
        const item = timeline[index];
        if (item && isVisibleTimelineItem(item, expandedToolGroupKeys)) {
          return index;
        }
      }
      return -1;
    }, [expandedToolGroupKeys, timeline]);

    const streamingSenderGroupState =
      timeline[timeline.length - 1]?.kind === "streaming"
        ? timelineSenderGroups[timeline.length - 1] ?? DEFAULT_ASSISTANT_GROUP_STATE
        : DEFAULT_ASSISTANT_GROUP_STATE;

    const lastItemIndex = firstItemIndex + timeline.length - 1;

    // Unified auto-scroll hook — Virtuoso followOutput handles new-message scroll;
    // the true-bottom pinning paths below handle streaming row growth.
    const {
      messagesEndRef,
      isAtBottom,
      isAtBottomRef,
      scrollToBottom,
      handleAtBottomStateChange,
      handleFollowOutput,
    } = useChatAutoScroll({
      messageCount: timeline.length,
      disabled: !!scrollToTimestamp, // Disable auto-scroll in history mode
      virtuosoRef, // Route scrollToBottom through Virtuoso scrollToIndex
      indexOffset: firstItemIndex,
      conversationId, // Reset isAtBottom when conversation changes
    });

    useLayoutEffect(() => {
      const anchor = pendingToolGroupScrollAnchorRef.current;
      if (!anchor) {
        return;
      }
      pendingToolGroupScrollAnchorRef.current = null;

      const scroller = getToolGroupScrollContainer();
      /* c8 ignore next 3 -- the scroller can detach before the layout adjustment runs. */
      if (!scroller) {
        return;
      }

      const nextAnchorElement = findToolCallGroupToggleElement(scroller, anchor.groupKey);
      const nextAnchorTop = nextAnchorElement?.getBoundingClientRect().top ?? null;
      let nextScrollTop = scroller.scrollTop;

      if (anchor.wasVisuallyAtBottom) {
        nextScrollTop = scroller.scrollHeight - scroller.clientHeight;
      } else if (anchor.anchorTop !== null && nextAnchorTop !== null) {
        nextScrollTop = anchor.scrollTop + (nextAnchorTop - anchor.anchorTop);
      } else {
        nextScrollTop = scroller.scrollHeight - scroller.clientHeight - anchor.bottomDelta;
      }

      const clampedScrollTop = clampScrollTop(scroller, nextScrollTop);
      if (Math.abs(scroller.scrollTop - clampedScrollTop) > VISUAL_BOTTOM_EPSILON_PX) {
        scroller.scrollTop = clampedScrollTop;
      }

      const visuallyAtBottom = isScrollElementVisuallyAtBottom(scroller);
      if (visuallyAtBottom) {
        isUserScrollingAwayFromBottomRef.current = false;
      }
      lastObservedScrollTopRef.current = scroller.scrollTop;
      setIsVisuallyAtBottom(visuallyAtBottom);

      const atBottom = getScrollBottomDelta(scroller) < AT_BOTTOM_THRESHOLD;
      if (atBottom !== isAtBottomRef.current) {
        handleAtBottomStateChange(atBottom);
      }
    }, [
      expandedToolGroupKeys,
      getToolGroupScrollContainer,
      handleAtBottomStateChange,
      isAtBottomRef,
      setIsVisuallyAtBottom,
    ]);

    useEffect(() => {
      scrollToTimestampRef.current = scrollToTimestamp;
    }, [scrollToTimestamp]);

    const isLastItemActuallyVisible = useCallback(() => {
      if (isLastItemVisibleRef.current === false) {
        return false;
      }

      const row = lastRenderedRowElRef.current;
      const scroller = scrollerElRef.current;
      if (!row || !scroller) {
        return true;
      }

      const rowRect = row.getBoundingClientRect();
      const scrollerRect = scroller.getBoundingClientRect();
      const hasUsableRowRect =
        rowRect.height > 0 || rowRect.top !== 0 || rowRect.bottom !== 0;
      const hasUsableScrollerRect =
        scrollerRect.height > 0 || scrollerRect.top !== 0 || scrollerRect.bottom !== 0;
      if (!hasUsableRowRect || !hasUsableScrollerRect) {
        return true;
      }

      return (
        rowRect.bottom > scrollerRect.top + VISUAL_BOTTOM_EPSILON_PX &&
        rowRect.top < scrollerRect.bottom - VISUAL_BOTTOM_EPSILON_PX
      );
    }, []);

    const shouldKeepBottomPinned = useCallback(
      (
        activeScrollToTimestamp: string | null | undefined = scrollToTimestampRef.current,
        { requireLastItemVisible = true }: { requireLastItemVisible?: boolean } = {},
      ) => {
        if (isUserScrollingAwayFromBottomRef.current) {
          return false;
        }
        if (requireLastItemVisible && !isLastItemActuallyVisible()) {
          return false;
        }
        if (!hasUserScrollInputRef.current) {
          return !activeScrollToTimestamp;
        }

        return shouldStickToBottom({
          scrollToTimestamp: activeScrollToTimestamp,
          isAtBottom: isAtBottomRef.current,
          isVisuallyAtBottom: isVisuallyAtBottomRef.current,
        });
      },
      [isAtBottomRef, isLastItemActuallyVisible],
    );

    const handleGuardedFollowOutput = useCallback(
      (atBottom: boolean) => {
        if (!atBottom) {
          return false as const;
        }
        if (
          scrollToTimestampRef.current ||
          isUserScrollingAwayFromBottomRef.current ||
          !isLastItemActuallyVisible()
        ) {
          return false as const;
        }
        return handleFollowOutput(atBottom);
      },
      [handleFollowOutput, isLastItemActuallyVisible],
    );

    // Window advancement follows the same bottom-range contract as chat auto-scroll.
    // Exact visual-bottom tracking can drift false during Virtuoso/footer growth even
    // while the user is still close enough to the tail to be following the live run.
    useEffect(() => {
      setStreamingTranscriptWindow((prev) => {
        return getNextStreamingTranscriptWindow(
          prev,
          liveStreamingTranscriptWindow,
          isAtBottom,
        );
      });
    }, [isAtBottom, liveStreamingTranscriptWindow]);

    // Scroll the actual DOM scroll container to its absolute bottom.
    // This goes past Virtuoso's last list item to include any Footer (streaming
    // indicators) + bottom padding — unlike scrollToIndex which only aligns the
    // last row to the viewport edge and leaves 20-50px of footer/padding below.
    const scrollToTrueBottom = useCallback(
      (behavior: ScrollBehavior = "smooth") => {
        const el = scrollerElRef.current;
        if (!el) {
          logger.debug("[ChatScroll] scrollToTrueBottom: no scroller ref yet, falling back to scrollToBottom hook");
          scrollToBottom();
          isUserScrollingAwayFromBottomRef.current = false;
          setIsVisuallyAtBottom(true);
          return;
        }
        const target = scrollElementToTrueBottom(el, behavior);
        isUserScrollingAwayFromBottomRef.current = false;
        logger.debug("[ChatScroll] scrollToTrueBottom", {
          scrollHeight: el.scrollHeight,
          clientHeight: el.clientHeight,
          currentTop: el.scrollTop,
          target,
          behavior,
        });
        setIsVisuallyAtBottom(true);
        // Eagerly mark atBottom=true so followOutput re-engages without waiting
        // for scrollend.
        if (!isAtBottomRef.current) {
          handleAtBottomStateChange(true);
        }
      },
      [scrollToBottom, setIsVisuallyAtBottom, handleAtBottomStateChange, isAtBottomRef]
    );

    const canRunScheduledBottomPin = useCallback(
      (
        scheduledAwayVersion: number,
        { requireLastItemVisible = true }: { requireLastItemVisible?: boolean } = {},
      ) => {
        if (userScrollAwayVersionRef.current !== scheduledAwayVersion) {
          return false;
        }
        if (requireLastItemVisible && !isLastItemActuallyVisible()) {
          return false;
        }
        return true;
      },
      [isLastItemActuallyVisible],
    );

    // After any layout-changing event that should land at bottom, run two
    // passes — first on next frame (catches most cases), second after a short
    // delay (catches late-arriving streaming footer height growth).
    const scheduleBottomPin = useCallback(
      (
        reason: string,
        behavior: ScrollBehavior = preferredScrollBehavior,
        { requireLastItemVisible = true }: { requireLastItemVisible?: boolean } = {},
      ) => {
        logger.debug(`[ChatScroll] scheduleBottomPin: ${reason}`);
        const scheduledAwayVersion = userScrollAwayVersionRef.current;
        for (const rafId of bottomPinRafIdsRef.current) {
          cancelAnimationFrame(rafId);
        }
        bottomPinRafIdsRef.current = [];
        if (bottomPinTimeoutRef.current) {
          clearTimeout(bottomPinTimeoutRef.current);
          bottomPinTimeoutRef.current = null;
        }

        const outerRafId = requestAnimationFrame(() => {
          bottomPinRafIdsRef.current = bottomPinRafIdsRef.current.filter((id) => id !== outerRafId);

          const innerRafId = requestAnimationFrame(() => {
            bottomPinRafIdsRef.current = bottomPinRafIdsRef.current.filter((id) => id !== innerRafId);
            if (!canRunScheduledBottomPin(scheduledAwayVersion, { requireLastItemVisible })) {
              return;
            }
            scrollToTrueBottom(behavior);
            // Second pass catches footer that grows in the same tick.
            bottomPinTimeoutRef.current = setTimeout(() => {
              bottomPinTimeoutRef.current = null;
              if (!canRunScheduledBottomPin(scheduledAwayVersion, { requireLastItemVisible })) {
                return;
              }
              scrollToTrueBottom(behavior);
            }, 120);
          });

          bottomPinRafIdsRef.current.push(innerRafId);
        });

        bottomPinRafIdsRef.current.push(outerRafId);
      },
      [canRunScheduledBottomPin, preferredScrollBehavior, scrollToTrueBottom]
    );

    const isToolGroupScrollAdjustmentActive = useCallback(() => {
      const until = toolGroupScrollAdjustmentUntilRef.current;
      if (until === null) {
        return false;
      }
      if (performance.now() <= until) {
        return true;
      }
      toolGroupScrollAdjustmentUntilRef.current = null;
      return false;
    }, []);

    const scheduleStickyResizeBottomPin = useCallback(
      (
        rafRef: React.MutableRefObject<number | null>,
        shouldRun?: () => boolean,
      ) => {
        if (isToolGroupScrollAdjustmentActive()) {
          return;
        }
        if (rafRef.current !== null) {
          cancelAnimationFrame(rafRef.current);
        }
        rafRef.current = requestAnimationFrame(() => {
          rafRef.current = null;
          if ((shouldRun?.() ?? true) && shouldKeepBottomPinned()) {
            scrollToTrueBottom("auto");
          }
        });
      },
      [isToolGroupScrollAdjustmentActive, scrollToTrueBottom, shouldKeepBottomPinned],
    );

    // Streaming auto-scroll — followOutput only fires on totalCount changes,
    // NOT on Footer height growth. Pin to the true DOM bottom while the user is
    // still inside the sticky bottom zone so footer/meta growth is included.
    useEffect(() => {
      if (scrollToTimestamp || !hasFooterStreamingContent) return;
      if (shouldKeepBottomPinned(scrollToTimestamp)) {
        scrollToTrueBottom("auto");
      }
    }, [
      footerContentHash,
      hasFooterStreamingContent,
      scrollToTimestamp,
      scrollToTrueBottom,
      shouldKeepBottomPinned,
    ]);

    useEffect(() => {
      return () => {
        for (const rafId of bottomPinRafIdsRef.current) {
          cancelAnimationFrame(rafId);
        }
        bottomPinRafIdsRef.current = [];
        if (bottomPinTimeoutRef.current) {
          clearTimeout(bottomPinTimeoutRef.current);
          bottomPinTimeoutRef.current = null;
        }
        if (totalListHeightRafRef.current !== null) {
          cancelAnimationFrame(totalListHeightRafRef.current);
          totalListHeightRafRef.current = null;
        }
        if (virtuosoAtBottomSettleRafRef.current !== null) {
          cancelAnimationFrame(virtuosoAtBottomSettleRafRef.current);
          virtuosoAtBottomSettleRafRef.current = null;
        }
      };
    }, []);

    useEffect(() => {
      for (const rafId of bottomPinRafIdsRef.current) {
        cancelAnimationFrame(rafId);
      }
      bottomPinRafIdsRef.current = [];
      if (bottomPinTimeoutRef.current) {
        clearTimeout(bottomPinTimeoutRef.current);
        bottomPinTimeoutRef.current = null;
      }
      if (virtuosoAtBottomSettleRafRef.current !== null) {
        cancelAnimationFrame(virtuosoAtBottomSettleRafRef.current);
        virtuosoAtBottomSettleRafRef.current = null;
      }
      setIsVisuallyAtBottom(true);
      setHasScrollableOverflow(false);
      isLastItemVisibleRef.current = true;
      setIsLastItemVisible(true);
      lastObservedScrollTopRef.current = null;
      bottomScrollIntentUntilRef.current = null;
      isUserScrollingAwayFromBottomRef.current = false;
      hasUserScrollInputRef.current = false;
      userScrollAwayVersionRef.current = 0;
      bottomSettleAttemptCountRef.current = 0;
      previousTotalListHeightRef.current = -1;
      previousLastItemIndexRef.current = null;
      lastUserMessageIdRef.current = conversationLastUserMessageIdRef.current;
      agentRunningRef.current = conversationAgentRunningRef.current;
    }, [conversationId, setIsVisuallyAtBottom]);

    // Trigger 1: new user message appended → always jump to true bottom.
    useEffect(() => {
      if (!lastUserMessageId) {
        lastUserMessageIdRef.current = null;
        return;
      }
      if (lastUserMessageIdRef.current === lastUserMessageId) return;
      lastUserMessageIdRef.current = lastUserMessageId;
      scheduleBottomPin(`new user message id=${lastUserMessageId}`);
    }, [lastUserMessageId, scheduleBottomPin]);

    // Trigger 2: streaming starts (transition false → true). User just-sent a
    // message expects the agent's first tokens to appear at bottom of viewport.
    useEffect(() => {
      if (isAgentRunning && !agentRunningRef.current) {
        scheduleBottomPin("streaming started");
      }
      agentRunningRef.current = isAgentRunning;
    }, [isAgentRunning, scheduleBottomPin]);

    const hasRecentBottomScrollIntent = useCallback(
      () =>
        bottomScrollIntentUntilRef.current !== null &&
        performance.now() <= bottomScrollIntentUntilRef.current,
      [],
    );

    const canAttemptTrueBottomSettle = useCallback(
      () =>
        !scrollToTimestampRef.current &&
        !isUserScrollingAwayFromBottomRef.current &&
        isLastItemActuallyVisible() &&
        hasRecentBottomScrollIntent() &&
        bottomSettleAttemptCountRef.current < MAX_TRUE_BOTTOM_SETTLE_ATTEMPTS,
      [hasRecentBottomScrollIntent, isLastItemActuallyVisible],
    );

    const recordTrueBottomSettleAttempt = useCallback(() => {
      bottomSettleAttemptCountRef.current += 1;
    }, []);

    const markUserScrollingAwayFromBottom = useCallback(() => {
      isUserScrollingAwayFromBottomRef.current = true;
      userScrollAwayVersionRef.current += 1;
    }, []);

    const shouldRecoverScrollDriftToBottom = useCallback(
      (bottomDelta: number, visuallyAtBottom: boolean) => {
        if (
          scrollToTimestampRef.current ||
          visuallyAtBottom ||
          bottomDelta < AT_BOTTOM_THRESHOLD ||
          isUserScrollingAwayFromBottomRef.current
        ) {
          return false;
        }

        if (!hasUserScrollInputRef.current) {
          return isAtBottomRef.current || isVisuallyAtBottomRef.current;
        }

        return false;
      },
      [isAtBottomRef],
    );

    // rAF-throttled DOM reconciliation — keeps isAtBottom accurate when Virtuoso doesn't detect footer growth.
    // Runs outside React render cycle (DOM event handler, not useEffect) — no render loop risk.
    // rAF fires post-paint, so scrollHeight reads don't force layout recalc during React commit phase.
    const reconcileScrollerBottomState = useCallback(() => {
      const el = scrollerElRef.current;
      if (!el) return;

      const bottomDelta = getScrollBottomDelta(el);
      const atBottom = bottomDelta < AT_BOTTOM_THRESHOLD;
      const visuallyAtBottom = bottomDelta <= VISUAL_BOTTOM_EPSILON_PX;
      const previousScrollTop = lastObservedScrollTopRef.current;
      const isScrollingTowardBottom =
        previousScrollTop === null || el.scrollTop >= previousScrollTop;
      if (visuallyAtBottom) {
        isUserScrollingAwayFromBottomRef.current = false;
      } else if (
        shouldTreatScrollTopDecreaseAsUserAway({
          hasUserScrollInput: hasUserScrollInputRef.current,
          previousScrollTop,
          currentScrollTop: el.scrollTop,
          isVisuallyAtBottom: visuallyAtBottom,
        })
      ) {
        markUserScrollingAwayFromBottom();
      }
      lastObservedScrollTopRef.current = el.scrollTop;
      setHasScrollableOverflow(
        el.scrollHeight > el.clientHeight + VISUAL_BOTTOM_EPSILON_PX
      );
      if (shouldRecoverScrollDriftToBottom(bottomDelta, visuallyAtBottom)) {
        scrollToTrueBottom("auto");
        return;
      }
      if (
        canAttemptTrueBottomSettle() &&
        atBottom &&
        isScrollingTowardBottom &&
        bottomDelta > VISUAL_BOTTOM_EPSILON_PX &&
        bottomDelta <= TRUE_BOTTOM_SETTLE_THRESHOLD_PX
      ) {
        recordTrueBottomSettleAttempt();
        scrollToTrueBottom("auto");
        return;
      }
      setIsVisuallyAtBottom(visuallyAtBottom);

      // Only reconcile if state disagrees — avoids unnecessary setState
      if (atBottom !== isAtBottomRef.current) {
        handleAtBottomStateChange(atBottom);
      }
    }, [
      canAttemptTrueBottomSettle,
      handleAtBottomStateChange,
      isAtBottomRef,
      markUserScrollingAwayFromBottom,
      recordTrueBottomSettleAttempt,
      shouldRecoverScrollDriftToBottom,
      scrollToTrueBottom,
      setIsVisuallyAtBottom,
    ]);

    const markBottomScrollIntent = useCallback(() => {
      bottomScrollIntentUntilRef.current =
        performance.now() + BOTTOM_SCROLL_INTENT_WINDOW_MS;
      isUserScrollingAwayFromBottomRef.current = false;
      bottomSettleAttemptCountRef.current = 0;
    }, []);

    const markScrollerDirectionFromCurrentPosition = useCallback(() => {
      const el = scrollerElRef.current;
      if (!el) {
        return;
      }

      const previousScrollTop = lastObservedScrollTopRef.current;
      const visuallyAtBottom = isScrollElementVisuallyAtBottom(el);
      if (visuallyAtBottom) {
        isUserScrollingAwayFromBottomRef.current = false;
        setIsVisuallyAtBottom(true);
        if (!isAtBottomRef.current) {
          handleAtBottomStateChange(true);
        }
        return;
      }
      if (
        shouldTreatScrollTopDecreaseAsUserAway({
          hasUserScrollInput: hasUserScrollInputRef.current,
          previousScrollTop,
          currentScrollTop: el.scrollTop,
          isVisuallyAtBottom: visuallyAtBottom,
        })
      ) {
        markUserScrollingAwayFromBottom();
        return;
      }
    }, [
      handleAtBottomStateChange,
      isAtBottomRef,
      markUserScrollingAwayFromBottom,
      setIsVisuallyAtBottom,
    ]);

    const markManualWheelScroll = useCallback(
      (deltaY: number, el: HTMLElement | null) => {
        hasUserScrollInputRef.current = true;
        if (deltaY < 0 || (deltaY > 0 && (!el || !isScrollElementVisuallyAtBottom(el)))) {
          markUserScrollingAwayFromBottom();
        }
      },
      [markUserScrollingAwayFromBottom],
    );

    const handleScrollerWheel = useCallback(
      (event: WheelEvent) => {
        markManualWheelScroll(event.deltaY, scrollerElRef.current);
      },
      [markManualWheelScroll],
    );

    const handleScrollerPointerDown = useCallback(
      (event: PointerEvent) => {
        hasUserScrollInputRef.current = true;
        const target = event.currentTarget;
        if (!(target instanceof HTMLElement)) {
          return;
        }

        const rect = target.getBoundingClientRect();
        if (event.clientX >= rect.right - 20) {
          markBottomScrollIntent();
        }
      },
      [markBottomScrollIntent],
    );

    const handleScrollReconcile = useCallback(() => {
      markScrollerDirectionFromCurrentPosition();
      if (reconcileRafRef.current) return; // Already scheduled — skip
      reconcileRafRef.current = requestAnimationFrame(() => {
        reconcileRafRef.current = null;
        reconcileScrollerBottomState();
      });
    }, [markScrollerDirectionFromCurrentPosition, reconcileScrollerBottomState]);

    const scheduleVirtuosoAtBottomSettle = useCallback(() => {
      if (virtuosoAtBottomSettleRafRef.current !== null) {
        cancelAnimationFrame(virtuosoAtBottomSettleRafRef.current);
      }

      virtuosoAtBottomSettleRafRef.current = requestAnimationFrame(() => {
        virtuosoAtBottomSettleRafRef.current = null;
        const el = scrollerElRef.current;
        if (!el || !canAttemptTrueBottomSettle()) {
          return;
        }

        if (getScrollBottomDelta(el) > VISUAL_BOTTOM_EPSILON_PX) {
          recordTrueBottomSettleAttempt();
          scrollToTrueBottom("auto");
        }
      });
    }, [canAttemptTrueBottomSettle, recordTrueBottomSettleAttempt, scrollToTrueBottom]);

    const handleVirtuosoAtBottomStateChange = useCallback(
      (atBottom: boolean) => {
        const el = scrollerElRef.current;
        const visuallyAtBottom =
          atBottom && el ? isScrollElementVisuallyAtBottom(el) : atBottom;
        if (
          !atBottom &&
          el &&
          shouldRecoverScrollDriftToBottom(getScrollBottomDelta(el), false)
        ) {
          scrollToTrueBottom("auto");
          return;
        }
        setIsVisuallyAtBottom(visuallyAtBottom);
        handleAtBottomStateChange(atBottom);
        if (!atBottom) {
          return;
        }
        if (el && !visuallyAtBottom && canAttemptTrueBottomSettle()) {
          scheduleVirtuosoAtBottomSettle();
        }
      },
      [
        canAttemptTrueBottomSettle,
        handleAtBottomStateChange,
        scheduleVirtuosoAtBottomSettle,
        scrollToTrueBottom,
        shouldRecoverScrollDriftToBottom,
        setIsVisuallyAtBottom,
      ],
    );

    useEffect(() => {
      if (scrollToTimestamp || !isAtBottom) {
        return;
      }
      if (isVisuallyAtBottom) {
        return;
      }
      if (virtuosoAtBottomSettleRafRef.current !== null) {
        return;
      }
      if (!canAttemptTrueBottomSettle()) {
        return;
      }

      scheduleVirtuosoAtBottomSettle();
    }, [
      canAttemptTrueBottomSettle,
      isAtBottom,
      isVisuallyAtBottom,
      scheduleVirtuosoAtBottomSettle,
      scrollToTimestamp,
    ]);

    const handleScrollerResize = useCallback(() => {
      if (scrollerResizeRafRef.current !== null) {
        cancelAnimationFrame(scrollerResizeRafRef.current);
      }
      scrollerResizeRafRef.current = requestAnimationFrame(() => {
        scrollerResizeRafRef.current = null;
        if (shouldKeepBottomPinned()) {
          scrollToTrueBottom("auto");
          return;
        }
        reconcileScrollerBottomState();
      });
    }, [reconcileScrollerBottomState, scrollToTrueBottom, shouldKeepBottomPinned]);

    useEffect(() => {
      if (externalLayoutVersion <= 0) {
        return;
      }
      if (
        shouldKeepBottomPinned(scrollToTimestampRef.current, {
          requireLastItemVisible: false,
        })
      ) {
        scheduleBottomPin("external layout changed", "auto", {
          requireLastItemVisible: false,
        });
        return;
      }
      handleScrollerResize();
    }, [
      externalLayoutVersion,
      handleScrollerResize,
      scheduleBottomPin,
      shouldKeepBottomPinned,
    ]);

    const handleTotalListHeightChanged = useCallback(
      (height: number) => {
        const previousHeight = previousTotalListHeightRef.current;
        previousTotalListHeightRef.current = height;

        if (previousHeight < 0) {
          const el = scrollerElRef.current;
          if (el && getScrollBottomDelta(el) > VISUAL_BOTTOM_EPSILON_PX) {
            scheduleStickyResizeBottomPin(totalListHeightRafRef);
          }
          return;
        }

        if (height <= previousHeight + VISUAL_BOTTOM_EPSILON_PX) {
          return;
        }

        scheduleStickyResizeBottomPin(totalListHeightRafRef);
      },
      [scheduleStickyResizeBottomPin],
    );

    const disconnectScrollerResizeObserver = useCallback(() => {
      scrollerResizeObserverRef.current?.disconnect();
      scrollerResizeObserverRef.current = null;
      if (scrollerResizeRafRef.current !== null) {
        cancelAnimationFrame(scrollerResizeRafRef.current);
        scrollerResizeRafRef.current = null;
      }
    }, []);

    // Attach passive scroll listener to Virtuoso's scroller element.
    // Passed to Virtuoso's scrollerRef prop so we capture the actual scroll container.
    const handleScrollerRef = useCallback((el: Window | HTMLElement | null) => {
      if (!(el instanceof HTMLElement)) {
        if (scrollerElRef.current) {
          scrollerElRef.current.removeEventListener("scroll", handleScrollReconcile);
          scrollerElRef.current.removeEventListener("wheel", handleScrollerWheel);
          scrollerElRef.current.removeEventListener("pointerdown", handleScrollerPointerDown);
          scrollerElRef.current = null;
        }
        lastObservedScrollTopRef.current = null;
        setHasScrollerElement(false);
        setHasScrollableOverflow(false);
        disconnectScrollerResizeObserver();
        return;
      }
      if (scrollerElRef.current && scrollerElRef.current !== el) {
        scrollerElRef.current.removeEventListener("scroll", handleScrollReconcile);
        scrollerElRef.current.removeEventListener("wheel", handleScrollerWheel);
        scrollerElRef.current.removeEventListener("pointerdown", handleScrollerPointerDown);
        disconnectScrollerResizeObserver();
      }
      if (scrollerElRef.current === el) {
        reconcileScrollerBottomState();
        return;
      }
      scrollerElRef.current = el;
      lastObservedScrollTopRef.current = el.scrollTop;
      setHasScrollerElement(true);
      el.addEventListener("scroll", handleScrollReconcile, { passive: true });
      el.addEventListener("wheel", handleScrollerWheel, { passive: true });
      el.addEventListener("pointerdown", handleScrollerPointerDown, { passive: true });
      reconcileScrollerBottomState();
      if (typeof ResizeObserver !== "undefined") {
        const observer = new ResizeObserver(handleScrollerResize);
        observer.observe(el);
        scrollerResizeObserverRef.current = observer;
      }
    }, [
      disconnectScrollerResizeObserver,
      handleScrollerPointerDown,
      handleScrollerWheel,
      handleScrollReconcile,
      handleScrollerResize,
      reconcileScrollerBottomState,
    ]);

    // Cleanup rAF and scroll listener on unmount
    useEffect(() => {
      return () => {
        if (reconcileRafRef.current) cancelAnimationFrame(reconcileRafRef.current);
        scrollerElRef.current?.removeEventListener("scroll", handleScrollReconcile);
        disconnectScrollerResizeObserver();
      };
    }, [disconnectScrollerResizeObserver, handleScrollReconcile]);

    // Stable callback ref for Footer element — creates ResizeObserver that detects footer height
    // changes (G2 fix: card expansion during streaming). Empty deps ensures observer is never
    // torn down due to prop changes.
    //
    // H1 analysis: Late tool results (after turn_completed) update finalized messages in the
    // timeline, not the footer. Virtuoso's followOutput handles timeline height changes natively.
    // The footer ResizeObserver only needs to cover the active streaming window, not post-stream updates.
    const handleFooterRef = useCallback((el: HTMLDivElement | null) => {
      // Cleanup old observer
      if (footerObserverRef.current) {
        footerObserverRef.current.disconnect();
        footerObserverRef.current = null;
      }
      footerElRef.current = el;
      footerMountedRef.current = false; // Reset mount flag on new element
      if (!el) return;

      footerObserverRef.current = new ResizeObserver((entries) => {
        const newHeight = entries[0]?.contentRect.height ?? 0;

        // H2 fix: Skip the very first observation after mount.
        // The first observation captures baseline height without triggering scroll.
        // Prevents jarring scroll jump when switching chat tabs or loading history.
        if (!footerMountedRef.current) {
          footerMountedRef.current = true;
          footerPrevHeightRef.current = newHeight;
          return;
        }

        // Only react to height increases, not width changes or shrinking
        if (newHeight <= footerPrevHeightRef.current) {
          footerPrevHeightRef.current = newHeight;
          return;
        }
        footerPrevHeightRef.current = newHeight;

        scheduleStickyResizeBottomPin(
          footerResizeRafRef,
          () => hasFooterStreamingContentRef.current,
        );
      });
      footerObserverRef.current.observe(el);
    }, [scheduleStickyResizeBottomPin]);

    // Cleanup Footer ResizeObserver and rAF on unmount
    useEffect(() => {
      return () => {
        footerObserverRef.current?.disconnect();
        footerObserverRef.current = null;
        if (footerResizeRafRef.current) {
          cancelAnimationFrame(footerResizeRafRef.current);
          footerResizeRafRef.current = null;
        }
      };
    }, []);

    const handleLastRenderedRowRef = useCallback((el: HTMLDivElement | null) => {
      if (lastRenderedRowObserverRef.current) {
        lastRenderedRowObserverRef.current.disconnect();
        lastRenderedRowObserverRef.current = null;
      }
      lastRenderedRowElRef.current = el;
      if (lastRenderedRowResizeRafRef.current !== null) {
        cancelAnimationFrame(lastRenderedRowResizeRafRef.current);
        lastRenderedRowResizeRafRef.current = null;
      }
      lastRenderedRowMountedRef.current = false;
      lastRenderedRowPrevHeightRef.current = -1;

      if (!el || typeof ResizeObserver === "undefined") {
        return;
      }

      lastRenderedRowObserverRef.current = new ResizeObserver((entries) => {
        const newHeight = entries[0]?.contentRect.height ?? 0;

        if (!lastRenderedRowMountedRef.current) {
          lastRenderedRowMountedRef.current = true;
          lastRenderedRowPrevHeightRef.current = newHeight;
          return;
        }

        if (newHeight <= lastRenderedRowPrevHeightRef.current) {
          lastRenderedRowPrevHeightRef.current = newHeight;
          return;
        }
        lastRenderedRowPrevHeightRef.current = newHeight;

        scheduleStickyResizeBottomPin(lastRenderedRowResizeRafRef);
      });
      lastRenderedRowObserverRef.current.observe(el);
    }, [scheduleStickyResizeBottomPin]);

    useEffect(() => {
      const root = transcriptRootRef.current;
      if (!root || typeof ResizeObserver === "undefined") {
        return undefined;
      }

      transcriptRootMountedRef.current = false;
      transcriptRootPrevHeightRef.current = -1;

      const observer = new ResizeObserver((entries) => {
        const newHeight = entries[0]?.contentRect.height ?? 0;

        if (!transcriptRootMountedRef.current) {
          transcriptRootMountedRef.current = true;
          transcriptRootPrevHeightRef.current = newHeight;
          return;
        }

        if (newHeight === transcriptRootPrevHeightRef.current) {
          return;
        }
        transcriptRootPrevHeightRef.current = newHeight;

        scheduleStickyResizeBottomPin(transcriptRootResizeRafRef);
      });
      observer.observe(root);
      transcriptRootResizeObserverRef.current = observer;

      return () => {
        observer.disconnect();
        if (transcriptRootResizeObserverRef.current === observer) {
          transcriptRootResizeObserverRef.current = null;
        }
        if (transcriptRootResizeRafRef.current !== null) {
          cancelAnimationFrame(transcriptRootResizeRafRef.current);
          transcriptRootResizeRafRef.current = null;
        }
        transcriptRootMountedRef.current = false;
        transcriptRootPrevHeightRef.current = -1;
      };
    }, [scheduleStickyResizeBottomPin]);

    // Scroll to specific timestamp for history mode (time-travel feature)
    // Finds the first message at or after the given timestamp and scrolls to it
    useEffect(() => {
      if (!scrollToTimestamp || messages.length === 0) return;

      const targetTime = new Date(scrollToTimestamp).getTime();
      const targetIndex = messages.findIndex(
        (msg) => new Date(msg.createdAt).getTime() >= targetTime
      );

      if (targetIndex >= 0) {
        // Add a small delay to ensure Virtuoso is ready
        const timeoutId = setTimeout(() => {
          virtuosoRef.current?.scrollToIndex({
            index: firstItemIndex + targetIndex,
            align: "start",
            behavior: preferredScrollBehavior,
          });
        }, MARKDOWN_RENDER_DELAY_MS);
        return () => clearTimeout(timeoutId);
      }
      return undefined;
    }, [scrollToTimestamp, messages, firstItemIndex, preferredScrollBehavior]);

    // When filter clears (streaming/finalizing ends), scroll to bottom so the newly
    // revealed finalized assistant message is visible.
    useEffect(() => {
      if (scrollToTimestamp) return; // Don't auto-scroll in history mode
      if (prevShouldFilterRef.current && !shouldFilterCurrentProviderMessage) {
        scheduleBottomPin("finalized provider message revealed");
      }
      prevShouldFilterRef.current = shouldFilterCurrentProviderMessage;
    }, [scheduleBottomPin, shouldFilterCurrentProviderMessage, scrollToTimestamp]);

    const startReachedHandler =
      hasOlderMessages && onLoadOlderMessages
        ? (_index: number) => {
            void onLoadOlderMessages();
          }
        : null;
    const shouldShowScrollToBottom = shouldShowScrollToBottomControl({
      hasScrollerElement,
      hasScrollableOverflow,
      isAtBottom,
      isLastItemVisible,
      isVisuallyAtBottom,
      scrollToTimestamp,
      timelineLength: timeline.length,
    });
    const handleScrollToBottomClick = useCallback(() => {
      markBottomScrollIntent();
      scrollToTrueBottom(preferredScrollBehavior);
      scheduleBottomPin("manual scroll-to-bottom", preferredScrollBehavior);
    }, [markBottomScrollIntent, preferredScrollBehavior, scheduleBottomPin, scrollToTrueBottom]);
    const handleScrollToBottomWheel = useCallback(
      (event: React.WheelEvent<HTMLButtonElement>) => {
        if (!shouldShowScrollToBottom) {
          return;
        }
        const el = scrollerElRef.current ?? (isTestEnv ? transcriptRootRef.current : null);
        if (!el) {
          return;
        }

        event.preventDefault();
        markManualWheelScroll(event.deltaY, el);
        scrollElementByDelta(el, event.deltaX, event.deltaY);
        handleScrollReconcile();
      },
      [handleScrollReconcile, isTestEnv, markManualWheelScroll, shouldShowScrollToBottom],
    );

    const handleRangeChanged = useCallback(
      (range: ListRange) => {
        if (timeline.length > 0 && range.endIndex >= range.startIndex) {
          const nextIsLastItemVisible = range.endIndex >= lastItemIndex;
          isLastItemVisibleRef.current = nextIsLastItemVisible;
          setIsLastItemVisible(nextIsLastItemVisible);
          scheduleInitialPaintReadyCheck();
        }
      },
      [lastItemIndex, scheduleInitialPaintReadyCheck, timeline.length],
    );

    useEffect(() => {
      if (!shouldShowInitialPaintCover) {
        return;
      }
      scheduleInitialPaintReadyCheck();
    }, [scheduleInitialPaintReadyCheck, shouldShowInitialPaintCover]);

    // Initial load scroll — fires when conversation changes and timeline populates.
    // Uses one-shot ResizeObserver on the scroller element to detect when virtual
    // content has actually rendered, rather than a fixed-duration setTimeout guess.
    // Falls back to MARKDOWN_RENDER_DELAY_MS if scrollerElRef not yet available.
    useEffect(() => {
      const targetScrollKey =
        conversationId != null && lastItemIndex >= 0
          ? `${conversationId}:${lastItemIndex}`
          : null;

      if (!conversationId || timeline.length === 0 || hasScrolledRef.current === targetScrollKey) {
        return;
      }

      const verifyTimers: ReturnType<typeof setTimeout>[] = [];
      const scheduledAwayVersion = userScrollAwayVersionRef.current;

      const doScroll = () => {
        if (hasScrolledRef.current === targetScrollKey) return;
        if (
          !canRunScheduledBottomPin(scheduledAwayVersion, {
            requireLastItemVisible: false,
          })
        ) {
          return;
        }
        virtuosoRef.current?.scrollToIndex({
          index: lastItemIndex,
          align: "end",
          behavior: "auto",
        });
        scheduleBottomPin("initial conversation load", "auto", {
          requireLastItemVisible: false,
        });
        hasScrolledRef.current = targetScrollKey;

        // Content (markdown, code blocks, tool results) can keep rendering after
        // the initial scroll. Verify we're actually at bottom and retry if not.
        const verifyAtBottom = () => {
          if (
            !canRunScheduledBottomPin(scheduledAwayVersion, {
              requireLastItemVisible: false,
            })
          ) {
            return;
          }
          const el = scrollerElRef.current;
          if (!el) return;
          const delta = el.scrollHeight - el.clientHeight - el.scrollTop;
          if (delta > VISUAL_BOTTOM_EPSILON_PX) {
            scrollToTrueBottom("auto");
          }
        };
        verifyTimers.push(
          setTimeout(verifyAtBottom, 500),
          setTimeout(verifyAtBottom, 1000),
        );
      };

      const scroller = scrollerElRef.current;
      if (!scroller) {
        // Fallback: scroller not yet mounted, use fixed delay
        const timer = setTimeout(doScroll, MARKDOWN_RENDER_DELAY_MS);
        return () => {
          clearTimeout(timer);
          verifyTimers.forEach(clearTimeout);
        };
      }

      let debounceTimer: ReturnType<typeof setTimeout>;
      const observer = new ResizeObserver(() => {
        clearTimeout(debounceTimer);
        debounceTimer = setTimeout(() => {
          doScroll();
          observer.disconnect();
        }, 200);
      });

      observer.observe(scroller);

      // ResizeObserver can miss the initial virtual-list settlement when the
      // scroller element itself does not resize. Keep the old settled-resize
      // path, but also run the first bottom pin on the normal markdown delay.
      const fallbackTimer = setTimeout(doScroll, MARKDOWN_RENDER_DELAY_MS);

      // Safety timeout: 3s max — disconnect + force scroll if debounce never settles
      const safetyTimer = setTimeout(() => {
        observer.disconnect();
        doScroll();
      }, 3000);

      return () => {
        observer.disconnect();
        clearTimeout(debounceTimer);
        clearTimeout(fallbackTimer);
        clearTimeout(safetyTimer);
        verifyTimers.forEach(clearTimeout);
      };
    }, [
      canRunScheduledBottomPin,
      conversationId,
      lastItemIndex,
      scheduleBottomPin,
      scrollToTrueBottom,
      timeline.length,
    ]);

    useEffect(() => {
      const previousLastItemIndex = previousLastItemIndexRef.current;
      previousLastItemIndexRef.current = lastItemIndex;

      if (
        scrollToTimestamp ||
        timeline.length === 0 ||
        previousLastItemIndex === null ||
        lastItemIndex <= previousLastItemIndex
      ) {
        return;
      }

      if (shouldKeepBottomPinned(scrollToTimestamp)) {
        scheduleBottomPin("new timeline item appended");
      }
    }, [lastItemIndex, scheduleBottomPin, scrollToTimestamp, shouldKeepBottomPinned, timeline.length]);

    const footerContent = useMemo(() => {
      if (!hasFooterStreamingContent) {
        return null;
      }
      if (!shouldRenderStreamingContentGroup && !shouldShowActiveTypingIndicator) {
        return null;
      }
      const renderStreamingToolCallBlock = (
        block: StreamingToolUseBlock,
        idx: number,
      ) => {
        if (isDiffToolCall(block.toolCall.name) && block.toolCall.arguments != null) {
          return (
            <DiffToolCallView
              key={`streaming-tool-${idx}`}
              toolCall={block.toolCall}
              isStreaming={block.toolCall.result == null && !block.toolCall.error}
              className="mb-2"
            />
          );
        }
        return (
          <ToolCallIndicator
            key={`streaming-tool-${idx}`}
            toolCall={block.toolCall}
            isStreaming={block.toolCall.result == null && !block.toolCall.error}
            className="mb-2"
          />
        );
      };
      const streamingContentNodes: React.ReactNode[] = [];
      for (let idx = 0; idx < renderedStreamingContentBlocks.length; idx += 1) {
        const block = renderedStreamingContentBlocks[idx];
        if (!block) {
          continue;
        }
        if (block.type === "text") {
          // Skip empty/whitespace-only text blocks (e.g. pre-stream flush artifacts)
          if (!block.text.trim()) {
            continue;
          }
          streamingContentNodes.push(
            <React.Fragment key={`streaming-text-${idx}`}>
              <TextBubble
                text={block.text}
                isUser={false}
              />
              <MessageMeta
                createdAt={streamingMessageCreatedAt}
                copyableText={block.text.trim()}
              />
            </React.Fragment>,
          );
          continue;
        }
        if (block.type === "task") {
          const task = streamingTasks?.get(block.toolUseId);
          if (task) {
            streamingContentNodes.push(
              <TaskSubagentCard key={`streaming-task-${block.toolUseId}`} task={task} />,
            );
          }
          continue;
        }

        const entries: StreamingToolGroupEntry[] = [];
        let endIndex = idx;
        while (endIndex < renderedStreamingContentBlocks.length) {
          const nextBlock = renderedStreamingContentBlocks[endIndex];
          if (!nextBlock || nextBlock.type !== "tool_use") {
            break;
          }
          if (!shouldHideCompletedProjectOrchestrationToolCall(nextBlock.toolCall)) {
            entries.push({ block: nextBlock, index: endIndex });
          }
          endIndex += 1;
        }

        if (entries.length === 1) {
          const entry = entries[0]!;
          streamingContentNodes.push(renderStreamingToolCallBlock(entry.block, entry.index));
        } else if (entries.length > 1) {
          const groupKey = streamingToolGroupKey(entries);
          const isExpanded = expandedToolGroupKeys.has(groupKey);
          streamingContentNodes.push(
            <React.Fragment key={groupKey}>
              <div className="mb-2">
                <ToolCallGroupToggle
                  groupKey={groupKey}
                  count={entries.length}
                  isExpanded={isExpanded}
                  onToggle={(event) => toggleToolCallGroup(groupKey, event.currentTarget)}
                />
              </div>
              {isExpanded
                ? entries.map((entry) => renderStreamingToolCallBlock(entry.block, entry.index))
                : null}
            </React.Fragment>,
          );
        }

        idx = endIndex - 1;
      }
      const visibleFallbackToolCalls = shouldShowFooterFallback
        ? streamingToolCalls
          .map((toolCall, index) => ({ toolCall, index }))
          .filter(({ toolCall }) => !shouldHideCompletedProjectOrchestrationToolCall(toolCall))
        : [];
      const fallbackToolGroupKey = visibleFallbackToolCalls.length > 1
        ? [
          "streaming-pending-tool-group",
          visibleFallbackToolCalls[0]?.toolCall.id || visibleFallbackToolCalls[0]?.index || "empty",
        ].join(":")
        : null;
      const isFallbackToolGroupExpanded =
        fallbackToolGroupKey != null && expandedToolGroupKeys.has(fallbackToolGroupKey);
      const singleFallbackToolCall =
        visibleFallbackToolCalls.length === 1 ? visibleFallbackToolCalls[0] : null;
      return (
        <>
          {shouldRenderStreamingContentGroup && (
            <MessageItem
              role="assistant"
              content=""
              createdAt={streamingMessageCreatedAt}
              isLastInList={!shouldShowActiveTypingIndicator}
              toolCalls={null}
              contentBlocks={null}
              providerHarness={providerHarness}
              providerSessionId={providerSessionId}
              showAssistantIcon={streamingSenderGroupState.showSenderHeader}
              reserveAssistantIconSpace={streamingSenderGroupState.reserveAssistantGutter}
              showProviderMeta={streamingSenderGroupState.showSenderHeader}
              hideMeta
            >
              {streamingTranscriptWindow.hiddenBlockCount > 0 && (
                <div
                  data-testid="streaming-transcript-window-notice"
                  className="mb-2 rounded-md px-2 py-1 text-[11px]"
                  style={{
                    backgroundColor: "color-mix(in srgb, var(--bg-elevated) 72%, transparent)",
                    border: "1px solid var(--border-subtle)",
                    color: "var(--text-muted)",
                  }}
                >
                  {streamingTranscriptWindow.hiddenBlockCount} earlier live updates hidden
                </div>
              )}
              {streamingContentNodes}

              {/* Fallback when agent is running but no content blocks yet:
                  Tool calls pending show immediate visibility into what agent is doing. */}
              {singleFallbackToolCall && (
                <ToolCallIndicator
                  key={`pending-tool-${singleFallbackToolCall.index}`}
                  toolCall={singleFallbackToolCall.toolCall}
                  isStreaming={
                    singleFallbackToolCall.toolCall.result == null
                    && !singleFallbackToolCall.toolCall.error
                  }
                  className="mb-2"
                />
              )}
              {fallbackToolGroupKey != null && visibleFallbackToolCalls.length > 1 && (
                <>
                  <div className="mb-2">
                    <ToolCallGroupToggle
                      groupKey={fallbackToolGroupKey}
                      count={visibleFallbackToolCalls.length}
                      isExpanded={isFallbackToolGroupExpanded}
                      onToggle={(event) => toggleToolCallGroup(fallbackToolGroupKey, event.currentTarget)}
                    />
                  </div>
                  {isFallbackToolGroupExpanded
                    ? visibleFallbackToolCalls.map(({ toolCall, index }) => (
                      <ToolCallIndicator
                        key={`pending-tool-${index}`}
                        toolCall={toolCall}
                        isStreaming={toolCall.result == null && !toolCall.error}
                        className="mb-2"
                      />
                    ))
                    : null}
                </>
              )}
            </MessageItem>
          )}
          {shouldShowActiveTypingIndicator && (
            <TypingIndicator label={activeTypingIndicatorLabel} />
          )}
        </>
      );
    }, [
      hasFooterStreamingContent,
      shouldRenderStreamingContentGroup,
      renderedStreamingContentBlocks,
      expandedToolGroupKeys,
      providerHarness,
      providerSessionId,
      streamingSenderGroupState,
      activeTypingIndicatorLabel,
      shouldShowActiveTypingIndicator,
      shouldShowFooterFallback,
      streamingMessageCreatedAt,
      streamingTranscriptWindow,
      streamingTasks,
      streamingToolCalls,
      toggleToolCallGroup,
    ]);

    // Memoize Virtuoso components to prevent infinite re-render loop.
    // Inline object literals create new references every render, causing Virtuoso
    // to re-mount Header → layout change → atBottomStateChange → re-render → loop.
    const virtuosoComponents = useMemo(() => ({
      Scroller: ChatVirtuosoScroller,
      Header: () => (
        <div
          className={cn("px-3 w-full", topInsetClassName ?? "pt-3")}
          style={contentContainerStyle}
        >
          <ContentShell className={contentWidthClassName}>
            {/* Show failed run banner if last run failed */}
            {failedRun?.errorMessage && onDismissFailedRun && (
              <FailedRunBanner
                errorMessage={failedRun.errorMessage}
                onDismiss={() => onDismissFailedRun(failedRun.id)}
              />
            )}
          </ContentShell>
        </div>
      ),
    }), [
      contentWidthClassName,
      failedRun, onDismissFailedRun,
      topInsetClassName,
    ]);

    // Detect when a teammate tab filter produces zero timeline items but messages exist.
    const isFilteredTabEmpty = teamFilter && teamFilter !== "lead" && timeline.length === 0 && messages.length > 0;
    const emptyTabLabel = isFilteredTabEmpty
      ? (teamFilter === "lead" ? "Lead" : teamFilter)
      : null;

    // Helper to look up teammate info from team store
    const getTeammateInfo = useCallback((sender: string | null | undefined) => {
      if (!sender || !contextKey) {
        return { teammateName: null, teammateColor: null };
      }
      const selector = selectTeammateByName(contextKey, sender);
      const teammate = selector(useTeamStore.getState());
      return {
        teammateName: teammate?.name ?? null,
        teammateColor: teammate?.color ?? null,
      };
    }, [contextKey]);

    // Memoize itemContent — lookup teammate info for team mode messages
    const renderItem = useCallback((index: number, item: TimelineItem) => {
      const timelineIndex = index - firstItemIndex;
      const isLastVisibleTimelineItem = timelineIndex === lastVisibleTimelineIndex;
      if (item.kind === "hook") {
        return (
          <div className="px-3 w-full" style={contentContainerStyle}>
            <ContentShell className={contentWidthClassName}>
              <HookEventMessage event={item.data} />
            </ContentShell>
          </div>
        );
      }
      if (item.kind === "team_event") {
        const teamMsg = item.data;
        return (
          <div className="px-3 w-full" style={contentContainerStyle}>
            <ContentShell className={contentWidthClassName}>
              <TeamMessageBubble
                from={teamMsg.from}
                to={teamMsg.to}
                content={teamMsg.content}
                timestamp={teamMsg.timestamp}
              />
            </ContentShell>
          </div>
        );
      }
      if (item.kind === "streaming") {
        if (!footerContent) {
          return null;
        }
        return (
          <div ref={handleFooterRef} className="px-3 pb-3 w-full relative" style={contentContainerStyle}>
            <ContentShell className={contentWidthClassName}>
              {footerContent}
            </ContentShell>
          </div>
        );
      }
      if (isCollapsedToolCallGroupCoveredItem(item, expandedToolGroupKeys)) {
        return null;
      }
      const msg = item.data;
      const senderGroupState =
        timelineSenderGroups[timelineIndex] ?? DEFAULT_ASSISTANT_GROUP_STATE;
      const toolCallGroup = item.toolCallGroup;
      const isExpandedToolCallGroup =
        toolCallGroup != null && expandedToolGroupKeys.has(toolCallGroup.key);
      const { teammateName, teammateColor } = isProviderRole(msg.role)
        ? getTeammateInfo(msg.sender)
        : { teammateName: null, teammateColor: null };
      const groupToggleRow = toolCallGroup?.position === "toggle"
        ? (
          <ToolCallGroupToggleRow
            msg={msg}
            marker={toolCallGroup}
            senderGroupState={senderGroupState}
            isLastInList={isLastVisibleTimelineItem && !isExpandedToolCallGroup}
            isExpanded={isExpandedToolCallGroup}
            teammateName={teammateName}
            teammateColor={teammateColor}
            onToggle={(event) => toggleToolCallGroup(toolCallGroup.key, event.currentTarget)}
            contentWidthClassName={contentWidthClassName}
            rowRef={
              isLastVisibleTimelineItem && !isExpandedToolCallGroup
                ? handleLastRenderedRowRef
                : undefined
            }
          />
        )
        : null;

      if (groupToggleRow && !isExpandedToolCallGroup) {
        return groupToggleRow;
      }

      const messageMetadata = parseMessageMetadata(msg.metadata);
      const systemCard = renderSystemCard(
        messageMetadata,
        msg.content,
        msg.createdAt,
      );
      if (systemCard) {
        return (
          <div className="px-3 w-full" style={contentContainerStyle}>
            <ContentShell className={contentWidthClassName}>{systemCard}</ContentShell>
          </div>
        );
      }

      const composerReferences = parseComposerReferencesFromMetadata(messageMetadata);
      const effectiveSenderGroupState =
        toolCallGroup?.position === "toggle" && isExpandedToolCallGroup
          ? { ...senderGroupState, showSenderHeader: false }
          : senderGroupState;

      const messageRow = (
        <div
          ref={isLastVisibleTimelineItem ? handleLastRenderedRowRef : undefined}
          className="px-3 w-full"
          data-chat-last-rendered-row={isLastVisibleTimelineItem ? "true" : undefined}
          style={contentContainerStyle}
        >
          <ContentShell className={contentWidthClassName}>
            <MessageItem
              role={msg.role}
              content={msg.content}
              createdAt={msg.createdAt}
              isLastInList={isLastVisibleTimelineItem}
              toolCalls={msg.toolCalls ?? null}
              contentBlocks={msg.contentBlocks ?? null}
              {...(msg.attachments && { attachments: msg.attachments })}
              {...(composerReferences ? { composerReferences } : {})}
              teammateName={teammateName}
              teammateColor={teammateColor}
              providerHarness={msg.providerHarness}
              providerSessionId={msg.providerSessionId}
              upstreamProvider={msg.upstreamProvider}
              providerProfile={msg.providerProfile}
              logicalModel={msg.logicalModel}
              effectiveModelId={msg.effectiveModelId}
              logicalEffort={msg.logicalEffort}
              effectiveEffort={msg.effectiveEffort}
              inputTokens={msg.inputTokens}
              outputTokens={msg.outputTokens}
              cacheCreationTokens={msg.cacheCreationTokens}
              cacheReadTokens={msg.cacheReadTokens}
              estimatedUsd={msg.estimatedUsd}
              showAssistantIcon={effectiveSenderGroupState.showSenderHeader}
              reserveAssistantIconSpace={effectiveSenderGroupState.reserveAssistantGutter}
              showProviderMeta={effectiveSenderGroupState.showSenderHeader}
            />
          </ContentShell>
        </div>
      );
      return groupToggleRow ? (
        <>
          {groupToggleRow}
          {messageRow}
        </>
      ) : messageRow;
    }, [
      contentWidthClassName,
      expandedToolGroupKeys,
      firstItemIndex,
      footerContent,
      getTeammateInfo,
      handleFooterRef,
      handleLastRenderedRowRef,
      lastVisibleTimelineIndex,
      timelineSenderGroups,
      toggleToolCallGroup,
    ]);

    if (isTestEnv) {
      return (
        <div
          ref={transcriptRootRef}
          className="flex-1 overflow-hidden relative"
          data-testid="integrated-chat-messages"
        >
          {shouldShowInitialPaintCover && (
            <ConversationTranscriptPlaceholders
              contentWidthClassName={contentWidthClassName}
              className="pointer-events-none absolute inset-0 z-10 bg-[var(--bg-base)]"
              testId="chat-transcript-settling-placeholders"
              ariaHidden
            />
          )}
          {isFilteredTabEmpty && (
            <div className="flex-1 flex items-center justify-center h-full" data-testid="teammate-tab-empty">
              <span className="text-sm" style={{ color: "var(--text-muted)" }}>
                No messages from {emptyTabLabel} yet
              </span>
            </div>
          )}
          <div
            className={cn("px-3 w-full", topInsetClassName ?? "pt-3")}
            style={contentContainerStyle}
          >
            <ContentShell className={contentWidthClassName}>
              {failedRun?.errorMessage && onDismissFailedRun && (
                <FailedRunBanner
                  errorMessage={failedRun.errorMessage}
                  onDismiss={() => onDismissFailedRun(failedRun.id)}
                />
              )}
            </ContentShell>
          </div>

          {timeline.map((item, index) => {
            if (item.kind === "hook") {
              return (
                <div key={`${item.kind}-${item.sortTime}-${index}`} className="px-3 w-full" style={contentContainerStyle}>
                  <ContentShell className={contentWidthClassName}>
                    <HookEventMessage event={item.data} />
                  </ContentShell>
                </div>
              );
            }
            if (item.kind === "team_event") {
              const teamMsg = item.data;
              return (
                <div key={`team-${teamMsg.id}`} className="px-3 w-full" style={contentContainerStyle}>
                  <ContentShell className={contentWidthClassName}>
                    <TeamMessageBubble
                      from={teamMsg.from}
                      to={teamMsg.to}
                      content={teamMsg.content}
                      timestamp={teamMsg.timestamp}
                    />
                  </ContentShell>
                </div>
              );
            }
            if (item.kind === "streaming") {
              if (!footerContent) {
                return null;
              }
              return (
                <div key="streaming-live" ref={handleFooterRef} className="px-3 pb-3 w-full relative" style={contentContainerStyle}>
                  <ContentShell className={contentWidthClassName}>
                    {footerContent}
                    <div ref={messagesEndRef} />
                  </ContentShell>
                </div>
              );
            }
            if (isCollapsedToolCallGroupCoveredItem(item, expandedToolGroupKeys)) {
              return null;
            }
            const msg = item.data;
            const senderGroupState =
              timelineSenderGroups[index] ?? DEFAULT_ASSISTANT_GROUP_STATE;
            const toolCallGroup = item.toolCallGroup;
            const isExpandedToolCallGroup =
              toolCallGroup != null && expandedToolGroupKeys.has(toolCallGroup.key);
            const isLastVisibleTimelineItem = index === lastVisibleTimelineIndex;
            const { teammateName, teammateColor } = isProviderRole(msg.role)
              ? getTeammateInfo(msg.sender)
              : { teammateName: null, teammateColor: null };
            const groupToggleRow = toolCallGroup?.position === "toggle"
              ? (
                <ToolCallGroupToggleRow
                  key={`tool-call-group-${toolCallGroup.key}`}
                  msg={msg}
                  marker={toolCallGroup}
                  senderGroupState={senderGroupState}
                  isLastInList={isLastVisibleTimelineItem && !isExpandedToolCallGroup}
                  isExpanded={isExpandedToolCallGroup}
                  teammateName={teammateName}
                  teammateColor={teammateColor}
                  onToggle={(event) => toggleToolCallGroup(toolCallGroup.key, event.currentTarget)}
                  contentWidthClassName={contentWidthClassName}
                  rowRef={
                    isLastVisibleTimelineItem && !isExpandedToolCallGroup
                      ? handleLastRenderedRowRef
                      : undefined
                  }
                />
              )
              : null;

            if (groupToggleRow && !isExpandedToolCallGroup) {
              return groupToggleRow;
            }

            const messageMetadata = parseMessageMetadata(msg.metadata);
            const systemCard = renderSystemCard(
              messageMetadata,
              msg.content,
              msg.createdAt,
            );
            if (systemCard) {
              return (
                <div key={`message-${msg.id}`} className="px-3 w-full" style={contentContainerStyle}>
                  <ContentShell className={contentWidthClassName}>{systemCard}</ContentShell>
                </div>
              );
            }

            const composerReferences = parseComposerReferencesFromMetadata(messageMetadata);
            const effectiveSenderGroupState =
              toolCallGroup?.position === "toggle" && isExpandedToolCallGroup
                ? { ...senderGroupState, showSenderHeader: false }
                : senderGroupState;

            const messageRow = (
              <div
                key={`message-${msg.id}`}
                ref={isLastVisibleTimelineItem ? handleLastRenderedRowRef : undefined}
                className="px-3 w-full"
                data-chat-last-rendered-row={isLastVisibleTimelineItem ? "true" : undefined}
                style={contentContainerStyle}
              >
                <ContentShell className={contentWidthClassName}>
                  <MessageItem
                    role={msg.role}
                    content={msg.content}
                    createdAt={msg.createdAt}
                    isLastInList={isLastVisibleTimelineItem}
                    toolCalls={msg.toolCalls ?? null}
                    contentBlocks={msg.contentBlocks ?? null}
                    {...(msg.attachments && { attachments: msg.attachments })}
                    {...(composerReferences ? { composerReferences } : {})}
                    teammateName={teammateName}
                    teammateColor={teammateColor}
                    providerHarness={msg.providerHarness}
                    providerSessionId={msg.providerSessionId}
                    upstreamProvider={msg.upstreamProvider}
                    providerProfile={msg.providerProfile}
                    logicalModel={msg.logicalModel}
                    effectiveModelId={msg.effectiveModelId}
                    logicalEffort={msg.logicalEffort}
                    effectiveEffort={msg.effectiveEffort}
                    inputTokens={msg.inputTokens}
                    outputTokens={msg.outputTokens}
                    cacheCreationTokens={msg.cacheCreationTokens}
                    cacheReadTokens={msg.cacheReadTokens}
                    estimatedUsd={msg.estimatedUsd}
                    showAssistantIcon={effectiveSenderGroupState.showSenderHeader}
                    reserveAssistantIconSpace={effectiveSenderGroupState.reserveAssistantGutter}
                    showProviderMeta={effectiveSenderGroupState.showSenderHeader}
                  />
                </ContentShell>
              </div>
            );
            return groupToggleRow ? (
              <React.Fragment key={`expanded-tool-call-group-${toolCallGroup?.key ?? msg.id}`}>
                {groupToggleRow}
                {messageRow}
              </React.Fragment>
            ) : messageRow;
          })}

          <ScrollToBottomControl
            visible={shouldShowScrollToBottom}
            onClick={handleScrollToBottomClick}
            onWheel={handleScrollToBottomWheel}
          />
        </div>
      );
    }

    return (
      <ToolCallStoreKeyContext.Provider value={contextKey ?? null}>
      <div
        ref={transcriptRootRef}
        className="flex-1 overflow-hidden relative"
        data-testid="integrated-chat-messages"
      >
        {shouldShowInitialPaintCover && (
          <ConversationTranscriptPlaceholders
            contentWidthClassName={contentWidthClassName}
            className="pointer-events-none absolute inset-0 z-10 bg-[var(--bg-base)]"
            testId="chat-transcript-settling-placeholders"
            ariaHidden
          />
        )}
        {isFilteredTabEmpty && (
          <div className="absolute inset-0 flex items-center justify-center" data-testid="teammate-tab-empty">
            <span className="text-sm" style={{ color: "var(--text-muted)" }}>
              No messages from {emptyTabLabel} yet
            </span>
          </div>
        )}
        <Virtuoso
          // Key forces complete remount when conversation changes - prevents scroll animation conflicts
          key={conversationId ?? "empty"}
          ref={virtuosoRef}
          scrollerRef={handleScrollerRef}
          data={timeline}
          firstItemIndex={firstItemIndex}
          context={footerContentHash}
          // Start at the last item on mount
          initialTopMostItemIndex={timeline.length > 0 ? lastItemIndex : 0}
          followOutput={handleGuardedFollowOutput}
          atBottomStateChange={handleVirtuosoAtBottomStateChange}
          atBottomThreshold={AT_BOTTOM_THRESHOLD}
          rangeChanged={handleRangeChanged}
          totalListHeightChanged={handleTotalListHeightChanged}
          {...(startReachedHandler
            ? { startReached: startReachedHandler }
            : {})}
          alignToBottom
          className="h-full"
          components={virtuosoComponents}
          itemContent={renderItem}
        />
        {isFetchingOlderMessages && (
          <div className="absolute top-2 left-0 right-0 flex justify-center pointer-events-none">
            <span
              className="rounded-full px-3 py-1 text-[0.6875rem]"
              style={{
                backgroundColor: "color-mix(in srgb, var(--bg-surface) 94%, transparent)",
                border: "1px solid var(--border-subtle)",
                color: "var(--text-secondary)",
              }}
            >
              Loading earlier messages...
            </span>
          </div>
        )}
        {/* Kept outside Virtuoso and always mounted so visibility changes do not rebuild the transcript. */}
        <ScrollToBottomControl
          visible={shouldShowScrollToBottom}
          onClick={handleScrollToBottomClick}
          onWheel={handleScrollToBottomWheel}
        />
      </div>
      </ToolCallStoreKeyContext.Provider>
    );
  }
);
