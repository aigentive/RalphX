import { create } from "zustand";
import { persist } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";

import { createEnvScopedStorage } from "@/lib/remote/env-scoped-storage";
import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import type { TicketRef, TicketingProvider } from "@/api/ticketing";

export type TicketingViewMode = "list" | "kanban";

export interface TicketingFilterState {
  text: string;
  assignees: string[];
  stateIds: string[];
  labels: string[];
  sprint: string | null;
  watcherMe: boolean;
}

interface TicketingState {
  activeProvider: TicketingProvider | null;
  activeContainerId: string | null;
  viewMode: TicketingViewMode;
  filters: TicketingFilterState;
  selectedTicketRef: TicketRef | null;
  /** Per-ticket "last opened" timestamps (ISO) keyed by ticketRefKey, persisted. */
  lastOpenedAt: Record<string, string>;
}

interface TicketingActions {
  setProvider: (provider: TicketingProvider | null) => void;
  setContainerId: (containerId: string | null) => void;
  setViewMode: (mode: TicketingViewMode) => void;
  setFilters: (filters: Partial<TicketingFilterState>) => void;
  resetFilters: () => void;
  setSelectedTicketRef: (ticketRef: TicketRef | null) => void;
  markTicketOpened: (ticketKey: string) => void;
  reset: () => void;
}

const DEFAULT_FILTERS: TicketingFilterState = {
  text: "",
  assignees: [],
  stateIds: [],
  labels: [],
  sprint: null,
  watcherMe: false,
};

const INITIAL_STATE: TicketingState = {
  activeProvider: null,
  activeContainerId: null,
  viewMode: "list",
  filters: DEFAULT_FILTERS,
  selectedTicketRef: null,
  lastOpenedAt: {},
};

function cloneFilters(filters: TicketingFilterState): TicketingFilterState {
  return {
    text: filters.text,
    assignees: [...filters.assignees],
    stateIds: [...filters.stateIds],
    labels: [...filters.labels],
    sprint: filters.sprint,
    watcherMe: filters.watcherMe,
  };
}

type PersistedTicketingFilters = Partial<TicketingFilterState> & {
  assignee?: string | null | undefined;
};

function normalizeFilters(filters: PersistedTicketingFilters): TicketingFilterState {
  const legacyAssignee = filters.assignee?.trim();
  return {
    text: filters.text ?? "",
    assignees:
      filters.assignees !== undefined
        ? [...filters.assignees]
        : legacyAssignee
          ? [legacyAssignee]
          : [],
    stateIds: filters.stateIds !== undefined ? [...filters.stateIds] : [],
    labels: filters.labels !== undefined ? [...filters.labels] : [],
    sprint: filters.sprint ?? null,
    watcherMe: filters.watcherMe ?? false,
  };
}

function migrateTicketingState(persistedState: unknown): unknown {
  if (!persistedState || typeof persistedState !== "object") {
    return persistedState;
  }
  const state = persistedState as Partial<TicketingState>;
  if (state.filters) {
    return {
      ...state,
      filters: normalizeFilters(state.filters as PersistedTicketingFilters),
    };
  }
  return state;
}

export const useTicketingStore = create<TicketingState & TicketingActions>()(
  persist(
    immer((set) => ({
      ...INITIAL_STATE,
      filters: cloneFilters(DEFAULT_FILTERS),
      lastOpenedAt: {},

      setProvider: (provider) =>
        set((state) => {
          if (state.activeProvider !== provider) {
            state.activeContainerId = null;
            state.selectedTicketRef = null;
          }
          state.activeProvider = provider;
        }),

      setContainerId: (containerId) =>
        set((state) => {
          if (state.activeContainerId !== containerId) {
            state.selectedTicketRef = null;
          }
          state.activeContainerId = containerId;
        }),

      setViewMode: (mode) =>
        set((state) => {
          state.viewMode = mode;
        }),

      setFilters: (filters) =>
        set((state) => {
          state.filters = normalizeFilters({ ...state.filters, ...filters });
        }),

      resetFilters: () =>
        set((state) => {
          state.filters = cloneFilters(DEFAULT_FILTERS);
        }),

      setSelectedTicketRef: (ticketRef) =>
        set((state) => {
          state.selectedTicketRef = ticketRef;
        }),

      markTicketOpened: (ticketKey) =>
        set((state) => {
          state.lastOpenedAt[ticketKey] = new Date().toISOString();
        }),

      reset: () =>
        set((state) => {
          state.activeProvider = INITIAL_STATE.activeProvider;
          state.activeContainerId = INITIAL_STATE.activeContainerId;
          state.viewMode = INITIAL_STATE.viewMode;
          state.filters = cloneFilters(DEFAULT_FILTERS);
          state.selectedTicketRef = INITIAL_STATE.selectedTicketRef;
          state.lastOpenedAt = {};
        }),
    })),
    {
      name: "ralphx-ticketing-store",
      storage: createEnvScopedStorage("ralphx-ticketing-store"),
      version: 2,
      migrate: migrateTicketingState,
      partialize: (state) => ({
        activeProvider: state.activeProvider,
        activeContainerId: state.activeContainerId,
        viewMode: state.viewMode,
        filters: cloneFilters(state.filters),
        selectedTicketRef: state.selectedTicketRef,
        lastOpenedAt: state.lastOpenedAt,
      }),
    },
  ),
);

registerEnvIsolatedStore({
  name: "useTicketingStore",
  reset: () => useTicketingStore.setState(useTicketingStore.getInitialState(), true),
  rehydrate: () => {
    void useTicketingStore.persist.rehydrate();
  },
});
