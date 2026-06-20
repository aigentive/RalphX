import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import type { TicketRef, TicketingProvider } from "@/api/ticketing";

export type TicketingViewMode = "list" | "kanban";

export interface TicketingFilterState {
  text: string;
  assignee: string | null;
  stateIds: string[];
  labels: string[];
}

interface TicketingState {
  activeProvider: TicketingProvider | null;
  activeContainerId: string | null;
  viewMode: TicketingViewMode;
  filters: TicketingFilterState;
  selectedTicketRef: TicketRef | null;
}

interface TicketingActions {
  setProvider: (provider: TicketingProvider | null) => void;
  setContainerId: (containerId: string | null) => void;
  setViewMode: (mode: TicketingViewMode) => void;
  setFilters: (filters: Partial<TicketingFilterState>) => void;
  resetFilters: () => void;
  setSelectedTicketRef: (ticketRef: TicketRef | null) => void;
  reset: () => void;
}

const DEFAULT_FILTERS: TicketingFilterState = {
  text: "",
  assignee: null,
  stateIds: [],
  labels: [],
};

const INITIAL_STATE: TicketingState = {
  activeProvider: null,
  activeContainerId: null,
  viewMode: "list",
  filters: DEFAULT_FILTERS,
  selectedTicketRef: null,
};

function cloneFilters(filters: TicketingFilterState): TicketingFilterState {
  return {
    text: filters.text,
    assignee: filters.assignee,
    stateIds: [...filters.stateIds],
    labels: [...filters.labels],
  };
}

export const useTicketingStore = create<TicketingState & TicketingActions>()(
  immer((set) => ({
    ...INITIAL_STATE,
    filters: cloneFilters(DEFAULT_FILTERS),

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
        state.filters = {
          ...state.filters,
          ...filters,
          ...(filters.stateIds !== undefined && { stateIds: [...filters.stateIds] }),
          ...(filters.labels !== undefined && { labels: [...filters.labels] }),
        };
      }),

    resetFilters: () =>
      set((state) => {
        state.filters = cloneFilters(DEFAULT_FILTERS);
      }),

    setSelectedTicketRef: (ticketRef) =>
      set((state) => {
        state.selectedTicketRef = ticketRef;
      }),

    reset: () =>
      set((state) => {
        state.activeProvider = INITIAL_STATE.activeProvider;
        state.activeContainerId = INITIAL_STATE.activeContainerId;
        state.viewMode = INITIAL_STATE.viewMode;
        state.filters = cloneFilters(DEFAULT_FILTERS);
        state.selectedTicketRef = INITIAL_STATE.selectedTicketRef;
      }),
  })),
);
