/**
 * Ideation event hooks - Tauri ideation event listeners with type-safe validation
 *
 * Uses EventBus abstraction for browser/Tauri compatibility.
 */

import { useEffect } from "react";
import { z } from "zod";
import { useQueryClient } from "@tanstack/react-query";
import { useEventBus } from "@/providers/EventProvider";
import { useIdeationStore } from "@/stores/ideationStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { ideationApi } from "@/api/ideation";
import {
  latestVerificationChildSessionData,
  latestVerificationChildSessionIdQueryKey,
} from "@/components/agents/agentChatFocus";
import { ideationKeys } from "./useIdeation";
import { dependencyKeys } from "./useDependencyGraph";
import { taskKeys } from "./useTasks";
import { proposalKeys } from "./useProposals";
import type { Unsubscribe } from "@/lib/event-bus";
import { logger } from "@/lib/logger";

/**
 * Schema for session title updated event payload
 */
const SessionTitleUpdatedEventSchema = z.object({
  sessionId: z.string(),
  title: z.string().nullable(),
});

/**
 * Schema for proposal priority assessed event payload
 */
const ProposalPriorityAssessedEventSchema = z.object({
  proposalId: z.string(),
  priority: z.string(),
  score: z.number(),
  reason: z.string(),
});

/**
 * Schema for session priorities assessed event payload
 */
const SessionPrioritiesAssessedEventSchema = z.object({
  sessionId: z.string(),
  count: z.number(),
});

/**
 * Schema for dependency added/removed event payload
 */
const DependencyEventSchema = z.object({
  proposalId: z.string(),
  dependsOnId: z.string(),
});

/**
 * Schema for child session created event payload
 */
const ChildSessionCreatedEventSchema = z.object({
  sessionId: z.string(),
  parentSessionId: z.string(),
  title: z.string(),
  purpose: z.enum(['general', 'verification']).optional(),
  orchestrationTriggered: z.boolean().optional(),
  pendingInitialPrompt: z.string().nullable().optional(),
});

/**
 * Schema for session created event payload
 */
const SessionCreatedEventSchema = z.object({
  sessionId: z.string(),
  projectId: z.string(),
});

/**
 * Schema for session accepted event payload (emitted by finalize_proposals HTTP handler)
 */
const SessionAcceptedEventSchema = z.object({
  sessionId: z.string(),
  projectId: z.string(),
});

/**
 * Schema for finalize pending confirmation event
 * (emitted when require_accept_for_finalize gate is active)
 */
const FinalizePendingConfirmationEventSchema = z.object({
  sessionId: z.string(),
  sessionTitle: z.string().nullable(),
});

/**
 * Hook to listen for ideation events from the backend
 *
 * Listens to 'ideation:session_title_updated' events and updates the
 * ideation store accordingly. This enables real-time session title updates
 * when the ralphx-utility-session-namer agent generates a title.
 *
 * @example
 * ```tsx
 * function App() {
 *   useIdeationEvents(); // Sets up listener automatically
 *   return <IdeationView />;
 * }
 * ```
 */
export function useIdeationEvents() {
  const bus = useEventBus();
  const updateSession = useIdeationStore((s) => s.updateSession);
  const setVerificationNotification = useIdeationStore((s) => s.setVerificationNotification);
  const clearVerificationNotification = useIdeationStore((s) => s.clearVerificationNotification);
  const setActiveVerificationChildId = useIdeationStore((s) => s.setActiveVerificationChildId);
  const setLastVerificationChildId = useIdeationStore((s) => s.setLastVerificationChildId);
  const enqueuePendingConfirmation = useUiStore((s) => s.enqueuePendingConfirmation);
  const autoAcceptPlans = useUiStore((s) => s.autoAcceptPlans);
  const autoAcceptSessions = useUiStore((s) => s.autoAcceptSessions);
  const queryClient = useQueryClient();

  useEffect(() => {
    logger.debug("[IdeationEvents] Setting up event listeners");
    const unsubscribes: Unsubscribe[] = [];

    // Listen for session title updates (from ralphx-utility-session-namer agent)
    unsubscribes.push(
      bus.subscribe<unknown>("ideation:session_title_updated", (payload) => {
        logger.debug("[IdeationEvents] Received session_title_updated:", payload);
        const parsed = SessionTitleUpdatedEventSchema.safeParse(payload);

        if (!parsed.success) {
          console.error(
            "Invalid ideation:session_title_updated event:",
            parsed.error.message
          );
          return;
        }

        logger.debug("[IdeationEvents] Updating session title:", parsed.data.sessionId, "->", parsed.data.title);
        updateSession(parsed.data.sessionId, { title: parsed.data.title });
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
      })
    );

    // Listen for single proposal priority assessment
    unsubscribes.push(
      bus.subscribe<unknown>("proposal:priority_assessed", (payload) => {
        logger.debug("[IdeationEvents] Received proposal:priority_assessed:", payload);
        const parsed = ProposalPriorityAssessedEventSchema.safeParse(payload);

        if (!parsed.success) {
          console.error(
            "Invalid proposal:priority_assessed event:",
            parsed.error.message
          );
          return;
        }

        // Invalidate proposals query to refetch with updated priority
        queryClient.invalidateQueries({ queryKey: ideationKeys.proposals() });
      })
    );

    // Listen for batch session priorities assessment
    unsubscribes.push(
      bus.subscribe<unknown>("session:priorities_assessed", (payload) => {
        logger.debug("[IdeationEvents] Received session:priorities_assessed:", payload);
        const parsed = SessionPrioritiesAssessedEventSchema.safeParse(payload);

        if (!parsed.success) {
          console.error(
            "Invalid session:priorities_assessed event:",
            parsed.error.message
          );
          return;
        }

        // Invalidate proposals query to refetch with updated priorities
        queryClient.invalidateQueries({ queryKey: ideationKeys.proposals() });
      })
    );

    // Listen for dependency added
    unsubscribes.push(
      bus.subscribe<unknown>("dependency:added", (payload) => {
        logger.debug("[IdeationEvents] Received dependency:added:", payload);
        const parsed = DependencyEventSchema.safeParse(payload);

        if (!parsed.success) {
          console.error("Invalid dependency:added event:", parsed.error.message);
          return;
        }

        // Invalidate dependency graph query
        queryClient.invalidateQueries({ queryKey: dependencyKeys.graphs() });
      })
    );

    // Listen for dependency removed
    unsubscribes.push(
      bus.subscribe<unknown>("dependency:removed", (payload) => {
        logger.debug("[IdeationEvents] Received dependency:removed:", payload);
        const parsed = DependencyEventSchema.safeParse(payload);

        if (!parsed.success) {
          console.error("Invalid dependency:removed event:", parsed.error.message);
          return;
        }

        // Invalidate dependency graph query
        queryClient.invalidateQueries({ queryKey: dependencyKeys.graphs() });
      })
    );

    // Listen for new session created (external MCP or internal)
    unsubscribes.push(
      bus.subscribe<unknown>("ideation:session_created", (payload) => {
        logger.debug("[IdeationEvents] Received ideation:session_created:", payload);
        const parsed = SessionCreatedEventSchema.safeParse(payload);

        if (!parsed.success) {
          console.error("Invalid ideation:session_created event:", parsed.error.message);
          return;
        }

        // Refresh session list so newly created session appears in sidebar
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
      })
    );

    // Listen for session accepted (emitted by finalize_proposals HTTP handler)
    unsubscribes.push(
      bus.subscribe<unknown>("ideation:session_accepted", (payload) => {
        logger.debug("[IdeationEvents] Received ideation:session_accepted:", payload);
        const parsed = SessionAcceptedEventSchema.safeParse(payload);

        if (!parsed.success) {
          console.error("Invalid ideation:session_accepted event:", parsed.error.message);
          return;
        }

        const { sessionId } = parsed.data;

        // Mirror useApplyProposals.onSuccess() — same queries to refresh
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessionWithData(sessionId) });
        queryClient.invalidateQueries({ queryKey: taskKeys.all });
        queryClient.invalidateQueries({ queryKey: proposalKeys.list(sessionId) });
        queryClient.invalidateQueries({ queryKey: ["plan-branch"] });
      })
    );

    // Listen for child session created (follow-up delegation)
    unsubscribes.push(
      bus.subscribe<unknown>("ideation:child_session_created", (payload) => {
        logger.debug("[IdeationEvents] Received ideation:child_session_created:", payload);
        const parsed = ChildSessionCreatedEventSchema.safeParse(payload);

        if (!parsed.success) {
          console.error("Invalid ideation:child_session_created event:", parsed.error.message);
          return;
        }

        // Track verification children in the store for notification display
        if (parsed.data.purpose === 'verification') {
          const orchestrationTriggered = parsed.data.orchestrationTriggered ?? true;
          queryClient.setQueryData(
            latestVerificationChildSessionIdQueryKey(parsed.data.parentSessionId),
            latestVerificationChildSessionData(
              parsed.data.parentSessionId,
              parsed.data.sessionId,
            ),
          );
          updateSession(parsed.data.parentSessionId, {
            verificationInProgress: orchestrationTriggered,
          });
          setLastVerificationChildId(parsed.data.parentSessionId, parsed.data.sessionId);
          if (orchestrationTriggered) {
            setVerificationNotification(parsed.data.parentSessionId, parsed.data.sessionId);
            setActiveVerificationChildId(parsed.data.parentSessionId, parsed.data.sessionId);
            // Synthetic "generating" status on parent while verification child is running
            const parentKey = buildStoreKey('ideation', parsed.data.parentSessionId);
            useChatStore.getState().setAgentStatus(parentKey, 'generating');
            useChatStore.getState().updateLastAgentEvent(parentKey);
          } else {
            clearVerificationNotification(parsed.data.parentSessionId);
            setActiveVerificationChildId(parsed.data.parentSessionId, null);
          }

          queryClient.invalidateQueries({
            queryKey: ["childSessions", parsed.data.parentSessionId, "verification"],
          });
          queryClient.invalidateQueries({
            queryKey: ["child-session-status", parsed.data.parentSessionId],
          });
          queryClient.invalidateQueries({
            queryKey: ["child-session-status", parsed.data.sessionId],
          });
          queryClient.invalidateQueries({
            queryKey: ["verification", parsed.data.parentSessionId],
          });
          queryClient.invalidateQueries({
            queryKey: ideationKeys.sessionWithData(parsed.data.parentSessionId),
          });
        }

        // Emit a local event for UI components to handle
        // This allows the PlanningView to show a "View Follow-up" link
        bus.emit("ideation:child_session_created:local", parsed.data);

        // Invalidate sessions query to refresh the session list
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
      })
    );

    // Listen for agent-initiated finalization awaiting user confirmation
    unsubscribes.push(
      bus.subscribe<unknown>("ideation:finalize_pending_confirmation", (payload) => {
        logger.debug("[IdeationEvents] Received ideation:finalize_pending_confirmation:", payload);
        const parsed = FinalizePendingConfirmationEventSchema.safeParse(payload);

        if (!parsed.success) {
          console.error(
            "Invalid ideation:finalize_pending_confirmation event:",
            parsed.error.message
          );
          return;
        }

        const { sessionId } = parsed.data;

        // Check auto-accept: global or per-session — if on, bypass dialog entirely
        if (autoAcceptPlans || autoAcceptSessions.has(sessionId)) {
          logger.debug("[IdeationEvents] Auto-accepting finalize for session:", sessionId);
          ideationApi.acceptance.accept(sessionId).then(() => {
            // Mirror useAcceptFinalize.onSuccess query invalidations
            queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
            queryClient.invalidateQueries({ queryKey: ideationKeys.sessionWithData(sessionId) });
            queryClient.invalidateQueries({ queryKey: taskKeys.all });
            queryClient.invalidateQueries({ queryKey: proposalKeys.list(sessionId) });
            queryClient.invalidateQueries({ queryKey: ["plan-branch"] });
          }).catch((err: Error) => {
            logger.warn("[IdeationEvents] Auto-accept failed, falling back to dialog:", err.message);
            // Fall back to showing the dialog on auto-accept failure
            enqueuePendingConfirmation(sessionId);
            queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
            queryClient.invalidateQueries({ queryKey: ideationKeys.sessionWithData(sessionId) });
          });
          return;
        }

        // Enqueue session for confirmation dialog; dialog shows first item in queue
        enqueuePendingConfirmation(sessionId);

        // Refresh session so acceptance_status reflects pending
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(sessionId),
        });
      })
    );

    return () => {
      unsubscribes.forEach((unsub) => unsub());
    };
  }, [bus, updateSession, setVerificationNotification, clearVerificationNotification, setActiveVerificationChildId, setLastVerificationChildId, enqueuePendingConfirmation, autoAcceptPlans, autoAcceptSessions, queryClient]);
}
