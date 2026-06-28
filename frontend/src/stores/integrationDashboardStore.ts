import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

export type GitHubBranchAssociationFilter = "all" | "pull_requests" | "tickets" | "rx";
export type GitHubBranchPrStatusFilter = "all" | "open" | "draft" | "merged" | "closed" | "no_pr";
export type GranolaDashboardNoteFilter =
  | "all"
  | "with_summary"
  | "without_summary"
  | "with_rx"
  | "with_tickets"
  | "with_prs";

interface GitHubDashboardState {
  associationFilter: GitHubBranchAssociationFilter;
  statusFilter: GitHubBranchPrStatusFilter;
  searchQuery: string;
  selectedBranchName: string | null;
}

interface GranolaDashboardState {
  query: string;
  noteFilter: GranolaDashboardNoteFilter;
  selectedNoteId: string | null;
}

interface IntegrationDashboardState {
  githubByProject: Record<string, GitHubDashboardState>;
  granolaByProject: Record<string, GranolaDashboardState>;
}

interface IntegrationDashboardActions {
  setGitHubState: (projectId: string, patch: Partial<GitHubDashboardState>) => void;
  resetGitHubFilters: (projectId: string) => void;
  setGranolaState: (projectId: string, patch: Partial<GranolaDashboardState>) => void;
  resetGranolaFilters: (projectId: string) => void;
  reset: () => void;
}

export const DEFAULT_GITHUB_DASHBOARD_STATE: GitHubDashboardState = {
  associationFilter: "pull_requests",
  statusFilter: "all",
  searchQuery: "",
  selectedBranchName: null,
};

export const DEFAULT_GRANOLA_DASHBOARD_STATE: GranolaDashboardState = {
  query: "",
  noteFilter: "all",
  selectedNoteId: null,
};

function githubStateWithDefaults(
  state: GitHubDashboardState | undefined,
): GitHubDashboardState {
  return state ?? DEFAULT_GITHUB_DASHBOARD_STATE;
}

function granolaStateWithDefaults(
  state: GranolaDashboardState | undefined,
): GranolaDashboardState {
  return state ?? DEFAULT_GRANOLA_DASHBOARD_STATE;
}

export const useIntegrationDashboardStore = create<
  IntegrationDashboardState & IntegrationDashboardActions
>()(
  immer((set) => ({
    githubByProject: {},
    granolaByProject: {},

    setGitHubState: (projectId, patch) =>
      set((state) => {
        state.githubByProject[projectId] = {
          ...githubStateWithDefaults(state.githubByProject[projectId]),
          ...patch,
        };
      }),

    resetGitHubFilters: (projectId) =>
      set((state) => {
        state.githubByProject[projectId] = {
          ...githubStateWithDefaults(state.githubByProject[projectId]),
          associationFilter: DEFAULT_GITHUB_DASHBOARD_STATE.associationFilter,
          statusFilter: DEFAULT_GITHUB_DASHBOARD_STATE.statusFilter,
          searchQuery: DEFAULT_GITHUB_DASHBOARD_STATE.searchQuery,
        };
      }),

    setGranolaState: (projectId, patch) =>
      set((state) => {
        state.granolaByProject[projectId] = {
          ...granolaStateWithDefaults(state.granolaByProject[projectId]),
          ...patch,
        };
      }),

    resetGranolaFilters: (projectId) =>
      set((state) => {
        state.granolaByProject[projectId] = {
          ...granolaStateWithDefaults(state.granolaByProject[projectId]),
          query: DEFAULT_GRANOLA_DASHBOARD_STATE.query,
          noteFilter: DEFAULT_GRANOLA_DASHBOARD_STATE.noteFilter,
        };
      }),

    reset: () =>
      set((state) => {
        state.githubByProject = {};
        state.granolaByProject = {};
      }),
  })),
);
