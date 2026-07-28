/**
 * Supervisor alerts Zustand store
 *
 * Manages supervisor alert state including alerts, configuration,
 * and connection status.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import type { SupervisorAlert, SupervisorConfig } from "@/types/supervisor";

// ============================================================================
// Constants
// ============================================================================

/** Maximum number of alerts to keep */
const MAX_ALERTS = 50;

/** Default supervisor configuration */
const DEFAULT_CONFIG: SupervisorConfig = {
  loopDetectionThreshold: 3,
  stuckTimeoutMinutes: 5,
  tokenWarningThreshold: 50000,
  timeWarningMinutes: 10,
  errorRepeatThreshold: 3,
};

// ============================================================================
// Store
// ============================================================================

interface SupervisorState {
  /** All supervisor alerts */
  alerts: SupervisorAlert[];
  /** Supervisor configuration */
  config: SupervisorConfig;
  /** Connected to event stream */
  isConnected: boolean;
}

interface SupervisorActions {
  /** Add a new alert */
  addAlert: (alert: Omit<SupervisorAlert, "id" | "acknowledged" | "createdAt">) => void;
  /** Acknowledge an alert by ID */
  acknowledgeAlert: (id: string) => void;
  /** Acknowledge all alerts */
  acknowledgeAll: () => void;
  /** Dismiss (remove) an alert by ID */
  dismissAlert: (id: string) => void;
  /** Dismiss all acknowledged alerts */
  dismissAcknowledged: () => void;
  /** Clear all alerts */
  clearAll: () => void;
  /** Clear alerts for a specific task */
  clearAlertsForTask: (taskId: string) => void;
  /** Update configuration */
  updateConfig: (config: Partial<SupervisorConfig>) => void;
  /** Set connection status */
  setConnected: (connected: boolean) => void;
}

export const useSupervisorStore = create<SupervisorState & SupervisorActions>()(
  immer((set) => ({
    // Initial state
    alerts: [],
    config: DEFAULT_CONFIG,
    isConnected: false,

    // Actions
    addAlert: (alertData) =>
      set((state) => {
        const alert: SupervisorAlert = {
          id: crypto.randomUUID(),
          ...alertData,
          acknowledged: false,
          createdAt: new Date().toISOString(),
        };
        state.alerts.unshift(alert); // Newest first
        // Limit alerts
        if (state.alerts.length > MAX_ALERTS) {
          state.alerts = state.alerts.slice(0, MAX_ALERTS);
        }
      }),

    acknowledgeAlert: (id) =>
      set((state) => {
        const alert = state.alerts.find((a) => a.id === id);
        if (alert) {
          alert.acknowledged = true;
          alert.acknowledgedAt = new Date().toISOString();
        }
      }),

    acknowledgeAll: () =>
      set((state) => {
        const now = new Date().toISOString();
        state.alerts.forEach((alert) => {
          if (!alert.acknowledged) {
            alert.acknowledged = true;
            alert.acknowledgedAt = now;
          }
        });
      }),

    dismissAlert: (id) =>
      set((state) => {
        state.alerts = state.alerts.filter((a) => a.id !== id);
      }),

    dismissAcknowledged: () =>
      set((state) => {
        state.alerts = state.alerts.filter((a) => !a.acknowledged);
      }),

    clearAll: () =>
      set((state) => {
        state.alerts = [];
      }),

    clearAlertsForTask: (taskId) =>
      set((state) => {
        state.alerts = state.alerts.filter((a) => a.taskId !== taskId);
      }),

    updateConfig: (config) =>
      set((state) => {
        state.config = { ...state.config, ...config };
      }),

    setConnected: (connected) =>
      set((state) => {
        state.isConnected = connected;
      }),
  }))
);

registerEnvIsolatedStore({ name: "useSupervisorStore", reset: () => useSupervisorStore.setState(useSupervisorStore.getInitialState(), true) });
