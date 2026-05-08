/* eslint-disable react-refresh/only-export-components */
/**
 * EventProvider - Global event listener setup and event bus context
 *
 * This component:
 * 1. Provides an EventBus instance via React context
 * 2. Sets up global Tauri event listeners for the application
 *
 * The EventBus abstraction allows switching between:
 * - TauriEventBus: Real Tauri events in native mode
 * - MockEventBus: In-memory events for browser testing
 *
 * It should wrap the main App content to ensure events are captured
 * throughout the application lifecycle.
 */

import { createContext, useContext, useMemo, useEffect, type ReactNode } from "react";
import {
  useTaskEvents,
  useSupervisorAlerts,
  useReviewEvents,
  useFileChangeEvents,
  useAgentEvents,
  useProposalEvents,
  useStepEvents,
  useExecutionErrorEvents,
  useRecoveryPromptEvents,
} from "@/hooks/useEvents";
import { useIdeationEvents } from "@/hooks/useIdeationEvents";
import { usePlanArtifactEvents } from "@/hooks/useEvents.planArtifact";
import { useVerificationEvents } from "@/hooks/useVerificationEvents";
import { useVerificationBootstrap } from "@/hooks/useVerificationBootstrap";
import { useFreshnessBlockedNotification } from "@/hooks/useFreshnessBlockedNotification";
import { useGitAuthStartupNotification } from "@/hooks/useGitAuthStartupNotification";
import { useGlobalAgentLifecycle } from "@/hooks/useGlobalAgentLifecycle";
import { createEventBus, type EventBus } from "@/lib/event-bus";

/**
 * Context for the event bus instance
 */
const EventBusContext = createContext<EventBus | null>(null);

/**
 * Hook to access the event bus from context
 *
 * @returns The EventBus instance (TauriEventBus or MockEventBus)
 * @throws Error if used outside of EventProvider
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const bus = useEventBus();
 *
 *   useEffect(() => {
 *     return bus.subscribe('my:event', (payload) => {
 *       console.log('Received:', payload);
 *     });
 *   }, [bus]);
 * }
 * ```
 */
export function useEventBus(): EventBus {
  const bus = useContext(EventBusContext);
  if (!bus) {
    throw new Error("useEventBus must be used within an EventProvider");
  }
  return bus;
}

/**
 * Internal component that sets up global event listeners.
 *
 * This is a child of the EventBusContext.Provider so the hooks
 * can use useEventBus() to access the bus instance.
 */
function GlobalEventListeners({ children }: { children: ReactNode }) {
  // Set up global event listeners using the EventBus from context
  useTaskEvents();
  useProposalEvents(); // Listen to proposal events for Ideation view
  useStepEvents(); // Listen to step events for task execution progress
  useSupervisorAlerts();
  useReviewEvents();
  useFileChangeEvents();
  useAgentEvents(); // Listen to agent:message events for Activity view (no active conversation)
  useExecutionErrorEvents(); // Handle agent execution errors and unstick UI
  useRecoveryPromptEvents(); // Listen to recovery prompts
  useIdeationEvents(); // Listen to ideation events (session title updates)
  usePlanArtifactEvents(); // Listen to plan artifact events for real-time updates
  useVerificationEvents(); // Listen to plan verification status changes globally
  useVerificationBootstrap(); // Hydrate pending verification confirmations on startup and project switch
  useFreshnessBlockedNotification(); // Show toast when task is freshness-blocked
  useGitAuthStartupNotification(); // Warn before Git/GitHub-dependent startup work fails
  useGlobalAgentLifecycle(); // Global agent lifecycle → agentStatus for all sessions

  return <>{children}</>;
}

interface EventProviderProps {
  children: ReactNode;
}

/**
 * Global event provider component
 *
 * Sets up all global event listeners:
 * - Task events (created, updated, deleted, status_changed)
 * - Proposal events (created, updated, deleted)
 * - Step events (created, updated, deleted, reordered)
 * - Supervisor alerts
 * - Review events (placeholder for Phase 9)
 * - File change events (placeholder)
 * - Ideation events (session title updates from ralphx-utility-session-namer agent)
 *
 * @example
 * ```tsx
 * function App() {
 *   return (
 *     <QueryClientProvider client={queryClient}>
 *       <EventProvider>
 *         <Router>
 *           <Routes />
 *         </Router>
 *       </EventProvider>
 *     </QueryClientProvider>
 *   );
 * }
 * ```
 */
export function EventProvider({ children }: EventProviderProps) {
  // Create event bus once based on environment (Tauri or browser mode)
  const eventBus = useMemo(() => createEventBus(), []);

  // Expose event bus to window in web mode for Playwright testing
  useEffect(() => {
    if (typeof window !== 'undefined' && !window.__TAURI_INTERNALS__) {
      window.__eventBus = eventBus;
    }
  }, [eventBus]);

  return (
    <EventBusContext.Provider value={eventBus}>
      <GlobalEventListeners>{children}</GlobalEventListeners>
    </EventBusContext.Provider>
  );
}
