/**
 * useVerificationEvents — typed listener for plan_verification:status_changed events.
 *
 * Listens to the Tauri event emitted by the backend whenever verification
 * state changes (POST /verification, revert-and-skip, reconciliation reset, etc.)
 * and invalidates TanStack Query caches so UI reflects the latest state.
 *
 * Extended payload schema (B1):
 * {
 *   session_id: string,
 *   status: VerificationStatus,
 *   in_progress: boolean,
 *   generation?: number | null,
 *   round?: number,
 *   max_rounds?: number,
 *   gap_score?: number,
 *   convergence_reason?: string,
 *   current_gaps?: EventVerificationGap[],
 *   rounds?: EventRoundSummary[]
 * }
 */

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useEventBus } from "@/providers/EventProvider";
import { useIdeationStore } from "@/stores/ideationStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { navigateToIdeationSession } from "@/lib/navigation";
import { latestVerificationChildSessionIdQueryKey } from "@/components/agents/agentChatFocus";
import { ideationKeys } from "./useIdeation";
import type { Unsubscribe } from "@/lib/event-bus";
import { logger } from "@/lib/logger";
import type { VerificationStatus, VerificationGap } from "@/types/ideation";
import type { VerificationStatusResponse } from "@/api/ideation.types";
import { PlanVerificationStatusChangedSchema } from "@/types/events";
import type { EventVerificationGap } from "@/types/events";
import { verificationApi } from "@/api/verification";
import { PendingVerificationEventSchema } from "@/types/verification-config";
import { verificationStatusKey } from "./useVerificationStatus";
export type { PlanVerificationStatusChangedEvent, PlanVerificationStatusChangedPayload } from "@/types/events";

// ============================================================================
// Hook
// ============================================================================

/**
 * Subscribes to `plan_verification:status_changed` events from the Tauri backend.
 *
 * On each event:
 * 1. Validates the D20 payload with Zod
 * 2. Updates the ideation store session inline (optimistic partial update)
 * 3. Cancels in-flight verification fetches to prevent stale overwrites (race fix)
 * 4. Fast path: populates verification cache directly with round guard + planVersion stamp
 * 5. Conditionally invalidates (only verification refetch when no fast-path data)
 *
 * Mount this once near the root of the ideation feature tree (e.g. alongside
 * `useIdeationEvents`).
 */
export function useVerificationEvents() {
  const bus = useEventBus();
  const updateSession = useIdeationStore((s) => s.updateSession);
  const clearVerificationNotification = useIdeationStore((s) => s.clearVerificationNotification);
  const queryClient = useQueryClient();
  const enqueuePendingVerification = useUiStore((s) => s.enqueuePendingVerification);
  const autoAcceptVerificationSessions = useUiStore((s) => s.autoAcceptVerificationSessions);

  useEffect(() => {
    logger.debug("[VerificationEvents] Setting up plan_verification:status_changed listener");
    const unsubscribes: Unsubscribe[] = [];

    unsubscribes.push(
      bus.subscribe<unknown>("plan_verification:status_changed", (payload) => {
        logger.debug("[VerificationEvents] Received plan_verification:status_changed:", payload);

        const parsed = PlanVerificationStatusChangedSchema.safeParse(payload);
        if (!parsed.success) {
          logger.debug(
            "[VerificationEvents] Invalid plan_verification:status_changed event:",
            parsed.error.message
          );
          return;
        }

        const {
          session_id: sessionId,
          status,
          in_progress: inProgress,
          generation: rawGeneration,
          gap_score: rawGapScore,
          round: rawRound,
          max_rounds: rawMaxRounds,
          convergence_reason: rawConvergenceReason,
          current_gaps: currentGaps,
          rounds,
        } = parsed.data;
        const generation = rawGeneration ?? undefined;
        const gapScore = rawGapScore ?? undefined;
        const round = rawRound ?? undefined;
        const maxRounds = rawMaxRounds ?? undefined;
        const convergenceReason = rawConvergenceReason ?? undefined;

        // Partial store update so components re-render immediately (sync)
        // Increment verificationUpdateSeq so resolvedSession merge prefers store over stale React Query data
        const currentSeq = useIdeationStore.getState().sessions[sessionId]?.verificationUpdateSeq ?? 0;
        updateSession(sessionId, {
          verificationStatus: status as VerificationStatus,
          verificationInProgress: inProgress,
          ...(gapScore !== undefined && { gapScore }),
          verificationUpdateSeq: currentSeq + 1,
        });

        // Toast notifications for terminal transitions (in_progress=false with terminal status)
        if (!inProgress) {
          const sessionTitle =
            useIdeationStore.getState().sessions[sessionId]?.title ?? "Ideation Session";
          const toastOptions = {
            id: `verification:${sessionId}`,
            duration: 30000,
            closeButton: true,
            description: `Session: ${sessionTitle}`,
            action: {
              label: "View Session",
              onClick: () => {
                navigateToIdeationSession(sessionId);
                toast.dismiss(`verification:${sessionId}`);
              },
            },
          };

          if (status === "verified") {
            toast.success("Plan verified — no critical gaps remain.", toastOptions);
          } else if (status === "needs_revision") {
            toast.warning("Verification complete — gaps found that need attention.", toastOptions);
          } else if (status === "skipped") {
            toast.info("Verification skipped.", toastOptions);
          }
          // Clear verification notification banner on any terminal state
          clearVerificationNotification(sessionId);
          // Clear active verification child ref, then set parent to idle.
          // Unconditional: parent agent already exited; next natural event self-corrects if still running.
          useIdeationStore.getState().setActiveVerificationChildId(sessionId, null);
          useChatStore.getState().setAgentStatus(buildStoreKey('ideation', sessionId), 'idle');
        }

        // Async cache update — bus.subscribe callbacks must be synchronous
        void updateVerificationQueryCache({
          queryClient,
          sessionId,
          status,
          inProgress,
          generation,
          gapScore,
          round,
          maxRounds,
          convergenceReason,
          currentGaps,
          rounds,
        });
      })
    );

    unsubscribes.push(
      bus.subscribe<unknown>("verification:pending_confirmation", (payload) => {
        logger.debug("[VerificationEvents] Received verification:pending_confirmation:", payload);

        const parsed = PendingVerificationEventSchema.safeParse(payload);
        if (!parsed.success) {
          logger.debug(
            "[VerificationEvents] Invalid verification:pending_confirmation event:",
            parsed.error.message
          );
          return;
        }

        const { session_id: sessionId } = parsed.data;

        // Auto-accept: if session is in auto-accept set, confirm without showing dialog
        if (autoAcceptVerificationSessions.has(sessionId)) {
          logger.debug("[VerificationEvents] Auto-accepting verification for session:", sessionId);
          verificationApi.confirm(sessionId, []).catch((err: Error) => {
            logger.warn("[VerificationEvents] Auto-accept verification failed, showing dialog:", err.message);
            // Fall back to dialog on auto-accept failure
            enqueuePendingVerification(sessionId);
          });
          return;
        }

        // Enqueue session for confirmation dialog
        enqueuePendingVerification(sessionId);

        // Refresh session data
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessionWithData(sessionId) });
      })
    );

    return () => {
      unsubscribes.forEach((unsub) => unsub());
    };
  }, [bus, updateSession, clearVerificationNotification, queryClient, enqueuePendingVerification, autoAcceptVerificationSessions]);
}

// ============================================================================
// Helpers
// ============================================================================

interface UpdateVerificationQueryCacheParams {
  queryClient: ReturnType<typeof useQueryClient>;
  sessionId: string;
  status: string;
  inProgress: boolean;
  generation: number | undefined;
  gapScore: number | undefined;
  round: number | undefined;
  maxRounds: number | undefined;
  convergenceReason: string | undefined;
  currentGaps: EventVerificationGap[] | undefined;
  rounds: unknown[] | undefined;
}

async function updateVerificationQueryCache({
  queryClient,
  sessionId,
  status,
  inProgress,
  generation,
  gapScore,
  round,
  maxRounds,
  convergenceReason,
  currentGaps,
  rounds,
}: UpdateVerificationQueryCacheParams): Promise<void> {
  const currentVerificationKey = verificationStatusKey(sessionId);

  // 1. Cancel in-flight verification fetches BEFORE setQueryData.
  //    This closes the race window where a background refetch started by a
  //    previous event could complete after setQueryData and overwrite fresh data.
  await queryClient.cancelQueries({ queryKey: currentVerificationKey });

  // 2. Fast path: if event carries full gap/round data, populate cache directly
  //    so UI updates instantly without waiting for a refetch round-trip.
  if (currentGaps !== undefined && rounds !== undefined) {
    const cached = queryClient.getQueryData<VerificationStatusResponse>(currentVerificationKey);
    const generationChanged =
      generation !== undefined && generation !== cached?.generation;
    const generationAdvanced =
      generation !== undefined &&
      (cached?.generation === undefined || generation > cached.generation);

    // Round guard: reject out-of-order events within the same generation only.
    // A newer generation must always win even if its round number is lower.
    const isStaleRound =
      !generationAdvanced &&
      cached?.currentRound !== undefined &&
      round !== undefined &&
      round < cached.currentRound &&
      status !== "unverified";

    if (isStaleRound) {
      logger.debug(
        "[VerificationEvents] Skipping stale event: round",
        round,
        "< cached",
        cached?.currentRound,
        "for generation",
        generation ?? cached?.generation
      );
    } else {
      const transformedGaps: VerificationGap[] = transformGaps(currentGaps);

      // Stamp current plan version so staleness can be derived later by comparing
      // planArtifact.version (store) vs planVersion (verification cache).
      const planVersion = useIdeationStore.getState().planArtifact?.metadata.version;
      if (planVersion === undefined) {
        logger.debug(
          "[VerificationEvents] planVersion undefined at stamp time — " +
          "staleness comparison will fall through to 'not stale'"
        );
      }

      const preservedRounds = generationChanged ? [] : (cached?.rounds ?? []);
      const preservedRoundDetails =
        generationChanged ? [] : (cached?.roundDetails ?? []);

      const cacheData: VerificationStatusResponse = {
        sessionId,
        status: status as VerificationStatusResponse["status"],
        inProgress,
        ...(generation !== undefined && { generation }),
        ...(round !== undefined && { currentRound: round }),
        ...(maxRounds !== undefined && { maxRounds }),
        ...(gapScore !== undefined && { gapScore }),
        ...(convergenceReason != null && { convergenceReason }),
        gaps: transformedGaps,
        // Event rounds have a slimmer shape than the HTTP query; preserve same-generation
        // lineage until a refetch replaces it with authoritative round summaries/details.
        rounds: preservedRounds,
        roundDetails: preservedRoundDetails,
        runHistory: cached?.runHistory ?? [],
        ...(planVersion !== undefined && { planVersion }),
      };
      queryClient.setQueryData(currentVerificationKey, cacheData);
      logger.debug(
        "[VerificationEvents] setQueryData fast path for",
        sessionId,
        "generation",
        generation,
        "round",
        round
      );
    }

    // Fast path carries authoritative headline state. For terminal same-generation updates,
    // mark the query stale without an immediate refetch so an active Verification tab does
    // not get clobbered by a behind-persistence response from the server.
    if (generationChanged || !inProgress) {
      queryClient.invalidateQueries({
        queryKey: currentVerificationKey,
        refetchType: generationChanged ? "active" : "none",
      });
    }
  } else {
    // No fast path data — must invalidate to trigger refetch.
    // This branch currently never fires (all emission points include full data)
    // but serves as safety net for future emission points.
    queryClient.invalidateQueries({ queryKey: currentVerificationKey });
  }

  // Invalidate child sessions so history picker and VerificationPanel stay fresh.
  queryClient.invalidateQueries({ queryKey: ["childSessions", sessionId, "verification"] });
  queryClient.invalidateQueries({ queryKey: ["child-session-status", sessionId] });
  queryClient.invalidateQueries({
    queryKey: latestVerificationChildSessionIdQueryKey(sessionId),
  });

  // Always invalidate session queries (different data source, no race risk)
  queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
  queryClient.invalidateQueries({
    queryKey: ideationKeys.sessionWithData(sessionId),
  });
}

function transformGaps(gaps: EventVerificationGap[]): VerificationGap[] {
  return gaps.map((g) => ({
    severity: g.severity,
    category: g.category,
    description: g.description,
    ...(g.why_it_matters != null && { whyItMatters: g.why_it_matters }),
  }));
}
