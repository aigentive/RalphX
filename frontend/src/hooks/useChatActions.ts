/**
 * useChatActions — Unified message handling for all chat panels
 *
 * Merges:
 * - useIntegratedChatHandlers (review mode send, ideation auto-naming, execution recovery)
 * - Action parts of useChatPanelHandlers (send, queue, stop, edit, delete)
 *
 * Uses contextType from registry instead of mode booleans.
 */

import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useChatStore } from "@/stores/chatStore";
import { chatApi, stopAgent } from "@/api/chat";
import { recoverTaskExecution } from "@/api/recovery";
import {
  addOptimisticUserMessageToConversationCache,
  chatKeys,
  invalidateConversationDataQueries,
  removeOptimisticMessageFromConversationCache,
} from "@/hooks/useChat";
import { ideationApi } from "@/api/ideation";
import { serializeComposerReferencesMetadata } from "@/components/Chat/MessageReferences.parse";
import { extractErrorMessage } from "@/lib/errors";
import { logger } from "@/lib/logger";
import type { ContextType } from "@/types/chat-conversation";
import type {
  ComposerArtifactReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
  SendAgentMessageOptions,
  SendAgentMessageResult,
} from "@/api/chat";

// ============================================================================
// Types
// ============================================================================

interface UseChatActionsProps {
  /** Resolved context type (from registry or caller) */
  contextType: ContextType;
  /** Context entity ID (task ID, session ID, or project ID) */
  contextId: string;
  /** Backend queue/process ID. Project-agent conversations use conversation ID here. */
  queueContextId?: string | undefined;
  /** Store context key for queue/agent state operations */
  storeContextKey: string;
  /** Selected task ID (for execution recovery) */
  selectedTaskId: string | undefined;
  /** Ideation session ID (for auto-naming) */
  ideationSessionId: string | undefined;
  /** Send message mutation from useChat or useTaskChat */
  sendMessage: {
    isPending: boolean;
    mutateAsync: (params: {
      content: string;
      attachmentIds?: string[];
      target?: string;
      composerArtifactReferences?: ComposerArtifactReference[];
      composerProjectReferences?: ComposerProjectReference[];
      composerIntegrationReferences?: ComposerIntegrationReference[];
    }) => Promise<SendAgentMessageResult>;
  };
  /** Current visible conversation ID, used by direct review/merge sends for immediate local echo. */
  activeConversationId?: string | null | undefined;
  /** Current message count (for first-message detection in ideation) */
  messageCount?: number;
  /** Explicit send options used by externally-owned session lists. */
  sendOptions?: SendAgentMessageOptions | undefined;
  /** Optional callback after a user message is accepted by the backend. */
  onUserMessageSent?: ((payload: {
    content: string;
    result: SendAgentMessageResult;
  }) => void | Promise<void>) | undefined;
}

// ============================================================================
// Hook
// ============================================================================

export function useChatActions({
  contextType,
  contextId,
  queueContextId,
  storeContextKey,
  selectedTaskId,
  ideationSessionId,
  sendMessage,
  activeConversationId,
  messageCount = 0,
  sendOptions,
  onUserMessageSent,
}: UseChatActionsProps) {
  const queryClient = useQueryClient();
  const queueMessage = useChatStore((s) => s.queueMessage);
  const deleteQueuedMessage = useChatStore((s) => s.deleteQueuedMessage);
  const startEditingQueuedMessage = useChatStore((s) => s.startEditingQueuedMessage);
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);
  const setAgentRunning = useChatStore((s) => s.setAgentRunning);
  const setSending = useChatStore((s) => s.setSending);
  const backendQueueContextId = queueContextId ?? contextId;

  const reportSendFailure = useCallback((err: unknown) => {
    toast.error("Failed to send message", {
      description: extractErrorMessage(err, "The agent runtime could not start."),
      duration: 10000,
    });
  }, []);

  const queueAcceptedMessage = useCallback(
    (content: string, queuedMessageId: string, attachmentIds?: string[]) => {
      if (attachmentIds !== undefined && attachmentIds.length > 0) {
        queueMessage(storeContextKey, content, queuedMessageId, attachmentIds);
        return;
      }
      queueMessage(storeContextKey, content, queuedMessageId);
    },
    [queueMessage, storeContextKey]
  );

  // ── Send ─────────────────────────────────────────────────────────
  const handleSend = useCallback(
    async (
      content: string,
      attachmentIds?: string[],
      target?: string,
      composerOptions?: {
        projectReferences?: ComposerProjectReference[];
        integrationReferences?: ComposerIntegrationReference[];
        artifactReferences?: ComposerArtifactReference[];
      },
    ) => {
      if (!content.trim() || sendMessage.isPending) return;

      // Capture first message state before sending (for auto-naming trigger)
      const isFirstIdeationMessage = ideationSessionId && messageCount === 0;
      let sentResult: SendAgentMessageResult | null = null;
      let optimisticMessage:
        | { conversationId: string; messageId: string }
        | null = null;

      try {
        // Agent side-panels use context-specific conversations. Review and merge must
        // bypass the generic task-detail mutation so steering messages reach the
        // active reviewer/merger process instead of a plain task chat.
        if (contextType === "review" || contextType === "merge") {
          const agentContextId = selectedTaskId ?? contextId;
          setSending(storeContextKey, true);
          try {
            if (activeConversationId && !target) {
              const referenceMetadata = serializeComposerReferencesMetadata({
                projectReferences: composerOptions?.projectReferences,
                integrationReferences: composerOptions?.integrationReferences,
                artifactReferences: composerOptions?.artifactReferences,
              });
              const message = referenceMetadata
                ? addOptimisticUserMessageToConversationCache(
                    queryClient,
                    activeConversationId,
                    content,
                    { metadata: referenceMetadata },
                  )
                : addOptimisticUserMessageToConversationCache(
                    queryClient,
                    activeConversationId,
                    content,
                  );
              optimisticMessage = {
                conversationId: activeConversationId,
                messageId: message.id,
              };
            }
            const result = await chatApi.sendAgentMessage(
              contextType,
              agentContextId,
              content,
              attachmentIds,
              target,
              composerOptions?.projectReferences?.length ||
                composerOptions?.integrationReferences?.length ||
                composerOptions?.artifactReferences?.length
                ? {
                    ...(composerOptions?.projectReferences?.length
                      ? {
                          composerProjectReferences:
                            composerOptions.projectReferences,
                        }
                      : {}),
                    ...(composerOptions?.integrationReferences?.length
                      ? {
                          composerIntegrationReferences:
                            composerOptions.integrationReferences,
                        }
                      : {}),
                    ...(composerOptions?.artifactReferences?.length
                      ? {
                          composerArtifactReferences:
                            composerOptions.artifactReferences,
                        }
                      : {}),
                  }
                : undefined,
            );
            sentResult = result;

            queryClient.invalidateQueries({
              queryKey: chatKeys.conversationList(contextType, agentContextId),
            });

            if (result.wasQueued && result.queuedMessageId != null) {
              queueAcceptedMessage(content, result.queuedMessageId, attachmentIds);
            }

            if (result.conversationId) {
              if (!optimisticMessage) {
                invalidateConversationDataQueries(queryClient, result.conversationId);
              }
              if (result.isNewConversation) {
                setActiveConversation(storeContextKey, result.conversationId);
              }
            }
          } finally {
            setSending(storeContextKey, false);
          }
        } else {
          const params: {
            content: string;
            attachmentIds?: string[];
            target?: string;
            composerArtifactReferences?: ComposerArtifactReference[];
            composerProjectReferences?: ComposerProjectReference[];
            composerIntegrationReferences?: ComposerIntegrationReference[];
          } = { content };
          if (attachmentIds !== undefined) {
            params.attachmentIds = attachmentIds;
          }
          if (target !== undefined) {
            params.target = target;
          }
          if (composerOptions?.projectReferences?.length) {
            params.composerProjectReferences = composerOptions.projectReferences;
          }
          if (composerOptions?.integrationReferences?.length) {
            params.composerIntegrationReferences =
              composerOptions.integrationReferences;
          }
          if (composerOptions?.artifactReferences?.length) {
            params.composerArtifactReferences =
              composerOptions.artifactReferences;
          }
          const result = await sendMessage.mutateAsync(params);
          sentResult = result;
          if (result.wasQueued && result.queuedMessageId != null) {
            queueAcceptedMessage(content, result.queuedMessageId, attachmentIds);
          }
          if (
            contextType === "ideation" &&
            ideationSessionId &&
            !target &&
            result.conversationId &&
            (result.isNewConversation || result.queuedAsPending)
          ) {
            setActiveConversation(storeContextKey, result.conversationId);
          }
          if (
            contextType === "ideation" &&
            ideationSessionId &&
            !target &&
            result.queuedAsPending
          ) {
            queryClient.setQueryData(
              ["child-session-status", ideationSessionId],
              {
                session_id: ideationSessionId,
                title: null,
                agent_state: { estimated_status: "idle" as const },
                recent_messages: [],
                pending_initial_prompt: content,
                lastEffectiveModel: null,
              },
            );
          }
        }

        // Trigger session auto-naming on first ideation message (fire-and-forget)
        if (isFirstIdeationMessage) {
          ideationApi.sessions.spawnSessionNamer(ideationSessionId, content).catch(() => {
            // Silently ignore — session namer is optional
          });
        }
        if (sentResult) {
          void onUserMessageSent?.({ content, result: sentResult });
        }
      } catch (err) {
        if (optimisticMessage) {
          removeOptimisticMessageFromConversationCache(
            queryClient,
            optimisticMessage.conversationId,
            optimisticMessage.messageId
          );
        }
        reportSendFailure(err);
        // Reset agent running state on error for the correct store context key.
        // Covers review, task_execution, merge, and ideation (idempotent for ideation
        // where storeContextKey and useChat's contextKey happen to match).
        setAgentRunning(storeContextKey, false);
      }
    },
    [sendMessage, contextType, contextId, selectedTaskId, storeContextKey, setAgentRunning, setSending, setActiveConversation, queryClient, ideationSessionId, messageCount, queueAcceptedMessage, onUserMessageSent, reportSendFailure, activeConversationId]
  );

  // ── Stop Agent ───────────────────────────────────────────────────
  const handleStopAgent = useCallback(async () => {
    // Always attempt immediate run cancellation
    try {
      await stopAgent(contextType, backendQueueContextId);
    } catch (err) {
      logger.warn("[chat] Failed to stop agent", {
        contextType,
        contextId,
        queueContextId: backendQueueContextId,
        error: err,
      });
    }

    // For execution mode, also run recovery so task status reconciles
    if (contextType === "task_execution" && selectedTaskId) {
      try {
        await recoverTaskExecution(selectedTaskId);
      } catch (err) {
        logger.warn("[chat] Failed to recover task execution after stop", { taskId: selectedTaskId, error: err });
      }
    }
  }, [contextType, contextId, backendQueueContextId, selectedTaskId]);

  // ── Delete Queued Message ────────────────────────────────────────
  const handleDeleteQueuedMessage = useCallback(
    async (messageId: string) => {
      // Delete from local store immediately (optimistic)
      deleteQueuedMessage(storeContextKey, messageId);

      // Delete from backend using the same ID
      try {
        await chatApi.deleteQueuedAgentMessage(contextType, backendQueueContextId, messageId);
      } catch {
        // Silently ignore — local state already updated
      }
    },
    [deleteQueuedMessage, storeContextKey, contextType, backendQueueContextId]
  );

  // ── Send Queued Message Now ─────────────────────────────────────
  const handleSendQueuedMessageNow = useCallback(
    async (messageId: string, content?: string, attachmentIds?: string[]) => {
      deleteQueuedMessage(storeContextKey, messageId);
      setSending(storeContextKey, true);

      try {
        const result = await chatApi.sendQueuedAgentMessageNow(
          contextType,
          backendQueueContextId,
          messageId
        );

        if (result.wasQueued && result.queuedMessageId != null && content) {
          queueAcceptedMessage(content, result.queuedMessageId, attachmentIds);
        } else if (!result.wasQueued) {
          setAgentRunning(storeContextKey, true);
        }
      } catch (err) {
        if (content) {
          queueAcceptedMessage(content, messageId, attachmentIds);
        }
        reportSendFailure(err);
      } finally {
        setSending(storeContextKey, false);
      }
    },
    [
      backendQueueContextId,
      contextType,
      deleteQueuedMessage,
      queueAcceptedMessage,
      reportSendFailure,
      setAgentRunning,
      setSending,
      storeContextKey,
    ]
  );

  // ── Edit Queued Message ──────────────────────────────────────────
  const handleEditQueuedMessage = useCallback(
    async (messageId: string, newContent: string, attachmentIds?: string[]) => {
      // Delete old message from backend
      try {
        await chatApi.deleteQueuedAgentMessage(contextType, backendQueueContextId, messageId);
      } catch {
        // Silently ignore
      }

      // Delete from local store
      deleteQueuedMessage(storeContextKey, messageId);

      // Send the edited content via sendAgentMessage (delete-before-send pattern)
      setSending(storeContextKey, true);
      try {
        const result = await chatApi.sendAgentMessage(
          contextType,
          contextId,
          newContent,
          attachmentIds !== undefined && attachmentIds.length > 0 ? attachmentIds : undefined,
          undefined,
          sendOptions
        );
        if (result.wasQueued && result.queuedMessageId != null) {
          queueAcceptedMessage(newContent, result.queuedMessageId, attachmentIds);
        }
      } catch (err) {
        reportSendFailure(err);
      } finally {
        setSending(storeContextKey, false);
      }
    },
    [
      deleteQueuedMessage,
      queueAcceptedMessage,
      contextType,
      contextId,
      backendQueueContextId,
      storeContextKey,
      setSending,
      sendOptions,
      reportSendFailure,
    ]
  );

  // ── Edit Last Queued ─────────────────────────────────────────────
  const handleEditLastQueued = useCallback(
    (queuedMessages: Array<{ id: string }>) => {
      const lastMessage = queuedMessages[queuedMessages.length - 1];
      if (!lastMessage) return;
      startEditingQueuedMessage(storeContextKey, lastMessage.id);
    },
    [startEditingQueuedMessage, storeContextKey]
  );

  return {
    handleSend,
    handleStopAgent,
    handleDeleteQueuedMessage,
    handleSendQueuedMessageNow,
    handleEditQueuedMessage,
    handleEditLastQueued,
  };
}
