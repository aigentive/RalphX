/**
 * useAgentHookEvents — Listen for agent:hook Tauri events and maintain hook state.
 *
 * Subscribes to AGENT_HOOK events via EventBus, transforms raw snake_case payloads
 * to camelCase HookEvent types, and manages three collections:
 * - activeHooks: hooks currently running (started but no completed yet)
 * - events: chronological list of all hook events (completed + blocks)
 *
 * When a "completed" event arrives, the matching "started" entry is removed from
 * activeHooks and the completed event is added to the events list.
 */

import { useEffect, useCallback, useRef } from "react";
import { useEventBus } from "@/providers/EventProvider";
import { AGENT_HOOK } from "@/lib/events";
import type {
  HookEvent,
  HookStartedEvent,
  HookCompletedEvent,
  HookBlockEvent,
  RawAgentHookPayload,
} from "@/types/hook-event";
import { create } from "zustand";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";

// ============================================================================
// Store
// ============================================================================

interface HookEventsState {
  /** Hooks currently running (started, awaiting completed) — keyed by hookId */
  activeHooks: Map<string, HookStartedEvent>;
  /** All resolved hook events in chronological order (completed + blocks) */
  events: HookEvent[];
  /** Add a started hook */
  addStarted: (event: HookStartedEvent) => void;
  /** Resolve a started hook → completed */
  resolveCompleted: (event: HookCompletedEvent) => void;
  /** Add a block event */
  addBlock: (event: HookBlockEvent) => void;
  /** Clear all state (e.g. conversation changed) */
  clear: () => void;
}

export const useHookEventsStore = create<HookEventsState>((set) => ({
  activeHooks: new Map(),
  events: [],

  addStarted: (event) =>
    set((state) => {
      const next = new Map(state.activeHooks);
      next.set(event.hookId, event);
      return { activeHooks: next };
    }),

  resolveCompleted: (event) =>
    set((state) => {
      const next = new Map(state.activeHooks);
      next.delete(event.hookId);
      return {
        activeHooks: next,
        events: [...state.events, event],
      };
    }),

  addBlock: (event) =>
    set((state) => ({
      events: [...state.events, event],
    })),

  clear: () =>
    set({ activeHooks: new Map(), events: [] }),
}));

registerEnvIsolatedStore({ name: "useHookEventsStore", reset: () => useHookEventsStore.setState(useHookEventsStore.getInitialState(), true) });

// ============================================================================
// Transform
// ============================================================================

function transformPayload(raw: RawAgentHookPayload): HookEvent | null {
  const base = {
    conversationId: raw.conversation_id,
    contextType: raw.context_type,
    contextId: raw.context_id,
    timestamp: raw.timestamp,
  };

  switch (raw.type) {
    case "started":
      if (!raw.hook_name || !raw.hook_event || !raw.hook_id) return null;
      return {
        ...base,
        type: "started",
        hookName: raw.hook_name,
        hookEvent: raw.hook_event,
        hookId: raw.hook_id,
      };

    case "completed":
      if (!raw.hook_name || !raw.hook_event || !raw.hook_id) return null;
      return {
        ...base,
        type: "completed",
        hookName: raw.hook_name,
        hookEvent: raw.hook_event,
        hookId: raw.hook_id,
        output: raw.output ?? null,
        outcome: raw.outcome ?? null,
        exitCode: raw.exit_code ?? null,
      };

    case "block":
      if (!raw.reason) return null;
      return {
        ...base,
        type: "block",
        hookName: raw.hook_name ?? null,
        reason: raw.reason,
      };

    default:
      return null;
  }
}

// ============================================================================
// Hook
// ============================================================================

/**
 * Subscribe to agent:hook events for a specific conversation.
 *
 * @param conversationId - Only process events matching this conversation. Pass null to skip subscription.
 */
export function useAgentHookEvents(conversationId: string | null) {
  const bus = useEventBus();
  const { addStarted, resolveCompleted, addBlock, clear } = useHookEventsStore();
  const prevConversationId = useRef(conversationId);

  // Clear state when conversation changes
  useEffect(() => {
    if (prevConversationId.current !== conversationId) {
      clear();
      prevConversationId.current = conversationId;
    }
  }, [conversationId, clear]);

  const handleHookEvent = useCallback(
    (raw: RawAgentHookPayload) => {
      if (!conversationId || raw.conversation_id !== conversationId) return;

      const event = transformPayload(raw);
      if (!event) return;

      switch (event.type) {
        case "started":
          addStarted(event);
          break;
        case "completed":
          resolveCompleted(event);
          break;
        case "block":
          addBlock(event);
          break;
      }
    },
    [conversationId, addStarted, resolveCompleted, addBlock]
  );

  useEffect(() => {
    if (!conversationId) return;

    const unsub = bus.subscribe<RawAgentHookPayload>(AGENT_HOOK, handleHookEvent);
    return unsub;
  }, [bus, conversationId, handleHookEvent]);
}
