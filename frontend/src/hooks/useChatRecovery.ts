/**
 * useChatRecovery — Recovery and polling effects for chat panels
 *
 * Extracted from IntegratedChatPanel to reduce component size.
 * Handles:
 * - Agent running state sync from backend
 * - Clearing stuck "running" state
 * - Polling conversation/list while agent is running
 * - Startup recovery window for agent contexts
 * - Merge watchdog polling
 */

import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { chatKeys, invalidateConversationDataQueries } from "@/hooks/useChat";
import { taskKeys } from "@/hooks/useTasks";
import type { ContextType } from "@/types/chat-conversation";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import { MERGE_STATUSES } from "@/types/status";
import { chatApi } from "@/api/chat";
import { isRemoteEnvironmentActive } from "@/hooks/useActiveEnvironment";
import { isRemotelyAvailable } from "@/lib/remote/agent-gate";
import { useChatStore } from "@/stores/chatStore";
import {
  mergeActiveStreamingContentBlocks,
  mergeActiveStreamingTasks,
  mergeActiveStreamingToolCalls,
  applyTranscriptInput,
  createLiveTranscriptState,
  preserveBlocksIfUnchanged,
  renderTranscriptSlots,
} from "./chat-active-state";

// ============================================================================
// Types
// ============================================================================

interface UseChatRecoveryProps {
  activeConversationId: string | null | undefined;
  storeContextKey: string;
  currentContextType: ContextType;
  /** Context ID used to key is_agent_running — bypasses activeConversationId mismatch */
  currentContextId: string;
  isHistoryMode: boolean;
  isAgentContext: boolean;
  isAgentRunning: boolean;
  /** Whether the agent is actively streaming chunks (generating state). Suppresses polling to avoid redundant refetches during streaming. */
  isGenerating: boolean;
  /** Whether active conversation belongs to current context */
  isConversationInCurrentContext: boolean;
  /** Backend agent run status */
  agentRunStatus: string | undefined;
  /** Current parent run; active-state snapshots from any other run are non-authoritative. */
  activeAgentRunId?: string;
  /** Whether the containing panel is currently visible. */
  isVisible: boolean;
  /** Canonical streaming timeline anchors loaded before active-state hydration. */
  persistedStreamingContentBlocks?: readonly StreamingContentBlock[];
  /** Prevent active-state recovery from racing the initial timeline response. */
  isTimelineHydrated?: boolean;
  setStreamingTasks?: (
    updater: (prev: Map<string, StreamingTask>) => Map<string, StreamingTask>,
  ) => void;
  setStreamingToolCalls?: (
    updater: (prev: ToolCall[]) => ToolCall[],
  ) => void;
  setStreamingContentBlocks?: (
    updater: (prev: StreamingContentBlock[]) => StreamingContentBlock[],
  ) => void;
  setAgentRunning: (contextKey: string, isRunning: boolean) => void;
  selectedTaskId: string | undefined;
  ideationSessionId: string | undefined;
  projectId: string | null;
  /** Effective status for merge watchdog */
  effectiveStatus: string | undefined;
}

// ============================================================================
// Hook
// ============================================================================

export function useChatRecovery({
  activeConversationId,
  storeContextKey,
  currentContextType,
  currentContextId,
  isHistoryMode,
  isAgentContext,
  isAgentRunning,
  isGenerating,
  isConversationInCurrentContext,
  agentRunStatus,
  activeAgentRunId,
  isVisible,
  persistedStreamingContentBlocks = [],
  isTimelineHydrated = true,
  setStreamingTasks,
  setStreamingToolCalls,
  setStreamingContentBlocks,
  setAgentRunning,
  selectedTaskId,
  ideationSessionId,
  projectId,
  effectiveStatus,
}: UseChatRecoveryProps) {
  const queryClient = useQueryClient();
  const hydratedConversationKeyRef = useRef<string | null>(null);
  const hydrationGenerationRef = useRef(0);
  const hydrationWithoutAnchorsRef = useRef<{ key: string; runId: string } | null>(null);
  const refreshedTimelineKeyRef = useRef<string | null>(null);

  const [hydrationKeyId, setHydrationKeyId] = useState(activeConversationId);
  const [isStreamingHydrated, setIsStreamingHydrated] = useState(false);
  const [timelineRefreshGeneration, setTimelineRefreshGeneration] = useState(0);

  if (hydrationKeyId !== activeConversationId) {
    setHydrationKeyId(activeConversationId);
    setIsStreamingHydrated(false);
  }

  // A timeline fetch can settle after the first active-state merge. Re-run
  // once with its durable anchors so live blocks are reconciled against the
  // canonical timeline instead of an empty pre-switch cache.
  useEffect(() => {
    const stillStreaming = isAgentRunning || agentRunStatus === "running" || isGenerating;
    const previousHydration = hydrationWithoutAnchorsRef.current;
    if (
      !activeConversationId
      || !stillStreaming
      || persistedStreamingContentBlocks.length === 0
      || previousHydration == null
      || previousHydration.key !== `${activeConversationId}:${activeAgentRunId ?? "unknown"}`
      || (activeAgentRunId != null && previousHydration.runId !== activeAgentRunId)
    ) {
      return;
    }

    hydratedConversationKeyRef.current = null;
    hydrationWithoutAnchorsRef.current = null;
  }, [
    activeAgentRunId,
    activeConversationId,
    agentRunStatus,
    isAgentRunning,
    isGenerating,
    persistedStreamingContentBlocks.length,
  ]);

  useEffect(() => {
    if (!activeConversationId) {
      hydratedConversationKeyRef.current = null;
      setIsStreamingHydrated(true);
      return;
    }
    const canHydrateStreamingState =
      Boolean(setStreamingTasks) ||
      Boolean(setStreamingToolCalls) ||
      Boolean(setStreamingContentBlocks);
    if (
      isHistoryMode
      || !isConversationInCurrentContext
      || !canHydrateStreamingState
      || !isTimelineHydrated
    ) {
      setIsStreamingHydrated(true);
      return;
    }
    const hydrationKey = `${activeConversationId}:${activeAgentRunId ?? "unknown"}:${isVisible ? "visible" : "hidden"}`;
    if (hydratedConversationKeyRef.current === hydrationKey) {
      setIsStreamingHydrated(true);
      return;
    }

    const generation = ++hydrationGenerationRef.current;
    let cancelled = false;

    void (async () => {
      if (isAgentRunning || agentRunStatus === "running") {
        if (refreshedTimelineKeyRef.current !== hydrationKey) {
          await queryClient.refetchQueries({
            queryKey: chatKeys.conversationTimeline(activeConversationId),
            exact: true,
          });
          if (cancelled || !isVisible || generation !== hydrationGenerationRef.current) return;
          refreshedTimelineKeyRef.current = hydrationKey;
          setTimelineRefreshGeneration((previous) => previous + 1);
          return;
        }
      }
      if (cancelled || !isVisible || generation !== hydrationGenerationRef.current) return;

      hydratedConversationKeyRef.current = hydrationKey;
      return chatApi.getConversationActiveState(activeConversationId);
    })()
      .then((activeState) => {
        if (activeState == null) return;
        if (cancelled || !isVisible || generation !== hydrationGenerationRef.current) return;
        if (activeAgentRunId && activeState.runId !== activeAgentRunId) return;
        if (!activeAgentRunId && (isAgentRunning || agentRunStatus === "running")) {
          const storedRunId = useChatStore.getState().activeAgentRunIds[storeContextKey];
          if (activeState.runId == null || (storedRunId != null && storedRunId !== activeState.runId)) {
            return;
          }
        }
        const hasStreamingTasks = activeState.streaming_tasks.length > 0;
        const hasToolCalls = activeState.tool_calls.length > 0;
        const hasPartialText = activeState.partial_text.trim().length > 0;
        const hasPartialThinking = activeState.partial_thinking_segments?.some((segment) => segment.trim().length > 0) ?? false;
        if (!hasStreamingTasks && !hasToolCalls && !hasPartialText && !hasPartialThinking) return;

        if (persistedStreamingContentBlocks.length === 0 && activeState.runId != null) {
          hydrationWithoutAnchorsRef.current = {
            key: `${activeConversationId}:${activeAgentRunId ?? "unknown"}`,
            runId: activeState.runId,
          };
        }

        if (setStreamingTasks && hasStreamingTasks) {
          setStreamingTasks((prev) => mergeActiveStreamingTasks(
            prev,
            activeState.streaming_tasks,
            activeState.tool_calls,
          ));
        }
        if (setStreamingToolCalls && hasToolCalls) {
          setStreamingToolCalls((prev) =>
            mergeActiveStreamingToolCalls(
              prev,
              activeState.tool_calls,
              activeState.streaming_tasks,
            )
          );
        }
        if (setStreamingContentBlocks) {
          setStreamingContentBlocks((prev) => {
            const runId = activeState.runId ?? null;
            // Durable rows are the anchors: seeding them as slots gives the
            // identity-less cache values something indexed to reconcile onto.
            let transcript = applyTranscriptInput(createLiveTranscriptState(runId), {
              kind: "persisted", runId, blocks: persistedStreamingContentBlocks,
            });
            transcript = applyTranscriptInput(transcript, {
              kind: "live", runId, blocks: persistedStreamingContentBlocks,
            });
            transcript = applyTranscriptInput(transcript, { kind: "live", runId, blocks: prev });
            transcript = activeState.partial_text_segments?.length
              ? applyTranscriptInput(transcript, {
                  kind: "segments", runId, segments: activeState.partial_text_segments,
                })
              : applyTranscriptInput(transcript, {
                  kind: "partialText", runId, text: activeState.partial_text,
                });
            transcript = applyTranscriptInput(transcript, {
              kind: "thinkingSegments", runId, segments: activeState.partial_thinking_segments ?? [],
            });
            return preserveBlocksIfUnchanged(prev, mergeActiveStreamingContentBlocks(
              renderTranscriptSlots(transcript),
              {
                ...activeState,
                partial_text: "",
                partial_text_segments: [],
                partial_thinking_segments: [],
              },
            ));
          });
        }
      })
      .catch(() => {
        // Best-effort recovery only. Live events remain authoritative.
      })
      .finally(() => {
        if (!cancelled) {
          setIsStreamingHydrated(true);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [
    activeConversationId,
    activeAgentRunId,
    agentRunStatus,
    isHistoryMode,
    isConversationInCurrentContext,
    setStreamingTasks,
    setStreamingToolCalls,
    setStreamingContentBlocks,
    persistedStreamingContentBlocks,
    isTimelineHydrated,
    isVisible,
    isAgentRunning,
    queryClient,
    storeContextKey,
    timelineRefreshGeneration,
  ]);

  // Switching away and back to a live conversation must hydrate from persisted
  // timeline blocks first; active-state only supplements not-yet-persisted chunks.
  useEffect(() => {
    if (!activeConversationId || isHistoryMode || !isVisible || !isConversationInCurrentContext) {
      return;
    }
    if (!isAgentRunning && agentRunStatus !== "running" && !isGenerating) {
      return;
    }

    void queryClient.invalidateQueries({
      queryKey: chatKeys.conversationTimeline(activeConversationId),
    });
  }, [
    activeConversationId,
    agentRunStatus,
    isAgentRunning,
    isConversationInCurrentContext,
    isGenerating,
    isHistoryMode,
    isVisible,
    queryClient,
  ]);

  // Recovery fallback: if agent is running but events were missed, reflect it in UI
  useEffect(() => {
    if (agentRunStatus === "running" && isConversationInCurrentContext) {
      setAgentRunning(storeContextKey, true);
    }
  }, [agentRunStatus, isConversationInCurrentContext, setAgentRunning, storeContextKey]);

  // Recovery fallback: clear stuck "running" state when backend says run finished.
  // IMPORTANT: agentRunStatus reflects the DB *turn* status, not the *process*.
  // Between interactive turns, the DB shows "completed" for the finished turn
  // even though the process is still alive. Check process-level truth (IPR)
  // before clearing to avoid a race window where ChatInput takes the SEND path.
  useEffect(() => {
    if (
      isRemoteEnvironmentActive() &&
      !isRemotelyAvailable("is_agent_running")
    )
      return;
    if (!activeConversationId || !isConversationInCurrentContext) return;
    // Wait for agentRunStatus to resolve before clearing — prevents
    // thrashing during mount when status is still undefined (loading).
    if (agentRunStatus === undefined) return;
    if (agentRunStatus !== "running") {
      chatApi
        .isAgentRunning(currentContextType, currentContextId)
        .then((processRunning) => {
          if (!processRunning) {
            setAgentRunning(storeContextKey, false);
          }
        })
        .catch(() => {
          // If process check fails, fall back to DB truth
          setAgentRunning(storeContextKey, false);
        });
    }
  }, [activeConversationId, agentRunStatus, isConversationInCurrentContext, setAgentRunning, storeContextKey, currentContextType, currentContextId]);

  // Recovery fallback: keep conversation list fresh while agent is running
  useEffect(() => {
    if (isHistoryMode || !isAgentContext) return undefined;
    if (!isAgentRunning || !selectedTaskId) return undefined;

    const intervalId = setInterval(() => {
      queryClient.invalidateQueries({
        queryKey: chatKeys.conversationList(currentContextType, selectedTaskId),
      });
    }, 2000);

    return () => clearInterval(intervalId);
  }, [currentContextType, isAgentRunning, isAgentContext, isHistoryMode, queryClient, selectedTaskId]);

  // Live updates: poll active conversation while agent is running (store state or backend status).
  // Consolidates two previously-separate intervals that both polled the same query key.
  // Suppressed during active streaming — events (agent:chunk, agent:message_created) already
  // drive UI updates, so polling is redundant and would cause unnecessary refetches.
  useEffect(() => {
    if (!activeConversationId) return undefined;
    if (!isAgentRunning && agentRunStatus !== "running") return undefined;
    if (isGenerating) return undefined;

    const intervalId = setInterval(() => {
      invalidateConversationDataQueries(queryClient, activeConversationId);
    }, 2000);

    return () => clearInterval(intervalId);
  }, [activeConversationId, isAgentRunning, isGenerating, agentRunStatus, queryClient]);

  // If a run is active but no conversation is selected, keep refreshing the list
  useEffect(() => {
    if (ideationSessionId || !selectedTaskId) return undefined;
    if (!isAgentRunning || activeConversationId) return undefined;
    if (!isAgentContext) return undefined;

    const intervalId = setInterval(() => {
      queryClient.invalidateQueries({
        queryKey: chatKeys.conversationList(currentContextType, selectedTaskId),
      });
    }, 2000);

    return () => clearInterval(intervalId);
  }, [activeConversationId, currentContextType, ideationSessionId, isAgentRunning, isAgentContext, queryClient, selectedTaskId]);

  const shouldPollSelectedConversationLiveness =
    isVisible &&
    !isHistoryMode &&
    !!activeConversationId &&
    isConversationInCurrentContext;

  // Selected-conversation liveness poll: process truth is authoritative for
  // recovering lost local status and for clearing genuinely stale non-idle UI.
  // Uses is_agent_running(contextType, contextId) rather than getAgentRunStatus(conversationId)
  // because interactive turns can leave the latest DB run completed while the
  // process is still alive between turns.
  useEffect(() => {
    if (
      isRemoteEnvironmentActive() &&
      !isRemotelyAvailable("is_agent_running")
    )
      return undefined;
    if (!shouldPollSelectedConversationLiveness) return undefined;

    let cancelled = false;
    let inFlight = false;

    const reconcile = () => {
      if (inFlight) return;
      inFlight = true;

      chatApi
        .isAgentRunning(currentContextType, currentContextId)
        .then((running) => {
          if (cancelled) return;
          if (running && !isAgentRunning) {
            setAgentRunning(storeContextKey, true);
            return;
          }
          if (!running && isAgentRunning) {
            setAgentRunning(storeContextKey, false);
          }
        })
        .catch(() => {
          // Silently ignore — primary signal is still Tauri events
        })
        .finally(() => {
          inFlight = false;
        });
    };

    reconcile();
    const intervalId = setInterval(reconcile, 1500);

    return () => {
      cancelled = true;
      clearInterval(intervalId);
    };
  }, [
    shouldPollSelectedConversationLiveness,
    currentContextType,
    currentContextId,
    isAgentRunning,
    storeContextKey,
    setAgentRunning,
  ]);

  // Fast path: reconcile immediately when user re-focuses the app.
  // Covers the most common user-facing case: app was backgrounded/suspended during completion.
  useEffect(() => {
    if (
      isRemoteEnvironmentActive() &&
      !isRemotelyAvailable("is_agent_running")
    )
      return undefined;
    if (!shouldPollSelectedConversationLiveness) return undefined;

    function handleVisibilityChange() {
      if (document.visibilityState === "visible") {
        chatApi
          .isAgentRunning(currentContextType, currentContextId)
          .then((running) => {
            if (running && !isAgentRunning) {
              setAgentRunning(storeContextKey, true);
              return;
            }
            if (!running && isAgentRunning) {
              setAgentRunning(storeContextKey, false);
            }
          })
          .catch(() => {
            // Silently ignore
          });
      }
    }

    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => document.removeEventListener("visibilitychange", handleVisibilityChange);
  }, [
    shouldPollSelectedConversationLiveness,
    currentContextType,
    currentContextId,
    isAgentRunning,
    storeContextKey,
    setAgentRunning,
  ]);

  // Merge watchdog: keep polling task status while in merge flow
  useEffect(() => {
    if (ideationSessionId || !selectedTaskId) return undefined;
    // Poll during approved status too — bridges the gap before pending_merge
    if (!effectiveStatus || (effectiveStatus !== "approved" && !(MERGE_STATUSES as readonly string[]).includes(effectiveStatus))) return undefined;

    const intervalId = setInterval(() => {
      if (projectId) {
        queryClient.invalidateQueries({ queryKey: taskKeys.list(projectId) });
      }
      queryClient.invalidateQueries({ queryKey: taskKeys.detail(selectedTaskId) });
    }, 2000);

    return () => clearInterval(intervalId);
  }, [effectiveStatus, ideationSessionId, projectId, queryClient, selectedTaskId]);

  // Recovery window: brief polling on startup for agent contexts
  useEffect(() => {
    if (ideationSessionId) return undefined;
    if (!selectedTaskId || !isAgentContext) return undefined;

    const intervalId = setInterval(() => {
      if (projectId) {
        queryClient.invalidateQueries({ queryKey: taskKeys.list(projectId) });
      }
      if (selectedTaskId) {
        queryClient.invalidateQueries({ queryKey: taskKeys.detail(selectedTaskId) });
      }
      queryClient.invalidateQueries({
        queryKey: chatKeys.conversationList(currentContextType, selectedTaskId),
      });
      if (activeConversationId) {
        invalidateConversationDataQueries(queryClient, activeConversationId);
      }
    }, 2000);

    const timeoutId = setTimeout(() => {
      clearInterval(intervalId);
    }, 10000);

    return () => {
      clearInterval(intervalId);
      clearTimeout(timeoutId);
    };
  }, [activeConversationId, currentContextType, ideationSessionId, isAgentContext, projectId, queryClient, selectedTaskId]);

  return { isStreamingHydrated };
}
