/**
 * useChatAutoScroll - Unified scroll behavior for all chat components
 *
 * Single-path scroll control: Virtuoso's followOutput callback is the ONLY
 * auto-scroll mechanism. No useEffect triggers, no DOM marker scrolling,
 * no requestAnimationFrame — just one callback that Virtuoso invokes when
 * content changes and the user is at bottom.
 *
 * Key features:
 * - Bottom tracking with 150px threshold (via Virtuoso atBottomStateChange)
 * - Virtuoso-native scrolling when virtuosoRef is provided
 * - DOM-based scrolling via messagesEndRef for non-Virtuoso consumers only
 * - Respects user manual scroll (pauses when scrolled up)
 * - Manual override via scrollToBottom()
 * - Disableable for history time-travel mode
 */

import { useRef, useCallback, useState, useEffect } from "react";
import type { VirtuosoHandle } from "react-virtuoso";
import { shouldUseWebkitSafeScrollBehavior } from "@/lib/platform-quirks";

// ============================================================================
// Types
// ============================================================================

export interface UseChatAutoScrollProps {
  /** Number of rendered scroll items - used for scrollToIndex target in Virtuoso mode */
  messageCount: number;
  /** Absolute index offset of the first loaded item (for paged/prepended history) */
  indexOffset?: number;
  /** Disable auto-scroll (for history mode) */
  disabled?: boolean;
  /** Virtuoso handle ref — when provided, all scrolling goes through Virtuoso APIs */
  virtuosoRef?: React.RefObject<VirtuosoHandle | null>;
  /** Conversation ID — when it changes, resets isAtBottom to true for the new conversation */
  conversationId?: string | null;
}

export interface UseChatAutoScrollReturn {
  /** Ref to attach to scroll container (for div-based components) */
  containerRef: React.RefObject<HTMLDivElement | null>;
  /** Ref to attach to end-of-messages marker (for div-based components only) */
  messagesEndRef: React.RefObject<HTMLDivElement | null>;
  /** Is user at bottom of scroll area */
  isAtBottom: boolean;
  /** Ref mirror of isAtBottom — readable without triggering re-renders */
  isAtBottomRef: React.RefObject<boolean>;
  /** Should auto-scroll (computed: isAtBottom && !disabled) */
  shouldAutoScroll: boolean;
  /** Manual scroll-to-bottom function */
  scrollToBottom: () => void;
  /** Virtuoso callback: atBottomStateChange */
  handleAtBottomStateChange: (atBottom: boolean) => void;
  /** Virtuoso callback: followOutput */
  handleFollowOutput: (isAtBottom: boolean) => "smooth" | "auto" | false;
}

// ============================================================================
// Hook
// ============================================================================

export function useChatAutoScroll({
  messageCount,
  indexOffset = 0,
  disabled = false,
  virtuosoRef,
  conversationId,
}: UseChatAutoScrollProps): UseChatAutoScrollReturn {
  const preferredScrollBehavior = shouldUseWebkitSafeScrollBehavior()
    ? "auto"
    : "smooth";

  // Refs for div-based scroll components (non-Virtuoso fallback)
  const containerRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Track if user is at bottom
  const [isAtBottom, setIsAtBottom] = useState(true);
  const isAtBottomRef = useRef(true);

  // Reset isAtBottom when conversation changes — ensures a scrolled-up state from
  // the previous conversation doesn't carry over to the new one.
  useEffect(() => {
    if (conversationId == null) return;
    isAtBottomRef.current = true;
    setIsAtBottom(true);
  }, [conversationId]);

  // Computed: should we auto-scroll?
  const shouldAutoScroll = isAtBottom && !disabled;

  // Virtuoso callback: update bottom state when user scrolls
  // Dedup guard prevents re-render when value hasn't changed — breaks the
  // atBottomStateChange → re-render → new Footer → atBottomStateChange feedback loop.
  const handleAtBottomStateChange = useCallback((atBottom: boolean) => {
    if (isAtBottomRef.current === atBottom) return;
    isAtBottomRef.current = atBottom;
    setIsAtBottom(atBottom);
  }, []);

  // Virtuoso callback: control followOutput behavior
  // This is the ONLY auto-scroll mechanism for Virtuoso-based lists.
  // When user is at bottom and not in history mode, Virtuoso smoothly
  // follows new content (messages, footer height changes).
  const handleFollowOutput = useCallback(
    (atBottom: boolean) => {
      if (atBottom && !disabled) return preferredScrollBehavior;
      return false as const;
    },
    [disabled, preferredScrollBehavior]
  );

  // Stable ref for messageCount — keeps scrollToBottom identity stable across
  // rendered item count changes. Without this, every new item creates a new
  // scrollToBottom → busts virtuosoComponents useMemo → Virtuoso re-mounts.
  const messageCountRef = useRef(messageCount);
  useEffect(() => {
    messageCountRef.current = messageCount;
  }, [messageCount]);

  const indexOffsetRef = useRef(indexOffset);
  useEffect(() => {
    indexOffsetRef.current = indexOffset;
  }, [indexOffset]);

  // Manual scroll-to-bottom
  // Routes through Virtuoso scrollToIndex when available, falls back to DOM marker
  const scrollToBottom = useCallback(() => {
    isAtBottomRef.current = true;
    setIsAtBottom(true);
    const count = messageCountRef.current;
    if (virtuosoRef?.current && count > 0) {
      virtuosoRef.current.scrollToIndex({
        index: indexOffsetRef.current + count - 1,
        align: "end",
        behavior: preferredScrollBehavior,
      });
    } else {
      const endMarker = messagesEndRef.current;
      if (endMarker && typeof endMarker.scrollIntoView === "function") {
        endMarker.scrollIntoView({ behavior: preferredScrollBehavior });
      }
    }
  }, [preferredScrollBehavior, virtuosoRef]);

  // NOTE: No useEffect auto-scroll triggers here for Virtuoso mode.
  // Virtuoso's followOutput callback handles all auto-scrolling natively.
  // The old messagesEndRef.scrollIntoView effects competed with followOutput
  // causing dual scroll mechanisms and jank during streaming.

  return {
    containerRef,
    messagesEndRef,
    isAtBottom,
    isAtBottomRef,
    shouldAutoScroll,
    scrollToBottom,
    handleAtBottomStateChange,
    handleFollowOutput,
  };
}
