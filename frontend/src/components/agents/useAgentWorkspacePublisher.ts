import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useQueries } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspacePublicationEvent,
} from "@/api/chat";
import { invalidateConversationDataQueries } from "@/hooks/useChat";

import type { AgentConversation } from "./agentConversations";
import {
  type ActivePublishAttempt,
  type AgentWorkspacePublishAttempt,
  type PublishAttemptState,
  type PublishFinalResult,
  createAgentWorkspacePublishAttempt,
  readAgentWorkspaceDurablePublishResult,
  readAgentWorkspacePublishBaseline,
  renderAgentWorkspacePublishResult,
} from "./agentWorkspacePublishAttempt";
import {
  agentWorkspaceOperationErrorDetail,
} from "./agentWorkspaceOperationToast";
import { isAgentWorkspacePublishActive } from "./agentWorkspacePublishState";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";

const PUBLISH_EVENT_POLL_MS = 1_500;

export type { AgentWorkspacePublishAttempt } from "./agentWorkspacePublishAttempt";

interface UseAgentWorkspacePublisherArgs {
  activeWorkspace: AgentConversationWorkspace | null;
  findConversationById: (conversationId: string) => AgentConversation | null;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
  optimisticWorkspacesByConversationId: Record<string, AgentConversationWorkspace>;
  queryClient: QueryClient;
  selectedConversationId: string | null;
}

function renderableAttemptState(
  attempt: ActivePublishAttempt,
): PublishAttemptState {
  return {
    baseline: attempt.baseline,
    conversationId: attempt.conversationId,
    startedAtMs: attempt.startedAtMs,
    token: attempt.token,
  };
}

export function useAgentWorkspacePublisher({
  activeWorkspace,
  findConversationById,
  invalidateProjectConversations,
  optimisticWorkspacesByConversationId,
  queryClient,
  selectedConversationId,
}: UseAgentWorkspacePublisherArgs) {
  const [attemptStates, setAttemptStates] = useState<
    Record<string, PublishAttemptState>
  >({});
  const activeAttemptsRef = useRef(new Map<string, ActivePublishAttempt>());
  const nextTokenRef = useRef(0);
  const attemptStateEntries = useMemo(
    () => Object.values(attemptStates),
    [attemptStates],
  );
  const publicationEventSnapshot = useQueries({
    queries: attemptStateEntries.map((attempt) => ({
      queryKey: agentWorkspaceKeys.publicationEvents(attempt.conversationId),
      queryFn: () =>
        chatApi.listAgentConversationWorkspacePublicationEvents(
          attempt.conversationId,
        ),
      enabled: attempt.baseline.state === "available",
      refetchInterval: PUBLISH_EVENT_POLL_MS,
      staleTime: 0,
    })),
    combine: (queries) => ({
      data: queries.map((query) => query.data),
      version: queries.map((query) => query.dataUpdatedAt).join(":"),
    }),
  });

  const clearAttemptState = useCallback((attempt: ActivePublishAttempt) => {
    setAttemptStates((current) => {
      if (current[attempt.conversationId]?.token !== attempt.token) {
        return current;
      }
      const next = { ...current };
      delete next[attempt.conversationId];
      return next;
    });
  }, []);

  const finalizeAttempt = useCallback(
    (attempt: ActivePublishAttempt, result: PublishFinalResult): boolean => {
      const current = activeAttemptsRef.current.get(attempt.conversationId);
      if (current?.token !== attempt.token || attempt.settled) {
        return false;
      }
      attempt.settled = true;
      if (result.kind === "success") {
        queryClient.setQueryData(
          agentWorkspaceKeys.workspace(attempt.conversationId),
          result.workspace,
        );
      }
      renderAgentWorkspacePublishResult(attempt, result);
      if (result.kind === "needs_agent") {
        invalidateConversationDataQueries(queryClient, attempt.conversationId);
      }
      activeAttemptsRef.current.delete(attempt.conversationId);
      clearAttemptState(attempt);
      attempt.resolve();
      void Promise.all([
        invalidateWorkspaceQueries(queryClient, attempt.conversationId),
        attempt.projectId
          ? invalidateProjectConversations(attempt.projectId)
          : Promise.resolve(),
      ]).catch(() => undefined);
      return true;
    },
    [clearAttemptState, invalidateProjectConversations, queryClient],
  );

  const reconcileDurableAttempt = useCallback(
    async (
      attemptState: PublishAttemptState,
      events: AgentConversationWorkspacePublicationEvent[] | undefined,
    ) => {
      const attempt = activeAttemptsRef.current.get(attemptState.conversationId);
      if (
        !attempt ||
        attempt.token !== attemptState.token ||
        !events
      ) {
        return;
      }
      if (attempt.reconciling) {
        return;
      }
      attempt.reconciling = true;
      try {
        const result = await readAgentWorkspaceDurablePublishResult(
          queryClient,
          attempt,
          events,
        );
        if (result) {
          finalizeAttempt(attempt, result);
        }
      } catch {
        // Durable reads fail closed; the direct RPC remains authoritative.
      } finally {
        const current = activeAttemptsRef.current.get(attempt.conversationId);
        if (current?.token === attempt.token) {
          current.reconciling = false;
        }
      }
    },
    [finalizeAttempt, queryClient],
  );

  useEffect(() => {
    attemptStateEntries.forEach((attempt, index) => {
      void reconcileDurableAttempt(attempt, publicationEventSnapshot.data[index]);
    });
    // dataUpdatedAt advances for structurally equal polling responses.
  }, [attemptStateEntries, publicationEventSnapshot, reconcileDurableAttempt]);

  useEffect(
    () => () => {
      activeAttemptsRef.current.forEach((attempt) => {
        attempt.controller.dismiss();
        attempt.resolve();
      });
      activeAttemptsRef.current.clear();
    },
    [],
  );

  const handlePublishWorkspace = useCallback(
    (conversationId: string): Promise<void> => {
      const existing = activeAttemptsRef.current.get(conversationId);
      if (existing) {
        return existing.completion;
      }
      const conversation = findConversationById(conversationId);
      const workspace =
        selectedConversationId === conversationId
          ? activeWorkspace
          : optimisticWorkspacesByConversationId[conversationId] ?? null;
      const startedAtMs = Date.now();
      const token = ++nextTokenRef.current;
      const attempt = createAgentWorkspacePublishAttempt({
        conversationId,
        conversationTitle: conversation?.title?.trim() || null,
        projectId: conversation?.projectId ?? workspace?.projectId ?? null,
        startedAtMs,
        token,
      });
      activeAttemptsRef.current.set(conversationId, attempt);
      setAttemptStates((current) => ({
        ...current,
        [conversationId]: renderableAttemptState(attempt),
      }));

      void (async () => {
        attempt.baseline = await readAgentWorkspacePublishBaseline(
          queryClient,
          conversationId,
        );
        setAttemptStates((current) =>
          current[conversationId]?.token === token
            ? {
                ...current,
                [conversationId]: renderableAttemptState(attempt),
              }
            : current,
        );
        if (activeAttemptsRef.current.get(conversationId)?.token !== token) {
          return;
        }
        try {
          const result = await chatApi.publishAgentConversationWorkspace(conversationId);
          if (activeAttemptsRef.current.get(conversationId)?.token !== token) {
            void invalidateWorkspaceQueries(queryClient, conversationId);
            return;
          }
          if (isAgentWorkspacePublishActive(result.workspace)) {
            void invalidateWorkspaceQueries(queryClient, conversationId);
            return;
          }
          finalizeAttempt(attempt, { kind: "success", workspace: result.workspace });
        } catch (error) {
          if (activeAttemptsRef.current.get(conversationId)?.token !== token) {
            void invalidateWorkspaceQueries(queryClient, conversationId);
            return;
          }
          const detail = agentWorkspaceOperationErrorDetail(
            error,
            "Failed to publish branch",
          );
          let refreshedWorkspace: AgentConversationWorkspace | null = null;
          try {
            refreshedWorkspace =
              await chatApi.getAgentConversationWorkspace(conversationId);
          } catch {
            // The direct error remains authoritative without a workspace refresh.
          }
          if (activeAttemptsRef.current.get(conversationId)?.token !== token) {
            void invalidateWorkspaceQueries(queryClient, conversationId);
            return;
          }
          const receiptNeedsDurableResolution =
            refreshedWorkspace?.publicationMetadataAttemptId !== null &&
            refreshedWorkspace?.publicationMetadataAttemptId !== undefined &&
            (isAgentWorkspacePublishActive(refreshedWorkspace) ||
              (refreshedWorkspace.publicationMetadataPhase === "settled" &&
                (refreshedWorkspace.publicationMetadataState === "applied" ||
                  refreshedWorkspace.publicationMetadataState === "reconciled")));
          if (receiptNeedsDurableResolution) {
            void invalidateWorkspaceQueries(queryClient, conversationId);
            return;
          }
          const needsAgent =
            (refreshedWorkspace ?? workspace)?.publicationPushStatus === "needs_agent";
          finalizeAttempt(attempt, {
            detail,
            kind: needsAgent ? "needs_agent" : "failure",
          });
        }
      })();
      return attempt.completion;
    },
    [
      activeWorkspace,
      finalizeAttempt,
      findConversationById,
      optimisticWorkspacesByConversationId,
      queryClient,
      selectedConversationId,
    ],
  );

  const publishAttemptsByConversationId = useMemo(
    () =>
      Object.fromEntries(
        Object.values(attemptStates).map(({ conversationId, startedAtMs }) => [
          conversationId,
          { conversationId, startedAtMs },
        ]),
      ) as Record<string, AgentWorkspacePublishAttempt>,
    [attemptStates],
  );

  return { handlePublishWorkspace, publishAttemptsByConversationId };
}
