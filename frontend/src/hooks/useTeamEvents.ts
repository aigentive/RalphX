/**
 * useTeamEvents — Team lifecycle event consumer
 *
 * Subscribes to team:* events and agent:* events with teammate_name.
 * Routes events to teamStore actions, filtered by contextKey.
 *
 * Split into two effects to avoid event subscription race conditions:
 *   Effect 1 (always active): team:created, team:disbanded, team:plan_requested,
 *     and team:teammate_spawned — runs whenever contextKey is non-null so
 *     creation and spawn events are never missed (backend emits them synchronously).
 *   Effect 2 (gated by isTeamActive): agent:* and remaining team:* events —
 *     only subscribes once the team exists in the store.
 *
 * Uses EventBus abstraction for browser/Tauri compatibility.
 */

import { useCallback, useEffect } from "react";
import { toast } from "sonner";
import { useEventBus } from "@/providers/EventProvider";
import { useTeamStore } from "@/stores/teamStore";
import { useChatStore } from "@/stores/chatStore";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { getPendingPlans } from "@/api/team";
import type { ContextType } from "@/types/chat-conversation";
import type { Unsubscribe } from "@/lib/event-bus";
import type {
  AgentRunCompletedPayload,
  AgentRunStartedPayload,
  TeamArtifactCreatedPayload,
  TeamCreatedPayload,
  TeamDisbandedPayload,
  TeamTeammateSpawnedPayload,
  TeamTeammateIdlePayload,
  TeamTeammateShutdownPayload,
  TeamMessagePayload,
  TeamCostUpdatePayload,
  TeamPlanRequestedPayload,
  TeamPlanResolvedPayload,
  TeamPlanAutoApprovedPayload,
} from "@/types/events";

export function useTeamEvents(contextKey: string | null) {
  const bus = useEventBus();
  const createTeam = useTeamStore((s) => s.createTeam);
  const addTeammate = useTeamStore((s) => s.addTeammate);
  const updateTeammateStatus = useTeamStore((s) => s.updateTeammateStatus);
  const setTeammateConversationId = useTeamStore((s) => s.setTeammateConversationId);
  const updateTeammateCost = useTeamStore((s) => s.updateTeammateCost);
  const addTeamMessage = useTeamStore((s) => s.addTeamMessage);
  const disbandTeam = useTeamStore((s) => s.disbandTeam);
  const setTeamActive = useChatStore((s) => s.setTeamActive);
  const setPendingPlan = useTeamStore((s) => s.setPendingPlan);
  const clearPendingPlan = useTeamStore((s) => s.clearPendingPlan);
  const clearPendingPlanIfMatches = useTeamStore((s) => s.clearPendingPlanIfMatches);
  const bumpArtifactVersion = useTeamStore((s) => s.bumpArtifactVersion);

  // Derive contextId from contextKey (format: "prefix:contextId")
  const contextId = contextKey ? contextKey.split(":").slice(1).join(":") : null;

  // Derive isTeamActive from the teamStore so effect 2 re-runs when team is created
  const selectActive = useCallback(
    (s: { activeTeams: Record<string, unknown> }) =>
      contextKey ? contextKey in s.activeTeams : false,
    [contextKey],
  );
  const isTeamActive = useTeamStore(selectActive);

  // Helper: build key from event payload and check match
  const matchKey = useCallback(
    (payload: { context_type: string; context_id: string }): boolean => {
      if (!contextKey) return false;
      const key = buildStoreKey(payload.context_type as ContextType, payload.context_id);
      return key === contextKey;
    },
    [contextKey],
  );

  // ── Effect 1: Always subscribe to lifecycle events ──────────────────────
  // These must fire even before a team exists so we catch the creation event.
  // Includes team:teammate_spawned to avoid race condition with team:created.
  useEffect(() => {
    if (!contextKey) return;

    const unsubs: Unsubscribe[] = [];

    // team:created — lead_name may not be in payload; default to team_name
    unsubs.push(
      bus.subscribe<TeamCreatedPayload & { lead_name?: string }>("team:created", (payload) => {
        if (matchKey(payload)) {
          createTeam(contextKey, payload.team_name, payload.lead_name ?? payload.team_name);
          setTeamActive(contextKey, true);
        }
      }),
    );

    // team:disbanded — mark team as historical and close isTeamActive gate
    // setTeamActive(false) ensures all team UI elements hide after disbandment
    unsubs.push(
      bus.subscribe<TeamDisbandedPayload>("team:disbanded", (payload) => {
        if (matchKey(payload)) {
          disbandTeam(contextKey);
          setTeamActive(contextKey, false);
        }
      }),
    );

    // team:plan_requested — show approval UI, filtered to current context
    unsubs.push(
      bus.subscribe<TeamPlanRequestedPayload>("team:plan_requested", (payload) => {
        if (payload.validated && matchKey(payload)) {
          setPendingPlan(contextKey, {
            planId: payload.plan_id,
            process: payload.process,
            teammates: payload.teammates,
            originContextType: payload.context_type,
            originContextId: payload.context_id,
            createdAt: Date.now(),
          });
        }
      }),
    );

    // Clear only the plan that reached a terminal backend state. A late event
    // for an older plan must not remove its replacement from the approval UI.
    unsubs.push(
      bus.subscribe<TeamPlanResolvedPayload>("team:plan_resolved", (payload) => {
        if (matchKey(payload)) {
          clearPendingPlanIfMatches(contextKey, payload.plan_id);
        }
      }),
    );

    // team:teammate_spawned — moved to Effect 1 to avoid race condition.
    // Backend emits this immediately after team:created, so we must subscribe
    // before Effect 2 has a chance to re-run.
    unsubs.push(
      bus.subscribe<TeamTeammateSpawnedPayload>("team:teammate_spawned", (payload) => {
        if (matchKey(payload)) {
          addTeammate(contextKey, {
            name: payload.teammate_name,
            color: payload.color,
            model: payload.model,
            roleDescription: payload.role,
            status: "spawning",
            currentActivity: null,
            tokensUsed: 0,
            estimatedCostUsd: 0,
            conversationId: payload.conversation_id ?? null,
          });
        }
      }),
    );

    // team:artifact_created — moved to Effect 1 so bumpArtifactVersion fires
    // even when no team:created event happened first (e.g. MCP-created artifacts).
    unsubs.push(
      bus.subscribe<TeamArtifactCreatedPayload>("team:artifact_created", (payload) => {
        // contextKey format: "prefix:contextId" — extract contextId for matching
        const contextId = contextKey.split(":").slice(1).join(":");
        if (payload.session_id === contextId) {
          bumpArtifactVersion(payload.session_id);
        }
      }),
    );

    // team:plan_auto_approved — backend auto-approved the plan; show info toast.
    // Defensively clear any pending plan to prevent phantom approval dialog.
    // Do NOT call setPendingPlan — team panel already appears via team:created + team:teammate_spawned.
    unsubs.push(
      bus.subscribe<TeamPlanAutoApprovedPayload>("team:plan_auto_approved", (payload) => {
        if (matchKey(payload)) {
          clearPendingPlan(contextKey);
          toast.info(`Team plan auto-approved — ${payload.teammates_spawned.length} teammate${payload.teammates_spawned.length !== 1 ? "s" : ""} spawning`);
        }
      }),
    );

    return () => unsubs.forEach((u) => u());
  }, [bus, contextKey, matchKey, createTeam, disbandTeam, setTeamActive, setPendingPlan, clearPendingPlan, clearPendingPlanIfMatches, addTeammate, bumpArtifactVersion]);

  // ── Effect 3: Mount-time hydration — restore pending plans on mount ───────
  // Handles app restarts or remounts where plan was pending before component mounted.
  useEffect(() => {
    if (!contextKey || !contextId) return;

    getPendingPlans(contextId).then((plans) => {
      // Restore the most recent plan (first in array)
      const plan = plans[0];
      if (plan) {
        setPendingPlan(contextKey, {
          planId: plan.plan_id,
          process: plan.process,
          teammates: plan.teammates.map((t) => ({
            role: t.role,
            model: t.model,
            tools: t.tools,
            mcp_tools: t.mcp_tools,
            prompt_summary: t.prompt_summary,
            ...(t.preset !== undefined && { preset: t.preset }),
          })),
          originContextType: plan.context_type,
          originContextId: plan.context_id,
          createdAt: plan.created_at_ms,
        });
      }
    }).catch(() => {
      // Non-critical — event listener is the primary delivery path
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [contextKey, contextId]);

  // ── Effect 4: Visibility reconciliation — detect stale plans on focus ─────
  // Mirrors useAskUserQuestion.ts reconciliation pattern.
  // If the agent died via TTL while app was backgrounded, no events were emitted.
  useEffect(() => {
    if (!contextKey || !contextId) return undefined;

    let debounceTimer: ReturnType<typeof setTimeout> | null = null;

    function handleVisibilityChange() {
      if (document.visibilityState !== "visible") return;

      // Only check if there's a pending plan for this contextKey
      const pendingPlan = useTeamStore.getState().pendingPlans[contextKey!];
      if (!pendingPlan) return;

      const preCallPlanId = pendingPlan.planId;

      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        getPendingPlans(contextId!).then((plans) => {
          const stillPending = plans.some((p) => p.plan_id === preCallPlanId);
          if (!stillPending) {
            // Verify plan wasn't replaced by a new event during the API call
            const current = useTeamStore.getState().pendingPlans[contextKey!];
            if (current && current.planId === preCallPlanId) {
              clearPendingPlan(contextKey!);
            }
          }
        }).catch(() => {
          // Non-critical — don't disrupt UX
        });
      }, 500);
    }

    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      if (debounceTimer) clearTimeout(debounceTimer);
    };
  }, [contextKey, contextId, clearPendingPlan]);

  // ── Effect 2: Subscribe to remaining events when team is active ──────────
  useEffect(() => {
    if (!contextKey || !isTeamActive) return;

    const unsubs: Unsubscribe[] = [];

    // agent:run_started — route to teammate status when teammate_name present
    // Also capture conversation_id if present (may arrive before team:teammate_spawned)
    unsubs.push(
      bus.subscribe<AgentRunStartedPayload>("agent:run_started", (payload) => {
        if (payload.teammate_name && matchKey(payload)) {
          updateTeammateStatus(contextKey, payload.teammate_name, "running");
          if (payload.conversation_id) {
            setTeammateConversationId(contextKey, payload.teammate_name, payload.conversation_id);
          }
        }
      }),
    );

    // agent:run_completed — teammate idle
    unsubs.push(
      bus.subscribe<AgentRunCompletedPayload>("agent:run_completed", (payload) => {
        if (payload.teammate_name && matchKey(payload)) {
          updateTeammateStatus(contextKey, payload.teammate_name, "idle");
        }
      }),
    );

    // team:teammate_idle
    unsubs.push(
      bus.subscribe<TeamTeammateIdlePayload & { last_activity?: string }>("team:teammate_idle", (payload) => {
        if (matchKey(payload)) {
          updateTeammateStatus(
            contextKey,
            payload.teammate_name,
            "idle",
            payload.last_activity,
          );
        }
      }),
    );

    // team:message — backend sends `sender`/`recipient`, not `from`/`to`
    unsubs.push(
      bus.subscribe<TeamMessagePayload>("team:message", (payload) => {
        if (matchKey(payload)) {
          addTeamMessage(contextKey, {
            id: payload.message_id ?? `msg-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
            from: payload.sender,
            to: payload.recipient ?? "*",
            // eslint-disable-next-line no-control-regex
            content: payload.content.replace(/\u001b\[[0-9;]*[A-Za-z]/g, ""),
            timestamp: payload.timestamp,
          });
        }
      }),
    );

    // team:cost_update — Claude processed tokens include input, output, and cache traffic.
    unsubs.push(
      bus.subscribe<TeamCostUpdatePayload>("team:cost_update", (payload) => {
        if (matchKey(payload)) {
          updateTeammateCost(
            contextKey,
            payload.teammate_name,
            payload.input_tokens +
              payload.output_tokens +
              payload.cache_creation_tokens +
              payload.cache_read_tokens,
            payload.estimated_usd,
          );
        }
      }),
    );

    // team:teammate_shutdown
    unsubs.push(
      bus.subscribe<TeamTeammateShutdownPayload>("team:teammate_shutdown", (payload) => {
        if (matchKey(payload)) {
          updateTeammateStatus(contextKey, payload.teammate_name, "shutdown");
        }
      }),
    );

    return () => unsubs.forEach((u) => u());
  }, [
    bus, contextKey, isTeamActive, matchKey,
    updateTeammateStatus, setTeammateConversationId,
    updateTeammateCost,
    addTeamMessage,
  ]);
}
