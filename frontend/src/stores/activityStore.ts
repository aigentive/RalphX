/**
 * Activity store using Zustand with immer middleware
 *
 * Manages agent activity messages and supervisor alerts. Uses a ring
 * buffer pattern to limit memory usage for high-frequency messages.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import type { AgentMessageEvent, SupervisorAlertEvent } from "@/types/events";

// ============================================================================
// Constants
// ============================================================================

/** Maximum number of messages to keep in the ring buffer */
const MAX_MESSAGES = 100;

// ============================================================================
// State Interface
// ============================================================================

interface ActivityState {
  /** Agent activity messages (ring buffer) */
  messages: AgentMessageEvent[];
  /** Supervisor alerts */
  alerts: SupervisorAlertEvent[];
  /** Timestamp of the most recent message (for pulsating Live indicator) */
  lastEventTime: number | null;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface ActivityActions {
  /** Add a message to the store (ring buffer) */
  addMessage: (message: AgentMessageEvent) => void;
  /** Clear all messages */
  clearMessages: () => void;
  /** Clear messages for a specific task */
  clearMessagesForTask: (taskId: string) => void;
  /** Add an alert to the store */
  addAlert: (alert: SupervisorAlertEvent) => void;
  /** Clear all alerts */
  clearAlerts: () => void;
  /** Clear alerts for a specific task */
  clearAlertsForTask: (taskId: string) => void;
  /** Dismiss a specific alert by index */
  dismissAlert: (index: number) => void;
  /** Clear both messages and alerts */
  clearAll: () => void;
  /** Get messages for a specific task */
  getMessagesForTask: (taskId: string) => AgentMessageEvent[];
  /** Get alerts for a specific task */
  getAlertsForTask: (taskId: string) => SupervisorAlertEvent[];
  /** Get alerts by severity */
  getAlertsBySeverity: (
    severity: SupervisorAlertEvent["severity"]
  ) => SupervisorAlertEvent[];
  /** Check if there are unread high/critical alerts */
  hasUnreadAlerts: () => boolean;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useActivityStore = create<ActivityState & ActivityActions>()(
  immer((set, get) => ({
    // Initial state
    messages: [],
    alerts: [],
    lastEventTime: null,

    // Actions
    addMessage: (message) =>
      set((state) => {
        state.messages.push(message);
        state.lastEventTime = Date.now();
        // Ring buffer: remove oldest messages if over limit
        if (state.messages.length > MAX_MESSAGES) {
          state.messages = state.messages.slice(-MAX_MESSAGES);
        }
      }),

    clearMessages: () =>
      set((state) => {
        state.messages = [];
      }),

    clearMessagesForTask: (taskId) =>
      set((state) => {
        state.messages = state.messages.filter((m) => m.taskId !== taskId);
      }),

    addAlert: (alert) =>
      set((state) => {
        state.alerts.push(alert);
      }),

    clearAlerts: () =>
      set((state) => {
        state.alerts = [];
      }),

    clearAlertsForTask: (taskId) =>
      set((state) => {
        state.alerts = state.alerts.filter((a) => a.taskId !== taskId);
      }),

    dismissAlert: (index) =>
      set((state) => {
        state.alerts.splice(index, 1);
      }),

    clearAll: () =>
      set((state) => {
        state.messages = [];
        state.alerts = [];
      }),

    // Selectors (using get() for computed values)
    getMessagesForTask: (taskId) =>
      get().messages.filter((m) => m.taskId === taskId),

    getAlertsForTask: (taskId) =>
      get().alerts.filter((a) => a.taskId === taskId),

    getAlertsBySeverity: (severity) =>
      get().alerts.filter((a) => a.severity === severity),

    hasUnreadAlerts: () =>
      get().alerts.some(
        (a) => a.severity === "high" || a.severity === "critical"
      ),
  }))
);

registerEnvIsolatedStore({ name: "useActivityStore", reset: () => useActivityStore.setState(useActivityStore.getInitialState(), true) });
