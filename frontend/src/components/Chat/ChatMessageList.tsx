/**
 * ChatMessageList - Virtualized message list for chat panels
 *
 * Wraps react-virtuoso with chat-specific rendering:
 * - Auto-scroll to bottom
 * - Failed run banner header
 * - Worker executing indicator
 * - Streaming tool calls / typing indicator footer
 */

import React, { forwardRef, useCallback, useEffect, useMemo, useRef, useState, useImperativeHandle } from "react";
import { Virtuoso, type ListRange, type VirtuosoHandle } from "react-virtuoso";
import { MessageItem } from "./MessageItem";
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
  getTrueBottomScrollTop,
  isScrollElementVisuallyAtBottom,
  shouldShowScrollToBottomControl,
  VISUAL_BOTTOM_EPSILON_PX,
} from "./ChatMessageList.scroll";

// ============================================================================
// Constants
// ============================================================================

/** Delay for markdown content to render and expand before scroll correction */
const MARKDOWN_RENDER_DELAY_MS = 300;

/** Shared bottom-detection threshold — used by both Virtuoso atBottomThreshold prop and rAF DOM reconciliation.
 *  Must match exactly so both agree on what "at bottom" means. */
export const AT_BOTTOM_THRESHOLD = 150;

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
};

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
}

/** Discriminated union for timeline items when hook events are interleaved */
type TimelineItem =
  | { kind: "message"; data: ChatMessageData; sortTime: number }
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
    // rAF reconciliation refs — used to keep isAtBottom accurate when footer grows
    const scrollerElRef = useRef<HTMLElement | null>(null);
    const reconcileRafRef = useRef<number | null>(null);
    const scrollerResizeObserverRef = useRef<ResizeObserver | null>(null);
    const scrollerResizeRafRef = useRef<number | null>(null);
    const isTestEnv = import.meta.env.VITEST;
    const [isVisuallyAtBottom, setIsVisuallyAtBottomState] = useState(true);
    const isVisuallyAtBottomRef = useRef(true);
    const [hasScrollerElement, setHasScrollerElement] = useState(false);
    const [hasScrollableOverflow, setHasScrollableOverflow] = useState(false);
    const [isLastItemVisible, setIsLastItemVisible] = useState<boolean | null>(true);

    // Footer ResizeObserver refs — for height-driven auto-scroll (G2 fix)
    const footerElRef = useRef<HTMLDivElement | null>(null);
    const footerResizeRafRef = useRef<number | null>(null);
    const footerObserverRef = useRef<ResizeObserver | null>(null);
    const footerPrevHeightRef = useRef<number>(-1); // -1 = uninitialized sentinel
    const footerMountedRef = useRef(false); // H2 fix: skip initial mount observation
    const hasFooterStreamingContentRef = useRef(false);
    const transcriptRootRef = useRef<HTMLDivElement | null>(null);
    const initialPaintReadyFrameRef = useRef<number | null>(null);
    const initialPaintReadyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
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

    // Tracks running max of text length across all streaming blocks.
    // State (not a ref) so changes propagate to footerContentHash and trigger autoscroll.
    // Math.max(prev, total) ensures the bucket never decreases mid-stream — prevents
    // bucket regression when tool_use blocks are inserted between text blocks.
    const [cumulativeTextLength, setCumulativeTextLength] = useState(0);

    // Recompute cumulative text length whenever streaming blocks change.
    // Resets to 0 when streaming ends (no blocks) so the next stream starts fresh.
    useEffect(() => {
      if (!normalizedStreamingContentBlocks.length) {
        setCumulativeTextLength(0);
        return;
      }
      const total = normalizedStreamingContentBlocks.reduce(
        (sum, block) => block.type === "text" ? sum + block.text.length : sum, 0
      );
      setCumulativeTextLength(prev => Math.max(prev, total));
    }, [normalizedStreamingContentBlocks]);

    const hasRenderableStreamingBlocks = useMemo(
      () =>
        normalizedStreamingContentBlocks.some((block) => {
          if (block.type === "text") {
            return block.text.trim().length > 0;
          }
          if (block.type === "task") {
            return Boolean(streamingTasks?.get(block.toolUseId));
          }
          return true;
        }),
      [normalizedStreamingContentBlocks, streamingTasks],
    );

    const shouldShowActiveTypingIndicator = isSending || isAgentRunning;
    const shouldShowFooterFallback = (isSending || isAgentRunning) && !hasRenderableStreamingBlocks;
    const hasFooterStreamingContent = hasRenderableStreamingBlocks || shouldShowFooterFallback;

    useEffect(() => {
      hasFooterStreamingContentRef.current = hasFooterStreamingContent;
    }, [hasFooterStreamingContent]);

    const footerContentHash = useMemo(() => ({
      toolCallCount: streamingToolCalls.length,
      // G1 fix: results update existing blocks (count unchanged) — track result arrivals separately
      toolResultCount: streamingToolCalls.filter(tc => tc.result != null || tc.error != null).length,
      childCallCount: totalChildCalls,
      taskCount: streamingTasks?.size ?? 0,
      contentBlockCount: normalizedStreamingContentBlocks.length,
      textLengthBucket: Math.floor(cumulativeTextLength / TEXT_LENGTH_BUCKET_SIZE),
    }), [streamingToolCalls, totalChildCalls, streamingTasks?.size, normalizedStreamingContentBlocks.length, cumulativeTextLength]);

    // Unified auto-scroll hook — Virtuoso followOutput handles new-message scroll,
    // while the useEffect above handles streaming footer growth.
    const {
      messagesEndRef,
      isAtBottom,
      isAtBottomRef,
      scrollToBottom,
      handleAtBottomStateChange,
      handleFollowOutput,
    } = useChatAutoScroll({
      messageCount: messages.length,
      disabled: !!scrollToTimestamp, // Disable auto-scroll in history mode
      virtuosoRef, // Route scrollToBottom through Virtuoso scrollToIndex
      indexOffset: firstItemIndex,
      conversationId, // Reset isAtBottom when conversation changes
    });

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
          setIsVisuallyAtBottom(true);
          return;
        }
        const target = getTrueBottomScrollTop(el);
        logger.debug("[ChatScroll] scrollToTrueBottom", {
          scrollHeight: el.scrollHeight,
          clientHeight: el.clientHeight,
          currentTop: el.scrollTop,
          target,
          behavior,
        });
        el.scrollTo({ top: target, behavior });
        setIsVisuallyAtBottom(true);
        // Eagerly mark atBottom=true so followOutput re-engages without waiting
        // for scrollend.
        if (!isAtBottomRef.current) {
          handleAtBottomStateChange(true);
        }
      },
      [scrollToBottom, setIsVisuallyAtBottom, handleAtBottomStateChange, isAtBottomRef]
    );

    // After any layout-changing event that should land at bottom, run two
    // passes — first on next frame (catches most cases), second after a short
    // delay (catches late-arriving streaming footer height growth).
    const scheduleBottomPin = useCallback(
      (reason: string, behavior: ScrollBehavior = preferredScrollBehavior) => {
        logger.debug(`[ChatScroll] scheduleBottomPin: ${reason}`);
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
            scrollToTrueBottom(behavior);
            // Second pass catches footer that grows in the same tick.
            bottomPinTimeoutRef.current = setTimeout(() => {
              bottomPinTimeoutRef.current = null;
              scrollToTrueBottom(behavior);
            }, 120);
          });

          bottomPinRafIdsRef.current.push(innerRafId);
        });

        bottomPinRafIdsRef.current.push(outerRafId);
      },
      [preferredScrollBehavior, scrollToTrueBottom]
    );

    // Streaming auto-scroll — followOutput only fires on totalCount changes,
    // NOT on Footer height growth. Pin to the true DOM bottom when the user was
    // already visually at bottom so footer/meta growth is included.
    useEffect(() => {
      if (scrollToTimestamp || !hasFooterStreamingContent) return;
      if (isVisuallyAtBottomRef.current) {
        scrollToTrueBottom("auto");
      }
    }, [footerContentHash, hasFooterStreamingContent, scrollToTimestamp, scrollToTrueBottom]);

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
      setIsVisuallyAtBottom(true);
      setHasScrollableOverflow(false);
      setIsLastItemVisible(true);
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

    // Keep scrollToTimestamp accessible via ref (avoids stale closure in ResizeObserver callback)
    const scrollToTimestampRef = useRef(scrollToTimestamp);
    useEffect(() => {
      scrollToTimestampRef.current = scrollToTimestamp;
    }, [scrollToTimestamp]);

    // rAF-throttled DOM reconciliation — keeps isAtBottom accurate when Virtuoso doesn't detect footer growth.
    // Runs outside React render cycle (DOM event handler, not useEffect) — no render loop risk.
    // rAF fires post-paint, so scrollHeight reads don't force layout recalc during React commit phase.
    const reconcileScrollerBottomState = useCallback(() => {
      const el = scrollerElRef.current;
      if (!el) return;

      const bottomDelta = getScrollBottomDelta(el);
      const atBottom = bottomDelta < AT_BOTTOM_THRESHOLD;
      const visuallyAtBottom = bottomDelta <= VISUAL_BOTTOM_EPSILON_PX;
      setHasScrollableOverflow(
        el.scrollHeight > el.clientHeight + VISUAL_BOTTOM_EPSILON_PX
      );
      setIsVisuallyAtBottom(visuallyAtBottom);

      // Only reconcile if state disagrees — avoids unnecessary setState
      if (atBottom !== isAtBottomRef.current) {
        handleAtBottomStateChange(atBottom);
      }
    }, [handleAtBottomStateChange, isAtBottomRef, setIsVisuallyAtBottom]);

    const handleScrollReconcile = useCallback(() => {
      if (reconcileRafRef.current) return; // Already scheduled — skip
      reconcileRafRef.current = requestAnimationFrame(() => {
        reconcileRafRef.current = null;
        reconcileScrollerBottomState();
      });
    }, [reconcileScrollerBottomState]);

    const handleVirtuosoAtBottomStateChange = useCallback(
      (atBottom: boolean) => {
        const el = scrollerElRef.current;
        setIsVisuallyAtBottom(
          atBottom && el ? isScrollElementVisuallyAtBottom(el) : atBottom
        );
        handleAtBottomStateChange(atBottom);
      },
      [handleAtBottomStateChange, setIsVisuallyAtBottom],
    );

    const handleScrollerResize = useCallback(() => {
      const wasVisuallyAtBottom = isVisuallyAtBottomRef.current;
      if (scrollerResizeRafRef.current !== null) {
        cancelAnimationFrame(scrollerResizeRafRef.current);
      }
      scrollerResizeRafRef.current = requestAnimationFrame(() => {
        scrollerResizeRafRef.current = null;
        if (wasVisuallyAtBottom && !scrollToTimestampRef.current) {
          scrollToTrueBottom("auto");
          return;
        }
        reconcileScrollerBottomState();
      });
    }, [reconcileScrollerBottomState, scrollToTrueBottom]);

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
          scrollerElRef.current = null;
        }
        setHasScrollerElement(false);
        setHasScrollableOverflow(false);
        disconnectScrollerResizeObserver();
        return;
      }
      if (scrollerElRef.current && scrollerElRef.current !== el) {
        scrollerElRef.current.removeEventListener("scroll", handleScrollReconcile);
        disconnectScrollerResizeObserver();
      }
      if (scrollerElRef.current === el) {
        reconcileScrollerBottomState();
        return;
      }
      scrollerElRef.current = el;
      setHasScrollerElement(true);
      el.addEventListener("scroll", handleScrollReconcile, { passive: true });
      reconcileScrollerBottomState();
      if (typeof ResizeObserver !== "undefined") {
        const observer = new ResizeObserver(handleScrollerResize);
        observer.observe(el);
        scrollerResizeObserverRef.current = observer;
      }
    }, [
      disconnectScrollerResizeObserver,
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

        // M1 fix: Cancel-reschedule rAF — don't skip if pending.
        // Rapid sequential resizes each get a scroll attempt; the last one wins.
        if (footerResizeRafRef.current) {
          cancelAnimationFrame(footerResizeRafRef.current);
        }
        footerResizeRafRef.current = requestAnimationFrame(() => {
          footerResizeRafRef.current = null;
          // Read from refs — always current, no stale closure
          if (
            hasFooterStreamingContentRef.current &&
            isVisuallyAtBottomRef.current &&
            !scrollToTimestampRef.current
          ) {
            scrollToTrueBottom("auto");
          }
        });
      });
      footerObserverRef.current.observe(el);
    }, [scrollToTrueBottom]);

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

    // When filter clears (streaming/finalizing ends), scroll to bottom so the newly
    // revealed finalized assistant message is visible.
    useEffect(() => {
      if (scrollToTimestamp) return; // Don't auto-scroll in history mode
      if (prevShouldFilterRef.current && !shouldFilterCurrentProviderMessage) {
        scheduleBottomPin("finalized provider message revealed");
      }
      prevShouldFilterRef.current = shouldFilterCurrentProviderMessage;
    }, [scheduleBottomPin, shouldFilterCurrentProviderMessage, scrollToTimestamp]);

    const timeline = useMemo((): TimelineItem[] => {
      const items: TimelineItem[] = [];

      const filteredMessages = suppressedProviderMessageId
        ? messages.filter((msg) => msg.id !== suppressedProviderMessageId)
        : messages;

      // Team filter: each tab (lead/teammate) loads its own conversation's messages via
      // useConversation, so all messages in the data set belong to that conversation.
      // No per-message filtering needed — the conversation switch handles the scoping.
      const teamFilteredMessages = filteredMessages;

      for (const msg of teamFilteredMessages) {
        // Enrich message with attachments if available
        const attachments = attachmentsMap?.get(msg.id);
        const enrichedMsg = attachments
          ? { ...msg, attachments }
          : msg;

        items.push({
          kind: "message",
          data: enrichedMsg,
          sortTime: new Date(msg.createdAt).getTime(),
        });
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

    const lastItemIndex = firstItemIndex + timeline.length - 1;
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
      scrollToTrueBottom(preferredScrollBehavior);
      scheduleBottomPin("manual scroll-to-bottom", preferredScrollBehavior);
    }, [preferredScrollBehavior, scheduleBottomPin, scrollToTrueBottom]);
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
        scrollElementByDelta(el, event.deltaX, event.deltaY);
        handleScrollReconcile();
      },
      [handleScrollReconcile, isTestEnv, shouldShowScrollToBottom],
    );

    const handleRangeChanged = useCallback(
      (range: ListRange) => {
        if (timeline.length > 0 && range.endIndex >= range.startIndex) {
          setIsLastItemVisible(range.endIndex >= lastItemIndex);
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

      const doScroll = () => {
        if (hasScrolledRef.current === targetScrollKey) return;
        virtuosoRef.current?.scrollToIndex({
          index: lastItemIndex,
          align: "end",
          behavior: "auto",
        });
        scheduleBottomPin("initial conversation load", "auto");
        hasScrolledRef.current = targetScrollKey;

        // Content (markdown, code blocks, tool results) can keep rendering after
        // the initial scroll. Verify we're actually at bottom and retry if not.
        const verifyAtBottom = () => {
          const el = scrollerElRef.current;
          if (!el) return;
          const delta = el.scrollHeight - el.clientHeight - el.scrollTop;
          if (delta > AT_BOTTOM_THRESHOLD) {
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

      // Safety timeout: 3s max — disconnect + force scroll if debounce never settles
      const safetyTimer = setTimeout(() => {
        observer.disconnect();
        doScroll();
      }, 3000);

      return () => {
        observer.disconnect();
        clearTimeout(debounceTimer);
        clearTimeout(safetyTimer);
        verifyTimers.forEach(clearTimeout);
      };
    }, [conversationId, lastItemIndex, scheduleBottomPin, scrollToTrueBottom, timeline.length]);

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

      if (isVisuallyAtBottomRef.current) {
        scheduleBottomPin("new timeline item appended");
      }
    }, [lastItemIndex, scheduleBottomPin, scrollToTimestamp, timeline.length]);

    const footerContent = useMemo(() => {
      if (!hasFooterStreamingContent) {
        return null;
      }
      const streamingCopyContent = normalizedStreamingContentBlocks
        .filter((block) => block.type === "text" && block.text.trim().length > 0)
        .map((block) => block.type === "text" ? block.text.trim() : "")
        .join("\n\n");

      return (
        <MessageItem
          role="assistant"
          content={streamingCopyContent}
          createdAt={new Date().toISOString()}
          isLastInList
          toolCalls={null}
          contentBlocks={null}
          providerHarness={providerHarness}
          providerSessionId={providerSessionId}
        >
          {normalizedStreamingContentBlocks.map((block, idx) => {
            if (block.type === "text") {
              // Skip empty/whitespace-only text blocks (e.g. pre-stream flush artifacts)
              if (!block.text.trim()) return null;
              return (
                <TextBubble
                  key={`streaming-text-${idx}`}
                  text={block.text}
                  isUser={false}
                />
              );
            }
            // task position marker — renders TaskSubagentCard at its chronological position.
            // Task metadata may not be available yet (agent:task_started fires after agent:tool_call),
            // so render nothing gracefully when the map entry is missing.
            if (block.type === "task") {
              const task = streamingTasks?.get(block.toolUseId);
              if (!task) return null;
              return <TaskSubagentCard key={`streaming-task-${block.toolUseId}`} task={task} />;
            }
            // tool_use block — diff calls render as DiffToolCallView, all others render as ToolCallIndicator
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
            // Non-diff tool call — render inline to preserve visual ordering with text blocks
            if (shouldHideCompletedProjectOrchestrationToolCall(block.toolCall)) {
              return null;
            }
            return (
              <ToolCallIndicator
                key={`streaming-tool-${idx}`}
                toolCall={block.toolCall}
                isStreaming={block.toolCall.result == null && !block.toolCall.error}
                className="mb-2"
              />
            );
          })}

          {shouldShowActiveTypingIndicator && hasRenderableStreamingBlocks && (
            <TypingIndicator />
          )}

          {/* Fallback when agent is running but no content blocks yet:
              - Tool calls pending → show ToolCallIndicator for each (immediate visibility into what agent is doing)
              - No tool calls either → show TypingIndicator (agent thinking) */}
          {shouldShowFooterFallback && (
            <>
              {streamingToolCalls.length > 0 && streamingToolCalls.map((tc, idx) => (
                shouldHideCompletedProjectOrchestrationToolCall(tc)
                  ? null
                  : (
                    <ToolCallIndicator
                      key={`pending-tool-${idx}`}
                      toolCall={tc}
                      isStreaming={tc.result == null && !tc.error}
                      className="mb-2"
                    />
                  )
              ))}
              <TypingIndicator />
            </>
          )}
        </MessageItem>
      );
    }, [
      hasFooterStreamingContent,
      hasRenderableStreamingBlocks,
      normalizedStreamingContentBlocks,
      providerHarness,
      providerSessionId,
      shouldShowActiveTypingIndicator,
      shouldShowFooterFallback,
      streamingTasks,
      streamingToolCalls,
    ]);

    // Memoize Virtuoso components to prevent infinite re-render loop.
    // Inline object literals create new references every render, causing Virtuoso
    // to re-mount Header → layout change → atBottomStateChange → re-render → loop.
    const virtuosoComponents = useMemo(() => ({
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
      const isLastTimelineItem = index === timeline.length - 1;
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
      const msg = item.data;

      const systemCard = renderSystemCard(
        parseMessageMetadata(msg.metadata),
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

      // Look up teammate info if sender is present and message is from assistant
      const { teammateName, teammateColor } = isProviderRole(msg.role)
        ? getTeammateInfo(msg.sender)
        : { teammateName: null, teammateColor: null };

      return (
        <div className="px-3 w-full" style={contentContainerStyle}>
          <ContentShell className={contentWidthClassName}>
            <MessageItem
              role={msg.role}
              content={msg.content}
              createdAt={msg.createdAt}
              isLastInList={isLastTimelineItem}
              toolCalls={msg.toolCalls ?? null}
              contentBlocks={msg.contentBlocks ?? null}
              {...(msg.attachments && { attachments: msg.attachments })}
              teammateName={teammateName}
              teammateColor={teammateColor}
              providerHarness={msg.providerHarness ?? providerHarness}
              providerSessionId={msg.providerSessionId ?? providerSessionId}
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
            />
          </ContentShell>
        </div>
      );
    }, [contentWidthClassName, footerContent, getTeammateInfo, handleFooterRef, providerHarness, providerSessionId, timeline.length]);

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
            const msg = item.data;

            const systemCard = renderSystemCard(
              parseMessageMetadata(msg.metadata),
              msg.content,
              msg.createdAt,
            );
            if (systemCard) {
              return (
                <div key={`${item.kind}-${item.sortTime}-${index}`} className="px-3 w-full" style={contentContainerStyle}>
                  <ContentShell className={contentWidthClassName}>{systemCard}</ContentShell>
                </div>
              );
            }

            const { teammateName, teammateColor } = isProviderRole(msg.role)
              ? getTeammateInfo(msg.sender)
              : { teammateName: null, teammateColor: null };

            return (
              <div key={`${item.kind}-${item.sortTime}-${index}`} className="px-3 w-full" style={contentContainerStyle}>
                <ContentShell className={contentWidthClassName}>
                  <MessageItem
                    role={msg.role}
                    content={msg.content}
                    createdAt={msg.createdAt}
                    isLastInList={index === timeline.length - 1}
                    toolCalls={msg.toolCalls ?? null}
                    contentBlocks={msg.contentBlocks ?? null}
                    {...(msg.attachments && { attachments: msg.attachments })}
                    teammateName={teammateName}
                    teammateColor={teammateColor}
                    providerHarness={msg.providerHarness ?? providerHarness}
                    providerSessionId={msg.providerSessionId ?? providerSessionId}
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
                  />
                </ContentShell>
              </div>
            );
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
          followOutput={handleFollowOutput}
          atBottomStateChange={handleVirtuosoAtBottomStateChange}
          atBottomThreshold={AT_BOTTOM_THRESHOLD}
          rangeChanged={handleRangeChanged}
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
