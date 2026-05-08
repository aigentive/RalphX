/**
 * useAgentEvents hook - Event listener management for agent lifecycle events
 *
 * Handles real-time updates for agent runs across all contexts (ideation, task, review, project).
 * Listens to unified agent:* events and updates query cache and store state accordingly.
 *
 * Uses EventBus abstraction for browser/Tauri compatibility.
 */

import { useEffect, useLayoutEffect, useRef } from "react";
import { useQueryClient, type InfiniteData } from "@tanstack/react-query";
import { toast } from "sonner";
import { chatApi } from "@/api/chat";
import { useEventBus } from "@/providers/EventProvider";
import type {
  ChatMessageResponse,
  ConversationMessagesPageResponse,
} from "@/api/chat";
import {
  mergeConversationProviderMetadata,
  type ChatConversation,
  type ContextType,
} from "@/types/chat-conversation";
import type {
  AgentRunCompletedPayload,
  AgentRunStartedPayload,
} from "@/types/events";
import { extractConversationProviderMetadataFromRunPayload } from "@/types/events";
import { useChatStore } from "@/stores/chatStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { useUiStore } from "@/stores/uiStore";
import { useTeamStore } from "@/stores/teamStore";
import { buildStoreKey, parseStoreKey } from "@/lib/chat-context-registry";
import { buildAgentEventStoreKey } from "@/lib/agent-store-key";
import { findStoreKeyForContextId } from "@/lib/agent-event-utils";
import {
  chatKeys,
  invalidateConversationDataQueries,
} from "./useChat";
import { ideationKeys } from "./useIdeation";
import { conversationStatsKey } from "./useConversationStats";
import type { Unsubscribe } from "@/lib/event-bus";
import { logger } from "@/lib/logger";

type ConversationHistoryCacheData = InfiniteData<ConversationMessagesPageResponse>;

function isConversationHistoryCacheData(
  data: ConversationHistoryCacheData | undefined
): data is ConversationHistoryCacheData {
  return Boolean(data && Array.isArray(data.pages));
}

function updateConversationHistoryConversation(
  data: ConversationHistoryCacheData | undefined,
  updateConversation: (conversation: ChatConversation) => ChatConversation
): ConversationHistoryCacheData | undefined {
  if (!isConversationHistoryCacheData(data)) {
    return data;
  }

  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      conversation: updateConversation(page.conversation),
    })),
  };
}

function appendMessageToConversationHistory(
  data: ConversationHistoryCacheData | undefined,
  message: ChatMessageResponse
): ConversationHistoryCacheData | undefined {
  if (!isConversationHistoryCacheData(data) || data.pages.length === 0) {
    return data;
  }

  if (data.pages.some((page) => page.messages.some((item) => item.id === message.id))) {
    return data;
  }

  return {
    ...data,
    pages: data.pages.map((page, index) => {
      if (index !== 0) {
        return page;
      }
      const messages = replaceMatchingOptimisticMessage(page.messages, message);
      return {
        ...page,
        messages,
        totalMessageCount:
          page.totalMessageCount + (messages.length > page.messages.length ? 1 : 0),
      };
    }),
  };
}

function replaceMatchingOptimisticMessage(
  messages: ChatMessageResponse[],
  message: ChatMessageResponse
) {
  if (messages.some((item) => item.id === message.id)) {
    return messages;
  }

  const optimisticIndex = messages.findIndex(
    (item) =>
      item.id.startsWith("optimistic:") &&
      item.conversationId === message.conversationId &&
      item.role === message.role &&
      item.content === message.content
  );

  if (optimisticIndex === -1) {
    return [...messages, message];
  }

  const nextMessages = [...messages];
  nextMessages[optimisticIndex] = message;
  return nextMessages;
}

function shouldRouteRunStartSelectionToCallerStoreKey(
  callerStoreKey: string | undefined,
  eventStoreKey: string,
  eventContextId: string
): boolean {
  if (!callerStoreKey) {
    return false;
  }
  if (callerStoreKey === eventStoreKey) {
    return true;
  }

  const parsedCallerStoreKey = parseStoreKey(callerStoreKey);
  if (!parsedCallerStoreKey) {
    return false;
  }

  // Project store keys are conversation-scoped. A different project key means a
  // different Agents workspace conversation and must not be overwritten by a
  // run_started event from another workspace.
  if (parsedCallerStoreKey.contextType === "project") {
    return false;
  }

  // Task detail panels can intentionally bridge related task/task_execution
  // slots for the same task id.
  return parsedCallerStoreKey.contextId === eventContextId;
}

/**
 * Hook to manage agent event listeners
 *
 * Subscribes to Tauri events for real-time updates of agent runs.
 * Uses unified agent:* events (Phase 5-6 consolidation).
 *
 * @param activeConversationId - The currently active conversation ID to filter events
 * @param storeKey - Caller-provided store key for scoped setActiveConversation writes.
 *   agent:run_started may use this key only when it targets the same slot or a
 *   compatible task slot; unrelated project conversation events stay isolated.
 */
export function useAgentEvents(activeConversationId: string | null, storeKey?: string) {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  // Ref for storeKey — prevents stale closure writes during teardown/resubscribe window.
  // useLayoutEffect keeps ref synchronised before any Tauri IPC events can arrive.
  const storeKeyRef = useRef(storeKey);
  useLayoutEffect(() => {
    storeKeyRef.current = storeKey;
  }, [storeKey]);
  const setAgentStatus = useChatStore((s) => s.setAgentStatus);
  const updateLastAgentEvent = useChatStore((s) => s.updateLastAgentEvent);
  const deleteQueuedMessage = useChatStore((s) => s.deleteQueuedMessage);
  const queueMessage = useChatStore((s) => s.queueMessage);
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);
  const clearActiveQuestion = useUiStore((s) => s.clearActiveQuestion);
  const clearPendingPlan = useTeamStore((s) => s.clearPendingPlan);

  useEffect(() => {
    const unsubscribes: Unsubscribe[] = [];

    function updateConversationProviderMetadata(args: {
      conversationId: string;
      contextType: ContextType;
      contextId: string;
      providerHarness?: string | null | undefined;
      providerSessionId?: string | null | undefined;
      claudeSessionId?: string | null | undefined;
    }) {
      const {
        conversationId,
        contextType,
        contextId,
        providerHarness,
        providerSessionId,
        claudeSessionId,
      } = args;

      const mergeConversation = (conversation: ChatConversation): ChatConversation =>
        mergeConversationProviderMetadata(conversation, {
          providerHarness,
          providerSessionId,
          claudeSessionId,
        });

      queryClient.setQueryData<{ conversation: ChatConversation; messages: ChatMessageResponse[] }>(
        chatKeys.conversation(conversationId),
        (oldData) => {
          if (!oldData) return oldData;
          return {
            ...oldData,
            conversation: mergeConversation(oldData.conversation),
          };
        }
      );

      queryClient.setQueryData<ConversationHistoryCacheData>(
        chatKeys.conversationHistory(conversationId),
        (oldData) => updateConversationHistoryConversation(oldData, mergeConversation)
      );

      queryClient.setQueryData<ChatConversation[]>(
        chatKeys.conversationList(contextType, contextId),
        (oldData) => {
          if (!oldData) return oldData;
          return oldData.map((conversation) =>
            conversation.id === conversationId
              ? mergeConversation(conversation)
              : conversation
          );
        }
      );
    }

    // Shared cleanup for agent termination (run_completed, stopped, error).
    // Handler-specific logic (updateLastAgentEvent, toast) stays in each caller.
    function handleAgentTermination(storeKey: string, eventContextId: string, conversationId: string) {
      setAgentStatus(storeKey, "idle");
      clearActiveQuestion(eventContextId);
      clearPendingPlan(storeKey);
      queryClient.invalidateQueries({ queryKey: chatKeys.agentRun(conversationId) });
      invalidateConversationDataQueries(queryClient, conversationId);
      queryClient.invalidateQueries({ queryKey: conversationStatsKey(conversationId) });
    }

    function invalidateAgentWorkspacePublishQueries(conversationId: string) {
      queryClient.invalidateQueries({
        queryKey: ["agents", "conversation-workspace", conversationId],
      });
      queryClient.invalidateQueries({
        queryKey: ["agents", "conversation-workspace-freshness", conversationId],
      });
      queryClient.invalidateQueries({
        queryKey: ["agents", "conversation-workspace-publication-events", conversationId],
      });
      queryClient.invalidateQueries({
        queryKey: ["agents", "workspace-diff", conversationId],
      });
      queryClient.invalidateQueries({
        queryKey: ["agents", "workspace-commits", conversationId],
      });
    }

    function workspaceChangedConversationId(payload: unknown): string | null {
      if (!payload || typeof payload !== "object") {
        return null;
      }
      const candidate =
        "conversation_id" in payload
          ? payload.conversation_id
          : "conversationId" in payload
            ? payload.conversationId
            : null;
      return typeof candidate === "string" && candidate.trim()
        ? candidate
        : null;
    }

    // Reverse lookup: when a child verification session terminates, find any parent that has
    // it as activeVerificationChildId and clean up parent's synthetic generating state.
    // Called in all three termination handlers (run_completed, error, stopped).
    function handleChildTerminationReverseLink(eventContextId: string) {
      const ideationState = useIdeationStore.getState();
      const chatState = useChatStore.getState();
      for (const [parentSessionId, childId] of Object.entries(ideationState.activeVerificationChildId)) {
        if (childId !== null && childId === eventContextId) {
          // Clear parent's child ref and set parent to idle
          ideationState.setActiveVerificationChildId(parentSessionId, null);
          chatState.setAgentStatus(buildStoreKey('ideation', parentSessionId), 'idle');
          // Abnormal termination detection: if verification is still in_progress per cached data,
          // log a warning and invalidate so the backend can reconcile.
          const verificationData = queryClient.getQueryData<{ inProgress?: boolean }>(['verification', parentSessionId]);
          if (verificationData?.inProgress) {
            logger.warn(
              `[AgentEvents] Child session ${eventContextId} terminated while verification still in_progress for parent ${parentSessionId} — invalidating verification cache`
            );
            queryClient.invalidateQueries({ queryKey: ['verification', parentSessionId] });
          }
        }
      }
    }

    // Guard: if the parent session has an active verification child, re-assert `generating`
    // instead of clearing to `idle`. The parent's generating state is synthetic — it reflects
    // the child session running. Normal termination events must not clear it prematurely.
    // Uses getState() pattern (not closure-captured values) matching watchdog at line 438.
    function guardedTermination(storeKey: string, eventContextId: string, conversationId: string) {
      // Conversation ID validation: ignore stale run_completed/stopped/error events from
      // previous conversations. Fail-open when activeConvId is null (unmounted panels)
      // to prevent stuck generating states.
      const activeConvId = useChatStore.getState().activeConversationIds[storeKey];
      if (activeConvId != null && conversationId !== activeConvId) {
        logger.warn(
          `[AgentEvents] Ignoring stale termination event: conversation_id=${conversationId} does not match active=${activeConvId} for key=${storeKey}`
        );
        return;
      }

      const parsed = parseStoreKey(storeKey);
      if (parsed?.contextType === "ideation") {
        const activeChildId = useIdeationStore.getState().activeVerificationChildId[parsed.contextId];
        if (activeChildId) {
          // Verification child is running — re-assert generating instead of clearing
          setAgentStatus(storeKey, "generating");
          return;
        }
      }
      handleAgentTermination(storeKey, eventContextId, conversationId);
    }

    // NOTE: Streaming cache updates disabled per user request.
    // Instead of trying to stream text/tool calls character-by-character,
    // we show a typing indicator while the agent is running and only
    // render the final message with proper content_blocks when the run completes.
    //
    // The agent:chunk and agent:tool_call events are still emitted by the backend
    // but we don't use them to update the UI during streaming. This avoids
    // issues with mismatched tool calls/results and partial content.

    // Listen for run started - set agent running state to true and update conversation cache
    // Skip teammate events — useTeamEvents handles those independently
    unsubscribes.push(
      bus.subscribe<AgentRunStartedPayload>("agent:run_started", (payload) => {
        if (payload.teammate_name) return;
        const { context_type, context_id: eventContextId, conversation_id } = payload;

        // Build context key from the event payload
        const eventContextKey = buildAgentEventStoreKey(
          context_type,
          eventContextId,
          conversation_id
        );

        // Update watchdog timestamp only for initial spawns, not queue re-runs.
        // Queue re-runs emit run_started while already in "generating" state —
        // resetting the timestamp there would mask a real stuck-generating condition.
        const currentStatus = useChatStore.getState().agentStatus[eventContextKey];
        if (currentStatus !== "generating") {
          updateLastAgentEvent(eventContextKey);
        }

        // Set agent as generating for this context
        setAgentStatus(eventContextKey, "generating");

        updateConversationProviderMetadata({
          conversationId: conversation_id,
          contextType: context_type as ContextType,
          contextId: eventContextId,
          ...extractConversationProviderMetadataFromRunPayload(payload),
        });

        // Invalidate conversations list to pick up newly created conversation
        // This fixes the race condition where the list query runs before the backend
        // creates the conversation (e.g., when task enters "reviewing" state)
        queryClient.invalidateQueries({
          queryKey: chatKeys.conversationList(context_type as ContextType, eventContextId),
        });
        queryClient.invalidateQueries({
          queryKey: conversationStatsKey(conversation_id),
        });

        // Invalidate ideation session list so hasPendingPrompt badge clears
        // when drain service launches a waiting-for-capacity session
        if (context_type === "ideation") {
          queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
        }

        // If no active conversation is set, set it to this one
        // This handles the case where a new conversation was just created by the backend.
        // When a caller provides a storeKey, only write to it if the event targets that
        // same slot or a compatible task slot. Project keys are conversation-scoped, so
        // blindly writing another project conversation here can poison Agents workspace
        // selection and render the wrong transcript.
        if (!activeConversationId && conversation_id) {
          const callerStoreKey = storeKeyRef.current;
          const activeSelectionStoreKey = callerStoreKey
            ? shouldRouteRunStartSelectionToCallerStoreKey(
                callerStoreKey,
                eventContextKey,
                eventContextId
              )
              ? callerStoreKey
              : null
            : eventContextKey;

          if (activeSelectionStoreKey) {
            setActiveConversation(activeSelectionStoreKey, conversation_id);
          }
        }
      })
    );

    // Listen for message created - optimistically add to cache for user messages only
    // Unified event: agent:message_created (replaces chat:message_created)
    unsubscribes.push(
      bus.subscribe<{
        context_type: string;
        context_id: string;
        conversation_id: string;
        message_id: string;
        role: string;
        content: string;
        created_at?: string;
        metadata?: string | null;
      }>("agent:message_created", (payload) => {
        const { conversation_id, message_id, role, content, created_at } = payload;

        // Heartbeat: update watchdog timestamp on every message (active event flow).
        // Prevents watchdog from firing during normal streaming bursts.
        const msgContextKey = buildAgentEventStoreKey(
          payload.context_type,
          payload.context_id,
          conversation_id
        );
        updateLastAgentEvent(msgContextKey);

        // Always invalidate the conversation query for this message's conversation.
        // This handles both lead and teammate conversations — teammate messages
        // have their own conversation_id that won't match activeConversationId.
        if (role === "user" && conversation_id === activeConversationId) {
          const newMessage: ChatMessageResponse = {
            id: message_id,
            conversationId: conversation_id,
            sessionId: null,
            projectId: null,
            taskId: null,
            role: role as "user" | "assistant" | "system",
            content: content || "",
            metadata: payload.metadata ?? null,
            parentMessageId: null,
            createdAt: created_at ?? new Date().toISOString(),
            toolCalls: null,
            contentBlocks: null,
            sender: null,
          };

          // Optimistic append for user messages in the active conversation only
          queryClient.setQueryData<{ conversation: ChatConversation; messages: ChatMessageResponse[] }>(
            chatKeys.conversation(activeConversationId),
            (oldData) => {
              if (!oldData) return oldData;
              return {
                ...oldData,
                messages: replaceMatchingOptimisticMessage(
                  oldData.messages ?? [],
                  newMessage
                ),
              };
            }
          );
          queryClient.setQueryData<ConversationHistoryCacheData>(
            chatKeys.conversationHistory(activeConversationId),
            (oldData) => appendMessageToConversationHistory(oldData, newMessage)
          );
        } else if (conversation_id !== activeConversationId) {
          // Non-active conversation (e.g. teammate messages): invalidate to refetch from DB.
          // Active-conversation assistant messages are handled exclusively by useChatEvents
          // to avoid duplicate DB refetches that cause visual artifacts during streaming.
          invalidateConversationDataQueries(queryClient, conversation_id);
        }
        queryClient.invalidateQueries({
          queryKey: conversationStatsKey(conversation_id),
        });
      })
    );

    // Listen for run completion
    // Unified event: agent:run_completed (replaces chat:run_completed)
    // Skip teammate events — useTeamEvents handles those independently
    unsubscribes.push(
      bus.subscribe<AgentRunCompletedPayload>("agent:run_completed", (payload) => {
        if (payload.teammate_name) return;
        const { conversation_id, context_type, context_id: eventContextId } = payload;

        // Build context key from the event payload
        const eventContextKey = buildAgentEventStoreKey(
          context_type,
          eventContextId,
          conversation_id
        );

        // Final heartbeat — clears the "stuck" condition before transitioning to idle.
        updateLastAgentEvent(eventContextKey);

        updateConversationProviderMetadata({
          conversationId: conversation_id,
          contextType: context_type as ContextType,
          contextId: eventContextId,
          ...extractConversationProviderMetadataFromRunPayload(payload),
        });

        guardedTermination(eventContextKey, eventContextId, conversation_id);
        handleChildTerminationReverseLink(eventContextId);
        if (context_type === "project") {
          invalidateAgentWorkspacePublishQueries(conversation_id);
        }

        // NOTE: Queue processing is now handled by the BACKEND
        // The backend automatically processes queued messages via --resume
        // when a run completes. We listen for agent:queue_sent to update UI.
      })
    );

    // Listen for turn completion (interactive mode - agent still alive)
    // Sets status to "waiting_for_input" so the UI shows the agent is idle between turns
    // (not "generating"), while the process remains alive.
    // Skip teammate events — useTeamEvents handles those independently
    unsubscribes.push(
      bus.subscribe<AgentRunCompletedPayload>("agent:turn_completed", (payload) => {
        if (payload.teammate_name) return;
        const { conversation_id, context_type, context_id: eventContextId } = payload;

        // Agent is still alive but waiting for user input — transition from "generating" to "waiting_for_input"
        const eventContextKey = buildAgentEventStoreKey(
          context_type,
          eventContextId,
          conversation_id
        );

        // Heartbeat: agent is alive between turns, reset watchdog timer.
        updateLastAgentEvent(eventContextKey);

        updateConversationProviderMetadata({
          conversationId: conversation_id,
          contextType: context_type as ContextType,
          contextId: eventContextId,
          ...extractConversationProviderMetadataFromRunPayload(payload),
        });

        // Guard: if parent ideation session has active verification child, maintain
        // generating instead of transitioning to waiting_for_input. Child's generating
        // state must dominate parent's waiting_for_input (PO2).
        const parsedKey = parseStoreKey(eventContextKey);
        if (parsedKey?.contextType === "ideation") {
          const activeChildId = useIdeationStore.getState().activeVerificationChildId[parsedKey.contextId];
          if (activeChildId) {
            setAgentStatus(eventContextKey, "generating");
          } else {
            setAgentStatus(eventContextKey, "waiting_for_input");
          }
        } else {
          setAgentStatus(eventContextKey, "waiting_for_input");
        }

        // Invalidate using conversation_id from payload to avoid stale closure mismatch
        queryClient.invalidateQueries({
          queryKey: chatKeys.agentRun(conversation_id),
        });
        // Active-conversation assistant turns already invalidate/refetch via
        // agent:message_created in useChatEvents. Skipping the second active
        // conversation invalidation avoids overlapping layout/scroll churn
        // during finalization while preserving refetches for non-active tabs.
        if (conversation_id !== activeConversationId) {
          invalidateConversationDataQueries(queryClient, conversation_id);
        }
        queryClient.invalidateQueries({
          queryKey: conversationStatsKey(conversation_id),
        });
      })
    );

    // Listen for queue_sent - backend notifies us when it sends a queued message
    // This allows us to update the optimistic UI by removing the sent message
    // Since frontend and backend use the same ID, we can match exactly by ID
    unsubscribes.push(
      bus.subscribe<{
        message_id: string;
        conversation_id: string;
        context_type: string;
        context_id: string;
      }>("agent:queue_sent", (payload) => {
        const { message_id, context_type, context_id: eventContextId } = payload;

        // Build context key from the event payload - unified queue with context-aware keys
        const eventContextKey = buildAgentEventStoreKey(
          context_type,
          eventContextId,
          payload.conversation_id
        );
        // Remove from frontend optimistic queue by exact ID match
        deleteQueuedMessage(eventContextKey, message_id);
      })
    );

    // Listen for message_queued - backend notifies us when a message enters the queue (Gate 2)
    // Idempotent: queueMessage has a duplicate-ID guard, so calling it twice with the same ID is safe
    unsubscribes.push(
      bus.subscribe<{
        message_id: string;
        content: string;
        context_type: string;
        context_id: string;
        conversation_id?: string | null;
        created_at: string;
      }>("agent:message_queued", (payload) => {
        const { message_id, content, context_type, context_id: eventContextId } = payload;

        const eventContextKey = buildAgentEventStoreKey(
          context_type,
          eventContextId,
          payload.conversation_id
        );
        queueMessage(eventContextKey, content, message_id);
      })
    );

    unsubscribes.push(
      bus.subscribe<unknown>("agent:workspace_changed", (payload) => {
        const conversationId = workspaceChangedConversationId(payload);
        if (conversationId) {
          invalidateAgentWorkspacePublishQueries(conversationId);
        }
      })
    );

    // Listen for agent stopped - defensive cleanup if agent:run_completed emission regresses.
    // Backend emits agent:stopped immediately on SIGTERM, before agent:run_completed.
    // This ensures running state clears even if the subsequent run_completed is lost.
    // Skip teammate events — useTeamEvents handles those independently
    unsubscribes.push(
      bus.subscribe<{
        context_type: string;
        context_id: string;
        conversation_id: string;
        agent_run_id: string;
        teammate_name?: string | null;
      }>("agent:stopped", (payload) => {
        if (payload.teammate_name) return;
        const { conversation_id, context_type, context_id: eventContextId } = payload;

        const eventContextKey = buildAgentEventStoreKey(
          context_type,
          eventContextId,
          conversation_id
        );

        guardedTermination(eventContextKey, eventContextId, conversation_id);
        handleChildTerminationReverseLink(eventContextId);
        if (context_type === "project") {
          invalidateAgentWorkspacePublishQueries(conversation_id);
        }
      })
    );

    // Listen for agent errors
    // Unified event: agent:error
    // Skip teammate events — useTeamEvents handles those independently
    unsubscribes.push(
      bus.subscribe<{
        context_type: string;
        context_id: string;
        conversation_id: string;
        error: string;
        teammate_name?: string | null;
      }>("agent:error", (payload) => {
        if (payload.teammate_name) return;
        const { conversation_id, context_type, context_id: eventContextId } = payload;

        // Build context key from the event payload
        const eventContextKey = buildAgentEventStoreKey(
          context_type,
          eventContextId,
          conversation_id
        );

        guardedTermination(eventContextKey, eventContextId, conversation_id);
        handleChildTerminationReverseLink(eventContextId);
        if (context_type === "project") {
          invalidateAgentWorkspacePublishQueries(conversation_id);
        }

        // Show error toast for agent failures in execution contexts
        if (["task_execution", "review", "merge"].includes(context_type)) {
          const contextLabel = context_type === "task_execution" ? "Worker"
            : context_type === "review" ? "Reviewer"
            : "Merger";
          const errorMsg = payload.error ? String(payload.error).slice(0, 150) : "Agent process exited unexpectedly";
          toast.error(`${contextLabel} agent error: ${errorMsg}`, { id: `error:${eventContextKey}`, duration: 8000 });
        }
      })
    );

    // Listen for session recovery events
    unsubscribes.push(
      bus.subscribe<{
        conversation_id: string;
        message: string;
      }>("agent:session_recovered", (payload) => {
        console.info("[session-recovery]", payload.message);
      })
    );

    // Listen for synthetic heartbeat events emitted by backend during PID-alive bypass.
    // Refreshes lastAgentEventTimestamp so the frontend watchdog doesn't false-trigger
    // while the backend keeps the agent alive during buffered-stdout commands.
    unsubscribes.push(
      bus.subscribe<{
        conversation_id: string;
        context_id: string;
        reason: string;
        pid?: number;
      }>("agent:heartbeat", (payload) => {
        const key = findStoreKeyForContextId(payload.context_id);
        if (key) updateLastAgentEvent(key);
      })
    );

    // Listen for task lifecycle events to reset watchdog timer.
    // These are Tauri events distinct from the streaming tool call events in useChatEvents.
    unsubscribes.push(
      bus.subscribe<{
        conversation_id: string;
        context_id: string;
      }>("agent:task_started", (payload) => {
        const key = findStoreKeyForContextId(payload.context_id);
        if (key) updateLastAgentEvent(key);
      })
    );

    unsubscribes.push(
      bus.subscribe<{
        conversation_id: string;
        context_id: string;
      }>("agent:task_completed", (payload) => {
        const key = findStoreKeyForContextId(payload.context_id);
        if (key) updateLastAgentEvent(key);
      })
    );

    return () => {
      unsubscribes.forEach((unsub) => unsub());
    };
  }, [bus, activeConversationId, storeKey, queryClient, setAgentStatus, updateLastAgentEvent, deleteQueuedMessage, queueMessage, setActiveConversation, clearActiveQuestion, clearPendingPlan]);

  // Global singleton watchdog — defense-in-depth for stuck generating state.
  // If the backend misses run_completed for any reason, this forces idle after
  // 5 minutes of no agent events for a context still in "generating" state.
  // Runs once per hook mount (empty deps) and checks all contexts every 30s.
  useEffect(() => {
    const WATCHDOG_TIMEOUT_MS = 300_000;     // 5 minutes
    const TOOL_CALL_MAX_DURATION_MS = 600_000; // 10 minutes per-tool ceiling
    const TOOL_CALL_GRACE_MS = 5_000;        // 5s grace after last tool completion
    const CHECK_INTERVAL_MS = 30_000;        // Check every 30s
    const projectLivenessChecksInFlight = new Set<string>();

    const interval = setInterval(() => {
      const now = Date.now();
      const chatState = useChatStore.getState();
      const ideationState = useIdeationStore.getState();

      for (const [key, status] of Object.entries(chatState.agentStatus)) {
        if (status !== "generating") continue;
        const lastEvent = chatState.lastAgentEventTimestamp[key] ?? 0;
        if (now - lastEvent <= WATCHDOG_TIMEOUT_MS) continue;

        // Check 1: Active tool calls with per-tool ceiling (10 min)
        const toolCalls = chatState.toolCallStartTimes[key];
        if (toolCalls && Object.keys(toolCalls).length > 0) {
          const hasActiveToolCall = (Object.values(toolCalls) as number[]).some(
            (startTime) => now - startTime <= TOOL_CALL_MAX_DURATION_MS
          );
          if (hasActiveToolCall) continue; // suppress — tool actively running
        }

        // Check 2: Grace period after last tool completion (5s)
        const lastCompletion = chatState.lastToolCallCompletionTimestamp[key] ?? 0;
        if (lastCompletion > 0 && now - lastCompletion < TOOL_CALL_GRACE_MS) continue;

        // Check 3: Verification child session running (synthetic generating)
        const parsedKey = parseStoreKey(key);
        if (parsedKey?.contextType === "ideation") {
          if (ideationState.activeVerificationChildId[parsedKey.contextId]) continue;
        }

        // Project agent conversations are runtime-keyed by conversation id. Before
        // clearing their UI status, ask the backend registry whether that process
        // is still alive; otherwise long but healthy Agents runs can lose Stop
        // state until the user reselects the conversation.
        if (parsedKey?.contextType === "project") {
          if (projectLivenessChecksInFlight.has(key)) continue;
          projectLivenessChecksInFlight.add(key);
          void chatApi
            .isAgentRunning("project", parsedKey.contextId)
            .then((isRunning) => {
              const latestState = useChatStore.getState();
              if (latestState.agentStatus[key] !== "generating") return;
              if (isRunning) {
                latestState.updateLastAgentEvent(key);
                return;
              }
              latestState.clearToolCallStartTimes(key);
              latestState.setAgentStatus(key, "idle");
            })
            .catch(() => {
              const latestState = useChatStore.getState();
              if (latestState.agentStatus[key] !== "generating") return;
              latestState.clearToolCallStartTimes(key);
              latestState.setAgentStatus(key, "idle");
            })
            .finally(() => {
              projectLivenessChecksInFlight.delete(key);
            });
          continue;
        }

        // Genuinely stalled — silent reset (no toast — bad UX)
        chatState.clearToolCallStartTimes(key); // prevent contradictory "Tool active" badge
        chatState.setAgentStatus(key, "idle");
      }
    }, CHECK_INTERVAL_MS);

    return () => clearInterval(interval);
  }, []); // Empty deps — runs once globally, reads fresh state via getState()
}
