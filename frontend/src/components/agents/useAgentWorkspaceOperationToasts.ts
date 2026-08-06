/**
 * Global driver for durable, poll-driven agent workspace operation toasts
 * (repair/publish/base-update). Watches the registry from
 * `agentWorkspaceOperationRegistry.ts`, polls each watched conversation's
 * workspace, and renders exactly one toast per conversation via the
 * precedence rules in `agentWorkspaceOperationToastDecision.ts`. Bounded
 * request/response flows with no durable backend state to poll should use
 * `startAgentWorkspaceOperationToast` instead — see the split-of-
 * responsibilities note in `agentWorkspaceOperationToast.tsx`.
 */

import { useCallback, useEffect, useRef, useSyncExternalStore } from "react";
import { useQueries, useQueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type { AgentConversationWorkspace } from "@/api/chat";
import { useStartupStatus } from "@/hooks/useStartupStatus";
import { useAgentSessionStore } from "@/stores/agentSessionStore";

import {
  clearAgentWorkspaceOperationDismissal,
  dismissAgentWorkspaceOperation,
  isAgentWorkspaceOperationDismissed,
} from "./agentWorkspaceOperationDismissals";
import {
  dismissAgentWorkspaceOperationToast,
  renderAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";
import {
  getWatchedAgentWorkspaceOperations,
  markAgentWorkspaceOperationAnnounced,
  subscribeWatchedAgentWorkspaceOperations,
  takeAgentWorkspaceOperationResult,
  unwatchAgentWorkspaceOperation,
  type WatchedAgentWorkspaceOperation,
} from "./agentWorkspaceOperationRegistry";
import { agentWorkspaceKeys, invalidateWorkspaceQueries } from "./agentWorkspaceQueries";
import { isAgentWorkspacePublishActive } from "./agentWorkspacePublishState";
import {
  deriveAgentWorkspaceOperationToastDecision,
  type AgentWorkspaceOperationToastDecision,
} from "./agentWorkspaceOperationToastDecision";

export {
  deriveAgentWorkspaceOperationToastDecision,
  type AgentWorkspaceOperationToastDecision,
  type DeriveAgentWorkspaceOperationToastDecisionInput,
} from "./agentWorkspaceOperationToastDecision";

const WORKSPACE_OPERATION_ACTIVE_POLL_MS = 1_500;
const WORKSPACE_OPERATION_IDLE_POLL_MS = 5_000;

export function useAgentWorkspaceOperationToasts(): void {
  const { isBackgroundSettled } = useStartupStatus();
  const queryClient = useQueryClient();
  const visibleConversationId = useAgentSessionStore(
    (state) => state.visibleAgentScope?.visibleConversationId ?? null,
  );

  const watchedEntries = useSyncExternalStore(
    subscribeWatchedAgentWorkspaceOperations,
    getWatchedAgentWorkspaceOperations,
    getWatchedAgentWorkspaceOperations,
  );

  const shownToastIdsRef = useRef(new Map<string, string>());
  const lastDismissalKeysRef = useRef(new Map<string, string>());

  const workspaceQueries = useQueries({
    queries: watchedEntries.map((entry) => ({
      queryKey: agentWorkspaceKeys.workspace(entry.conversationId),
      queryFn: () => chatApi.getAgentConversationWorkspace(entry.conversationId),
      enabled: isBackgroundSettled,
      refetchInterval: (query: {
        state: { data: AgentConversationWorkspace | null | undefined };
      }) => {
        const data = query.state.data ?? null;
        const inFlight =
          data?.maintenanceOperation?.status === "active" || isAgentWorkspacePublishActive(data);
        return inFlight ? WORKSPACE_OPERATION_ACTIVE_POLL_MS : WORKSPACE_OPERATION_IDLE_POLL_MS;
      },
    })),
    combine: (queries) => ({
      workspaces: queries.map((query) => query.data ?? null),
      hasData: queries.map((query) => query.data !== undefined),
      isError: queries.map((query) => query.status === "error"),
      errorUpdateCounts: queries.map((query) => query.errorUpdateCount),
      version: queries
        .map((query) => `${query.dataUpdatedAt}:${query.errorUpdateCount}`)
        .join("|"),
    }),
  });

  // React Query resets `failureCount` to 0 at the start of every new fetch
  // attempt (including a fresh poll tick), so it cannot express "consecutive
  // failed poll ticks" on its own; track that ourselves off the monotonic
  // `errorUpdateCount`.
  const consecutiveFailuresRef = useRef(new Map<string, { count: number; lastErrorUpdateCount: number }>());

  const applyDecision = useCallback(
    (entry: WatchedAgentWorkspaceOperation, decision: AgentWorkspaceOperationToastDecision) => {
      const { conversationId } = entry;

      if (decision.kind === "idle") {
        const shownId = shownToastIdsRef.current.get(conversationId);
        if (shownId) {
          dismissAgentWorkspaceOperationToast(shownId);
          shownToastIdsRef.current.delete(conversationId);
        }
        if (decision.unwatch) {
          const lastDismissalKey = lastDismissalKeysRef.current.get(conversationId);
          if (lastDismissalKey) {
            clearAgentWorkspaceOperationDismissal(lastDismissalKey);
          }
          lastDismissalKeysRef.current.delete(conversationId);
          consecutiveFailuresRef.current.delete(conversationId);
          unwatchAgentWorkspaceOperation(conversationId);
          void invalidateWorkspaceQueries(queryClient, conversationId);
        }
        return;
      }

      const { view } = decision;
      lastDismissalKeysRef.current.set(conversationId, view.dismissalKey);

      if (decision.kind === "announce") {
        if (entry.announcedStateKey === decision.stateKey) {
          return;
        }
        markAgentWorkspaceOperationAnnounced(conversationId, decision.stateKey);
      }

      const suppressed =
        visibleConversationId === conversationId ||
        isAgentWorkspaceOperationDismissed(view.dismissalKey);

      if (suppressed) {
        const shownId = shownToastIdsRef.current.get(conversationId);
        if (shownId) {
          dismissAgentWorkspaceOperationToast(shownId);
          shownToastIdsRef.current.delete(conversationId);
        }
        return;
      }

      renderAgentWorkspaceOperationToast(view, {
        onDismiss: () => {
          dismissAgentWorkspaceOperation(view.dismissalKey);
          shownToastIdsRef.current.delete(conversationId);
          dismissAgentWorkspaceOperationToast(view.id);
        },
      });
      shownToastIdsRef.current.set(conversationId, view.id);
    },
    [queryClient, visibleConversationId],
  );

  useEffect(() => {
    if (!isBackgroundSettled) {
      return;
    }
    watchedEntries.forEach((entry, index) => {
      const hasData = workspaceQueries.hasData[index] ?? false;
      const isError = workspaceQueries.isError[index] ?? false;
      const errorUpdateCount = workspaceQueries.errorUpdateCounts[index] ?? 0;

      if (hasData) {
        consecutiveFailuresRef.current.delete(entry.conversationId);
      } else if (isError && errorUpdateCount > 0) {
        const tracked = consecutiveFailuresRef.current.get(entry.conversationId);
        if (tracked?.lastErrorUpdateCount !== errorUpdateCount) {
          consecutiveFailuresRef.current.set(entry.conversationId, {
            count: (tracked?.count ?? 0) + 1,
            lastErrorUpdateCount: errorUpdateCount,
          });
        }
      }
      const consecutiveFetchFailures =
        consecutiveFailuresRef.current.get(entry.conversationId)?.count ?? 0;

      // A query that has never resolved (no data yet, under the failure
      // threshold) tells us nothing; deciding now would read as "nothing in
      // flight" and unwatch an operation we simply haven't observed yet.
      if (!hasData && consecutiveFetchFailures < 3) {
        return;
      }
      const workspace = workspaceQueries.workspaces[index] ?? null;
      const pendingResult = takeAgentWorkspaceOperationResult(entry.conversationId);
      const decision = deriveAgentWorkspaceOperationToastDecision({
        workspace,
        entry,
        pendingResult,
        consecutiveFetchFailures,
      });
      applyDecision(entry, decision);
    });
    // workspaceQueries.version intentionally drives re-evaluation on every poll tick,
    // even when a poll returns structurally-equal data.
  }, [applyDecision, isBackgroundSettled, watchedEntries, workspaceQueries]);
}
