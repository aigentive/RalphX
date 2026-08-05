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
import { selectActiveAgentRunId, useChatStore } from "@/stores/chatStore";
import { chatApi, RemoteQueuedMessageSendError, stopAgent } from "@/api/chat";
import { recoverTaskExecution } from "@/api/recovery";
import {
  addOptimisticUserMessageToConversationCache,
  chatKeys,
  invalidateConversationDataQueries,
  removeOptimisticMessageFromConversationCache,
} from "@/hooks/useChat";
import { ideationApi } from "@/api/ideation";
import {
  serializeComposerReferencesMetadata,
  type MessageFolderReference,
} from "@/components/Chat/MessageReferences.parse";
import { extractErrorMessage } from "@/lib/errors";
import { logger } from "@/lib/logger";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";
import { remoteErrorBannerProps } from "@/lib/remote/agent-gate";
import { isPersonaUnavailableError } from "@/lib/personaErrors";
import { isMessageDeliveredNotPersistedError } from "@/lib/sendDeliveryErrors";
import type { ContextType } from "@/types/chat-conversation";
import type {
  ComposerArtifactReference,
  ComposerExcerptReference,
  CapabilityIntent,
  ComposerIntegrationReference,
  ComposerProjectReference,
  ComposerSelectionSnapshot,
  SendAgentMessageOptions,
  SendAgentMessageResult,
  TeamIntent,
  TeamMessageTarget,
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
      composerArtifactReferences?: ComposerArtifactReference[];
      composerProjectReferences?: ComposerProjectReference[];
      composerIntegrationReferences?: ComposerIntegrationReference[];
      composerExcerptReferences?: ComposerExcerptReference[];
      capabilityIntent?: CapabilityIntent | null;
      composerSelectionSnapshot?: ComposerSelectionSnapshot;
      teamIntent?: TeamIntent | null;
      teamMessageTarget?: TeamMessageTarget | null;
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
    composerIntegrationReferences?: ComposerIntegrationReference[];
  }) => void | Promise<void>) | undefined;
  /** Receives persona binding failures for inline, recoverable composer UI. */
  onPersonaUnavailable?: ((message: string) => void) | undefined;
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
  onPersonaUnavailable,
}: UseChatActionsProps) {
  const queryClient = useQueryClient();
  const queueMessage = useChatStore((s) => s.queueMessage);
  const deleteQueuedMessage = useChatStore((s) => s.deleteQueuedMessage);
  const setQueuedMessages = useChatStore((s) => s.setQueuedMessages);
  const startEditingQueuedMessage = useChatStore((s) => s.startEditingQueuedMessage);
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);
  const setAgentRunning = useChatStore((s) => s.setAgentRunning);
  const setSending = useChatStore((s) => s.setSending);
  const activeAgentRunId = useChatStore(selectActiveAgentRunId(storeContextKey));
  const backendQueueContextId = queueContextId ?? contextId;

  const hydrateRemoteQueue = useCallback(async () => {
    const messages = await chatApi.listRemoteQueuedAgentMessages(
      backendQueueContextId,
    );
    setQueuedMessages(
      storeContextKey,
      messages.map((message) => ({
        id: message.id,
        content: message.content,
        createdAt: message.createdAt,
        isEditing: message.isEditing,
        source: "backend" as const,
        attachmentIds: [...(message.attachmentIds ?? [])],
        ...(message.composerSelectionSnapshot
          ? { composerSelectionSnapshot: message.composerSelectionSnapshot }
          : {}),
      })),
    );
  }, [backendQueueContextId, setQueuedMessages, storeContextKey]);

  const reportSendFailure = useCallback((err: unknown) => {
    const message = extractErrorMessage(err, "The agent runtime could not start.");
    if (isPersonaUnavailableError(message)) {
      onPersonaUnavailable?.(message);
      return;
    }
    // The agent already received this turn — saying "failed to send" would be a lie.
    if (isMessageDeliveredNotPersistedError(message)) {
      toast.warning("Message sent, but not saved", {
        description:
          "The agent received your message and is replying, but it could not be added to the transcript.",
        duration: 10000,
      });
      return;
    }
    toast.error("Failed to send message", {
      description: message,
      duration: 10000,
    });
  }, [onPersonaUnavailable]);

  const queueAcceptedMessage = useCallback(
    (
      content: string,
      queuedMessageId: string,
      attachmentIds?: string[],
      selectionSnapshot?: ComposerSelectionSnapshot,
    ) => {
      if (attachmentIds !== undefined && attachmentIds.length > 0) {
        if (selectionSnapshot) {
          queueMessage(
            storeContextKey,
            content,
            queuedMessageId,
            attachmentIds,
            selectionSnapshot,
          );
        } else {
          queueMessage(
            storeContextKey,
            content,
            queuedMessageId,
            attachmentIds,
          );
        }
        return;
      }
      if (selectionSnapshot) {
        queueMessage(
          storeContextKey,
          content,
          queuedMessageId,
          undefined,
          selectionSnapshot,
        );
      } else {
        queueMessage(storeContextKey, content, queuedMessageId);
      }
    },
    [queueMessage, storeContextKey]
  );

  // ── Send ─────────────────────────────────────────────────────────
  const handleSend = useCallback(
    async (
      content: string,
      attachmentIds?: string[],
      composerOptions?: {
        folderReferences?: MessageFolderReference[];
        projectReferences?: ComposerProjectReference[];
        integrationReferences?: ComposerIntegrationReference[];
        artifactReferences?: ComposerArtifactReference[];
        excerptReferences?: ComposerExcerptReference[];
        capabilityIntent?: CapabilityIntent | null;
        selectionSnapshot?: ComposerSelectionSnapshot;
        teamIntent?: TeamIntent | null;
        teamMessageTarget?: TeamMessageTarget | null;
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
        if (
          contextType === "review" ||
          contextType === "merge" ||
          contextType === "branch_update"
        ) {
          const agentContextId = selectedTaskId ?? contextId;
          setSending(storeContextKey, true);
          try {
            if (activeConversationId) {
              const referenceMetadata = serializeComposerReferencesMetadata({
                folderReferences: composerOptions?.folderReferences,
                projectReferences: composerOptions?.projectReferences,
                integrationReferences: composerOptions?.integrationReferences,
                artifactReferences: composerOptions?.artifactReferences,
                selectionSnapshot: composerOptions?.selectionSnapshot,
                excerptReferences: composerOptions?.excerptReferences,
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
            const directSendOptions =
              composerOptions?.projectReferences?.length ||
              composerOptions?.integrationReferences?.length ||
              composerOptions?.artifactReferences?.length ||
              composerOptions?.excerptReferences?.length ||
              composerOptions?.capabilityIntent ||
              composerOptions?.selectionSnapshot ||
              composerOptions?.teamIntent ||
              composerOptions?.teamMessageTarget
                ? {
                    ...(composerOptions?.capabilityIntent
                      ? { capabilityIntent: composerOptions.capabilityIntent }
                      : {}),
                    ...(composerOptions?.teamIntent
                      ? { teamIntent: composerOptions.teamIntent }
                      : {}),
                    ...(composerOptions?.teamMessageTarget
                      ? { teamMessageTarget: composerOptions.teamMessageTarget }
                      : {}),
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
                    ...(composerOptions?.selectionSnapshot
                      ? {
                          composerSelectionSnapshot:
                            composerOptions.selectionSnapshot,
                        }
                      : {}),
                    ...(composerOptions?.excerptReferences?.length
                      ? {
                          composerExcerptReferences:
                            composerOptions.excerptReferences,
                        }
                      : {}),
                  }
                : undefined;
            const result = await chatApi.sendAgentMessage(
              contextType,
              agentContextId,
              content,
              attachmentIds,
              directSendOptions,
            );
            sentResult = result;

            queryClient.invalidateQueries({
              queryKey: chatKeys.conversationList(contextType, agentContextId),
            });

            if (result.wasQueued && result.queuedMessageId != null) {
              queueAcceptedMessage(
                content,
                result.queuedMessageId,
                attachmentIds,
                composerOptions?.selectionSnapshot,
              );
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
            composerFolderReferences?: MessageFolderReference[];
            attachmentIds?: string[];
            composerArtifactReferences?: ComposerArtifactReference[];
            composerProjectReferences?: ComposerProjectReference[];
            composerIntegrationReferences?: ComposerIntegrationReference[];
            composerExcerptReferences?: ComposerExcerptReference[];
            capabilityIntent?: CapabilityIntent | null;
            composerSelectionSnapshot?: ComposerSelectionSnapshot;
            teamIntent?: TeamIntent | null;
            teamMessageTarget?: TeamMessageTarget | null;
          } = { content };
          if (composerOptions?.folderReferences?.length) {
            params.composerFolderReferences = composerOptions.folderReferences;
          }
          if (attachmentIds !== undefined) {
            params.attachmentIds = attachmentIds;
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
          if (composerOptions?.excerptReferences?.length) {
            params.composerExcerptReferences = composerOptions.excerptReferences;
          }
          if (composerOptions?.capabilityIntent) {
            params.capabilityIntent = composerOptions.capabilityIntent;
          }
          if (composerOptions?.selectionSnapshot) {
            params.composerSelectionSnapshot = composerOptions.selectionSnapshot;
          }
          if (composerOptions?.teamIntent) {
            params.teamIntent = composerOptions.teamIntent;
          }
          if (composerOptions?.teamMessageTarget) {
            params.teamMessageTarget = composerOptions.teamMessageTarget;
          }
          const result = await sendMessage.mutateAsync(params);
          sentResult = result;
          if (result.wasQueued && result.queuedMessageId != null) {
            queueAcceptedMessage(
              content,
              result.queuedMessageId,
              attachmentIds,
              composerOptions?.selectionSnapshot,
            );
          }
          if (
            contextType === "ideation" &&
            ideationSessionId &&
            result.conversationId &&
            (result.isNewConversation || result.queuedAsPending)
          ) {
            setActiveConversation(storeContextKey, result.conversationId);
          }
          if (
            contextType === "ideation" &&
            ideationSessionId &&
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
          void onUserMessageSent?.({
            content,
            result: sentResult,
            ...(composerOptions?.integrationReferences?.length
              ? { composerIntegrationReferences: composerOptions.integrationReferences }
              : {}),
          });
        }
      } catch (err) {
        // A delivered-but-unsaved turn is live: the process is answering it. Keep the
        // bubble and the spinner instead of pretending the send never happened.
        const deliveredWithoutPersistence = isMessageDeliveredNotPersistedError(
          extractErrorMessage(err, ""),
        );
        if (optimisticMessage && !deliveredWithoutPersistence) {
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
        if (!deliveredWithoutPersistence) {
          setAgentRunning(storeContextKey, false);
        }
        if (composerOptions?.selectionSnapshot) {
          throw err;
        }
      }
    },
    [sendMessage, contextType, contextId, selectedTaskId, storeContextKey, setAgentRunning, setSending, setActiveConversation, queryClient, ideationSessionId, messageCount, queueAcceptedMessage, onUserMessageSent, reportSendFailure, activeConversationId]
  );

  // ── Stop Agent ───────────────────────────────────────────────────
  const handleStopAgent = useCallback(async () => {
    // Always attempt immediate run cancellation.
    //
    // A failed BRAKE is the one failure a user must never miss: they asked for the agent to
    // stop, the host refused or could not, and the run is still burning tokens. This used to
    // `logger.warn` and return, which made a remote `REMOTE_COMMAND_UNAVAILABLE` — and every
    // other stop failure — completely invisible. It surfaces through the same toast channel
    // the rest of this hook's chat failures use.
    try {
      await stopAgent(contextType, backendQueueContextId);
    } catch (err) {
      logger.warn("[chat] Failed to stop agent", {
        contextType,
        contextId,
        queueContextId: backendQueueContextId,
        error: err,
      });
      toast.error("Couldn't stop the agent", {
        description: extractErrorMessage(
          err,
          "The agent is still running. Try again, or stop it on the host.",
        ),
        duration: 10000,
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

  /**
   * Surfaces a failed queue mutation. Typed transport codes reuse the gate's own copy;
   * everything else carries the host's message. Never invents a new "not available" phrasing.
   */
  const reportQueueMutationFailure = useCallback(
    (title: string, err: unknown) => {
      const banner = remoteErrorBannerProps(err);
      toast.error(banner?.title ?? title, {
        description:
          banner?.body ??
          extractErrorMessage(err, "The queued message is unchanged. Try again."),
        duration: 10000,
      });
    },
    []
  );

  // ── Delete Queued Message ────────────────────────────────────────
  const handleDeleteQueuedMessage = useCallback(
    async (messageId: string) => {
      // FAIL CLOSED under a remote environment: prove the host dropped the turn BEFORE the
      // chip disappears. The optimistic order below deletes locally and swallows the host
      // failure, which on a paired device meant a turn the user watched vanish was still
      // queued on the host and still delivered to the agent. Mirrors
      // `handleSendQueuedMessageNow`, which already keeps local state truthful on failure.
      if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
        try {
          const deleted = await chatApi.cancelRemoteQueuedAgentMessage(
            backendQueueContextId,
            messageId,
          );
          deleteQueuedMessage(storeContextKey, messageId);
          if (!deleted) {
            toast.warning("Message already sent");
          }
        } catch (err) {
          // The chip stays: it is describing a turn that really is still queued.
          reportQueueMutationFailure("Couldn't delete the queued message", err);
          return;
        }
        return;
      }

      // Local: unchanged. Delete from local store immediately (optimistic)
      deleteQueuedMessage(storeContextKey, messageId);

      // Delete from backend using the same ID
      try {
        await chatApi.deleteQueuedAgentMessage(contextType, backendQueueContextId, messageId);
      } catch {
        // Silently ignore — local state already updated
      }
    },
    [
      deleteQueuedMessage,
      storeContextKey,
      contextType,
      backendQueueContextId,
      reportQueueMutationFailure,
    ]
  );

  // ── Send Queued Message Now ─────────────────────────────────────
  const handleSendQueuedMessageNow = useCallback(
    async (
      messageId: string,
      content?: string,
      attachmentIds?: string[],
      selectionSnapshot?: ComposerSelectionSnapshot,
    ) => {
      if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
        deleteQueuedMessage(storeContextKey, messageId);
        setSending(storeContextKey, true);
        try {
          const outcome = await chatApi.sendRemoteQueuedAgentMessageNow(
            backendQueueContextId,
            messageId,
            activeAgentRunId,
          );
          if (outcome.status === "alreadySent") {
            toast.warning("Message already sent");
          } else {
            setAgentRunning(storeContextKey, true);
          }
          if (outcome.rehydrateQueue) {
            await hydrateRemoteQueue();
          }
        } catch (err) {
          const hostWillRehydrate =
            err instanceof RemoteQueuedMessageSendError &&
            err.restoredToFront &&
            err.rehydrateQueue;
          if (
            err instanceof RemoteQueuedMessageSendError &&
            err.errorCode === "REMOTE_QUEUE_SEND_RUN_CHANGED"
          ) {
            toast.error("The agent moved on — refresh");
          } else {
            reportQueueMutationFailure("Couldn't send the queued message", err);
          }
          if (hostWillRehydrate) {
            await hydrateRemoteQueue();
          } else if (content) {
            queueAcceptedMessage(
              content,
              messageId,
              attachmentIds,
              selectionSnapshot,
            );
          }
        } finally {
          setSending(storeContextKey, false);
        }
        return;
      }

      deleteQueuedMessage(storeContextKey, messageId);
      setSending(storeContextKey, true);

      try {
        const result = await chatApi.sendQueuedAgentMessageNow(
          contextType,
          backendQueueContextId,
          messageId
        );

        if (result.wasQueued && result.queuedMessageId != null && content) {
          queueAcceptedMessage(
            content,
            result.queuedMessageId,
            attachmentIds,
            selectionSnapshot,
          );
        } else if (!result.wasQueued) {
          setAgentRunning(storeContextKey, true);
        }
      } catch (err) {
        // Re-queueing a turn the agent already received would send it twice.
        const deliveredWithoutPersistence = isMessageDeliveredNotPersistedError(
          extractErrorMessage(err, ""),
        );
        if (content && !deliveredWithoutPersistence) {
          queueAcceptedMessage(content, messageId, attachmentIds, selectionSnapshot);
        }
        if (deliveredWithoutPersistence) {
          setAgentRunning(storeContextKey, true);
        }
        reportSendFailure(err);
      } finally {
        setSending(storeContextKey, false);
      }
    },
    [
      backendQueueContextId,
      activeAgentRunId,
      hydrateRemoteQueue,
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
    async (
      messageId: string,
      newContent: string,
      attachmentIds?: string[],
      selectionSnapshot?: ComposerSelectionSnapshot,
    ) => {
      // The edit is delete-then-send, and the delete is the step that decides it. Under a
      // remote environment a swallowed delete failure was a DOUBLE TURN: the original stayed
      // queued on the host, the local chip was removed anyway, and the rewritten content was
      // sent unconditionally — so the agent received both. Abort the whole edit instead, and
      // leave the queue exactly as the host still has it.
      if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
        try {
          const deleted = await chatApi.cancelRemoteQueuedAgentMessage(
            backendQueueContextId,
            messageId,
          );
          if (!deleted) {
            deleteQueuedMessage(storeContextKey, messageId);
            toast.warning("Message already sent");
            return;
          }
        } catch (err) {
          reportQueueMutationFailure("Couldn't edit the queued message", err);
          return;
        }
        deleteQueuedMessage(storeContextKey, messageId);
      } else {
        // Local: unchanged. Delete old message from backend
        try {
          await chatApi.deleteQueuedAgentMessage(contextType, backendQueueContextId, messageId);
        } catch {
          // Silently ignore
        }

        // Delete from local store
        deleteQueuedMessage(storeContextKey, messageId);
      }

      // Send the edited content via sendAgentMessage (delete-before-send pattern)
      setSending(storeContextKey, true);
      try {
        const result = await chatApi.sendAgentMessage(
          contextType,
          contextId,
          newContent,
          attachmentIds !== undefined && attachmentIds.length > 0 ? attachmentIds : undefined,
          selectionSnapshot
            ? { ...sendOptions, composerSelectionSnapshot: selectionSnapshot }
            : sendOptions,
        );
        if (result.wasQueued && result.queuedMessageId != null) {
          queueAcceptedMessage(
            newContent,
            result.queuedMessageId,
            attachmentIds,
            selectionSnapshot,
          );
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
      reportQueueMutationFailure,
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
