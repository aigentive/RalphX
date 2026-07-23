/**
 * Plan artifact event hooks - Tauri plan artifact event listeners with type-safe validation
 *
 * Uses EventBus abstraction for browser/Tauri compatibility.
 */

import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useEventBus } from "@/providers/EventProvider";
import { PlanArtifactEventSchema } from "@/types/events";
import { useIdeationStore } from "@/stores/ideationStore";
import { ideationKeys } from "./useIdeation";
import { AGENT_ORCHESTRATOR } from "@/constants/agents";
import type { Artifact } from "@/types/artifact";
import type { IdeationSession } from "@/types/ideation";
import type { Unsubscribe } from "@/lib/event-bus";

/**
 * Hook to listen for plan artifact events from the backend
 *
 * Listens to 'plan_artifact:created' and 'plan_artifact:updated' events
 * and updates the ideation store/query cache accordingly. This enables
 * real-time plan artifact updates when the orchestrator creates or updates plans.
 *
 * @example
 * ```tsx
 * function App() {
 *   usePlanArtifactEvents(); // Sets up listener automatically
 *   return <AgentsView />;
 * }
 * ```
 */
export function usePlanArtifactEvents() {
  const bus = useEventBus();
  const setPlanArtifact = useIdeationStore((s) => s.setPlanArtifact);
  const updateSession = useIdeationStore((s) => s.updateSession);
  const activeSessionId = useIdeationStore((s) => s.activeSessionId);
  const sessions = useIdeationStore((s) => s.sessions);
  const queryClient = useQueryClient();

  // Keep all handler dependencies in refs so the effect doesn't
  // re-subscribe every time the sessions Record or store actions change.
  // activeSessionId is also ref'd to prevent event gaps during re-subscription.
  const sessionsRef = useRef<Record<string, IdeationSession>>(sessions);
  const setPlanArtifactRef = useRef(setPlanArtifact);
  const updateSessionRef = useRef(updateSession);
  const queryClientRef = useRef(queryClient);
  const activeSessionIdRef = useRef(activeSessionId);
  useEffect(() => { sessionsRef.current = sessions; }, [sessions]);
  useEffect(() => { setPlanArtifactRef.current = setPlanArtifact; }, [setPlanArtifact]);
  useEffect(() => { updateSessionRef.current = updateSession; }, [updateSession]);
  useEffect(() => { queryClientRef.current = queryClient; }, [queryClient]);
  useEffect(() => { activeSessionIdRef.current = activeSessionId; }, [activeSessionId]);

  // Dedup guard: skip duplicate events during subscribe/unsubscribe cycles
  const lastProcessedRef = useRef<string | null>(null);

  useEffect(() => {
    const unsubscribes: Unsubscribe[] = [];
    const invalidateAgentArtifacts = (
      ...artifactIds: Array<string | null | undefined>
    ) => {
      for (const artifactId of new Set(artifactIds.filter(Boolean) as string[])) {
        queryClientRef.current.invalidateQueries({
          queryKey: ["agents", "artifact", artifactId],
        });
      }
    };
    const invalidateAgentSessionPlan = (sessionId: string | null | undefined) => {
      if (!sessionId) {
        return;
      }
      queryClientRef.current.invalidateQueries({
        queryKey: ["agents", "session-plan", sessionId],
      });
      queryClientRef.current.invalidateQueries({
        queryKey: ["agents", "plan-approval", sessionId],
      });
    };

    // Listen for created events
    unsubscribes.push(
      bus.subscribe<unknown>("plan_artifact:created", (payload) => {
        const parsed = PlanArtifactEventSchema.safeParse({
          type: "created",
          ...(payload as Record<string, unknown>),
        });

        if (!parsed.success) {
          console.error(
            "Invalid plan_artifact:created event:",
            parsed.error.message
          );
          return;
        }

        if (parsed.data.type === "created") {
          const { sessionId, artifact } = parsed.data;

          // Dedup: skip if we already processed this exact event
          const eventKey = `created:${artifact.id}:${artifact.version}`;
          if (lastProcessedRef.current === eventKey) return;
          lastProcessedRef.current = eventKey;

          // Only update store if this is for the active session
          if (sessionId === activeSessionIdRef.current) {
            // Transform to Artifact type
            const planArtifact: Artifact = {
              id: artifact.id,
              type: "specification",
              name: artifact.name,
              content: { type: "inline", text: artifact.content },
              metadata: {
                createdAt: new Date().toISOString(),
                createdBy: AGENT_ORCHESTRATOR,
                version: artifact.version,
              },
              derivedFrom: [],
            };
            setPlanArtifactRef.current(planArtifact);
          }

          // Update session's planArtifactId so the subsequent `updated`
          // handler can match on it (avoids stale-null race).
          const session = sessionsRef.current[sessionId];
          const currentSeq = session?.planUpdateSeq ?? 0;
          if (session && session.planArtifactId !== artifact.id) {
            updateSessionRef.current(sessionId, {
              planArtifactId: artifact.id,
              planUpdateSeq: currentSeq + 1,
            });
          }

          // Invalidate the base session detail key so both the Ideation
          // with-data view and Agents header/artifact availability refetch.
          queryClientRef.current.invalidateQueries({
            queryKey: ideationKeys.sessionDetail(sessionId),
          });
          invalidateAgentSessionPlan(sessionId);
          invalidateAgentArtifacts(artifact.id);
        }
      })
    );

    // Listen for updated events
    unsubscribes.push(
      bus.subscribe<unknown>("plan_artifact:updated", (payload) => {
        const parsed = PlanArtifactEventSchema.safeParse({
          type: "updated",
          ...(payload as Record<string, unknown>),
        });

        if (!parsed.success) {
          console.error(
            "Invalid plan_artifact:updated event:",
            parsed.error.message
          );
          return;
        }

        if (parsed.data.type === "updated") {
          const { sessionId, artifactId, previousArtifactId, artifact } =
            parsed.data;

          // Dedup: skip if we already processed this exact event
          const eventKey = `updated:${artifact.id}:${artifact.version}`;
          if (lastProcessedRef.current === eventKey) return;
          lastProcessedRef.current = eventKey;

          const currentSessions = sessionsRef.current;
          const currentActiveSessionId = activeSessionIdRef.current;

          const planArtifact: Artifact = {
            id: artifact.id,
            type: "specification",
            name: artifact.name,
            content: { type: "inline", text: artifact.content },
            metadata: {
              createdAt: new Date().toISOString(),
              createdBy: AGENT_ORCHESTRATOR,
              version: artifact.version,
            },
            derivedFrom: [],
          };

          // Tier 1: sessionId from the backend is authoritative. Agents plan-mode
          // sessions may not be the global active Ideation session or present in
          // the ideation store, but their agent-specific plan queries still need
          // to refetch immediately.
          if (sessionId) {
            const session = currentSessions[sessionId];
            if (currentActiveSessionId === sessionId) {
              setPlanArtifactRef.current(planArtifact);
            }
            if (session) {
              const currentSeq = session.planUpdateSeq ?? 0;
              updateSessionRef.current(sessionId, {
                planArtifactId: artifact.id,
                planUpdateSeq: currentSeq + 1,
              });
            }
            queryClientRef.current.invalidateQueries({
              queryKey: ideationKeys.sessionDetail(sessionId),
            });
            invalidateAgentSessionPlan(sessionId);
            invalidateAgentArtifacts(artifact.id, artifactId, previousArtifactId);
            return;
          }

          // Tier 2: planArtifactId matching — fallback when sessionId absent/null
          // Match against previousArtifactId because the store's session
          // still holds the old artifact ID when this event arrives.
          // Immediately update planArtifactId so rapid subsequent events still match.
          // Also checks inheritedPlanArtifactId for followup sessions that inherit
          // a plan but never set planArtifactId themselves.
          let tier2Matched = false;
          for (const session of Object.values(currentSessions)) {
            const matchedOnOwn =
              session.planArtifactId === previousArtifactId ||
              session.planArtifactId === artifactId;
            const matchedOnInherited =
              !matchedOnOwn &&
              (session.inheritedPlanArtifactId === previousArtifactId ||
                session.inheritedPlanArtifactId === artifactId);

            if (matchedOnOwn || matchedOnInherited) {
              tier2Matched = true;
              const currentSeq = session.planUpdateSeq ?? 0;
              if (session.id === currentActiveSessionId) {
                setPlanArtifactRef.current(planArtifact);
              }
              if (matchedOnOwn && session.planArtifactId !== artifact.id) {
                updateSessionRef.current(session.id, {
                  planArtifactId: artifact.id,
                  planUpdateSeq: currentSeq + 1,
                });
              } else if (
                matchedOnInherited &&
                session.inheritedPlanArtifactId !== artifact.id
              ) {
                updateSessionRef.current(session.id, {
                  inheritedPlanArtifactId: artifact.id,
                  planUpdateSeq: currentSeq + 1,
                });
              }
              queryClientRef.current.invalidateQueries({
                queryKey: ideationKeys.sessionDetail(session.id),
              });
              invalidateAgentSessionPlan(session.id);
              invalidateAgentArtifacts(artifact.id, artifactId, previousArtifactId);
            }
          }

          // Tier 3: safety net — if nothing matched but we have an active session,
          // invalidate its query so it re-fetches and picks up the latest artifact
          if (!tier2Matched && currentActiveSessionId) {
            queryClientRef.current.invalidateQueries({
              queryKey: ideationKeys.sessionDetail(currentActiveSessionId),
            });
            invalidateAgentSessionPlan(currentActiveSessionId);
          }
        }
      })
    );

    return () => {
      unsubscribes.forEach((unsub) => unsub());
    };
  }, [bus]);
}
