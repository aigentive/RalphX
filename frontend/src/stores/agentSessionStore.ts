import { create } from "zustand";
import { persist } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";

export type AgentProvider = "claude" | "codex";
export type AgentArtifactTab = "plan" | "verification" | "proposal" | "tasks" | "publish";
export type AgentTaskArtifactMode = "graph" | "kanban";
export type AgentProjectSort = "latest" | "az" | "za";
export type AgentSidebarGroupBy = "project" | "publication";
export type AgentSidebarPublicationState =
  | "active"
  | "draft"
  | "merged"
  | "closed"
  | "uncommitted"
  | "unpushed";

export interface AgentRuntimeSelection {
  provider: AgentProvider;
  modelId: string;
}

export interface AgentArtifactState {
  isOpen: boolean;
  activeTab: AgentArtifactTab;
  taskMode: AgentTaskArtifactMode;
}

interface AgentSessionState {
  focusedProjectId: string | null;
  selectedProjectId: string | null;
  selectedConversationId: string | null;
  lastSelectedConversationByProjectId: Record<string, string>;
  expandedProjectIds: Record<string, boolean>;
  showAllProjects: boolean;
  projectSort: AgentProjectSort;
  sidebarGroupBy: AgentSidebarGroupBy;
  sidebarProjectFilterIds: string[];
  sidebarPublicationStateFilters: AgentSidebarPublicationState[];
  pinnedConversationIds: Record<string, true>;
  artifactByConversationId: Record<string, AgentArtifactState>;
  runtimeByConversationId: Record<string, AgentRuntimeSelection>;
  lastRuntimeByProjectId: Record<string, AgentRuntimeSelection>;
}

interface AgentSessionActions {
  setFocusedProject: (projectId: string | null) => void;
  selectConversation: (projectId: string, conversationId: string) => void;
  clearSelection: () => void;
  setProjectExpanded: (projectId: string, expanded: boolean) => void;
  toggleProjectExpanded: (projectId: string) => void;
  setShowAllProjects: (showAllProjects: boolean) => void;
  setProjectSort: (projectSort: AgentProjectSort) => void;
  setSidebarGroupBy: (groupBy: AgentSidebarGroupBy) => void;
  setSidebarProjectFilterIds: (projectIds: string[]) => void;
  toggleSidebarProjectFilter: (projectId: string) => void;
  setSidebarPublicationStateFilters: (
    states: AgentSidebarPublicationState[]
  ) => void;
  toggleSidebarPublicationStateFilter: (
    state: AgentSidebarPublicationState
  ) => void;
  togglePinnedConversation: (conversationId: string) => void;
  setArtifactOpen: (conversationId: string, isOpen: boolean) => void;
  setArtifactTab: (conversationId: string, tab: AgentArtifactTab) => void;
  setArtifactState: (conversationId: string, artifactState: AgentArtifactState) => void;
  setTaskArtifactMode: (conversationId: string, mode: AgentTaskArtifactMode) => void;
  setRuntimeForConversation: (
    conversationId: string,
    projectId: string,
    runtime: AgentRuntimeSelection
  ) => void;
  setLastRuntimeForProject: (projectId: string, runtime: AgentRuntimeSelection) => void;
}

const DEFAULT_ARTIFACT_STATE: AgentArtifactState = {
  isOpen: false,
  activeTab: "plan",
  taskMode: "graph",
};
const DEFAULT_SHOW_ALL_PROJECTS = true;
const AGENT_SESSION_STORE_VERSION = 1;

export function migrateAgentSessionStore(
  persistedState: unknown,
  version: number,
) {
  if (
    version >= AGENT_SESSION_STORE_VERSION ||
    persistedState === null ||
    typeof persistedState !== "object"
  ) {
    return persistedState;
  }

  return {
    ...persistedState,
    showAllProjects: DEFAULT_SHOW_ALL_PROJECTS,
  };
}

function ensureArtifactState(state: AgentSessionState, conversationId: string): AgentArtifactState {
  if (!state.artifactByConversationId[conversationId]) {
    state.artifactByConversationId[conversationId] = { ...DEFAULT_ARTIFACT_STATE };
  }
  return state.artifactByConversationId[conversationId];
}

export const useAgentSessionStore = create<AgentSessionState & AgentSessionActions>()(
  persist(
    immer((set) => ({
      focusedProjectId: null,
      selectedProjectId: null,
      selectedConversationId: null,
      lastSelectedConversationByProjectId: {},
      expandedProjectIds: {},
      showAllProjects: DEFAULT_SHOW_ALL_PROJECTS,
      projectSort: "latest",
      sidebarGroupBy: "project",
      sidebarProjectFilterIds: [],
      sidebarPublicationStateFilters: [...DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS],
      pinnedConversationIds: {},
      artifactByConversationId: {},
      runtimeByConversationId: {},
      lastRuntimeByProjectId: {},

      setFocusedProject: (projectId) =>
        set((state) => {
          state.focusedProjectId = projectId;
          const lastConversationId = projectId
            ? (state.lastSelectedConversationByProjectId ?? {})[projectId]
            : null;
          if (projectId && lastConversationId) {
            state.selectedProjectId = projectId;
            state.selectedConversationId = lastConversationId;
          }
          if (projectId) {
            state.expandedProjectIds[projectId] = true;
          }
        }),

      selectConversation: (projectId, conversationId) =>
        set((state) => {
          state.focusedProjectId = projectId;
          state.selectedProjectId = projectId;
          state.selectedConversationId = conversationId;
          state.lastSelectedConversationByProjectId ??= {};
          state.lastSelectedConversationByProjectId[projectId] = conversationId;
          state.expandedProjectIds[projectId] = true;
        }),

      clearSelection: () =>
        set((state) => {
          state.selectedProjectId = null;
          state.selectedConversationId = null;
        }),

      setProjectExpanded: (projectId, expanded) =>
        set((state) => {
          state.expandedProjectIds[projectId] = expanded;
        }),

      toggleProjectExpanded: (projectId) =>
        set((state) => {
          state.expandedProjectIds[projectId] = !(state.expandedProjectIds[projectId] ?? true);
        }),

      setShowAllProjects: (showAllProjects) =>
        set((state) => {
          state.showAllProjects = showAllProjects;
        }),

      setProjectSort: (projectSort) =>
        set((state) => {
          state.projectSort = projectSort;
        }),

      setSidebarGroupBy: (groupBy) =>
        set((state) => {
          state.sidebarGroupBy = groupBy;
        }),

      setSidebarProjectFilterIds: (projectIds) =>
        set((state) => {
          state.sidebarProjectFilterIds = Array.from(new Set(projectIds));
        }),

      toggleSidebarProjectFilter: (projectId) =>
        set((state) => {
          state.showAllProjects = false;
          const current = new Set(state.sidebarProjectFilterIds);
          if (current.has(projectId)) {
            current.delete(projectId);
          } else {
            current.add(projectId);
          }
          state.sidebarProjectFilterIds = Array.from(current);
        }),

      setSidebarPublicationStateFilters: (states) =>
        set((state) => {
          state.sidebarPublicationStateFilters = Array.from(new Set(states));
        }),

      toggleSidebarPublicationStateFilter: (publicationState) =>
        set((state) => {
          const current = new Set(state.sidebarPublicationStateFilters);
          if (current.has(publicationState)) {
            current.delete(publicationState);
          } else {
            current.add(publicationState);
          }
          state.sidebarPublicationStateFilters = Array.from(current);
        }),

      togglePinnedConversation: (conversationId) =>
        set((state) => {
          if (state.pinnedConversationIds[conversationId]) {
            delete state.pinnedConversationIds[conversationId];
          } else {
            state.pinnedConversationIds[conversationId] = true;
          }
        }),

      setArtifactOpen: (conversationId, isOpen) =>
        set((state) => {
          ensureArtifactState(state, conversationId).isOpen = isOpen;
        }),

      setArtifactTab: (conversationId, tab) =>
        set((state) => {
          const artifactState = ensureArtifactState(state, conversationId);
          artifactState.activeTab = tab;
          artifactState.isOpen = true;
        }),

      setArtifactState: (conversationId, artifactState) =>
        set((state) => {
          state.artifactByConversationId[conversationId] = { ...artifactState };
        }),

      setTaskArtifactMode: (conversationId, mode) =>
        set((state) => {
          ensureArtifactState(state, conversationId).taskMode = mode;
        }),

      setRuntimeForConversation: (conversationId, projectId, runtime) =>
        set((state) => {
          state.runtimeByConversationId[conversationId] = runtime;
          state.lastRuntimeByProjectId[projectId] = runtime;
        }),

      setLastRuntimeForProject: (projectId, runtime) =>
        set((state) => {
          state.lastRuntimeByProjectId[projectId] = runtime;
        }),
    })),
    {
      name: "ralphx-agent-session-store",
      version: AGENT_SESSION_STORE_VERSION,
      migrate: migrateAgentSessionStore,
      partialize: (state) => ({
        focusedProjectId: state.focusedProjectId,
        selectedProjectId: state.selectedProjectId,
        selectedConversationId: state.selectedConversationId,
        lastSelectedConversationByProjectId: state.lastSelectedConversationByProjectId,
        expandedProjectIds: state.expandedProjectIds,
        showAllProjects: state.showAllProjects,
        projectSort: state.projectSort,
        sidebarGroupBy: state.sidebarGroupBy,
        sidebarProjectFilterIds: state.sidebarProjectFilterIds,
        sidebarPublicationStateFilters: state.sidebarPublicationStateFilters,
        pinnedConversationIds: state.pinnedConversationIds,
        artifactByConversationId: state.artifactByConversationId,
        runtimeByConversationId: state.runtimeByConversationId,
        lastRuntimeByProjectId: state.lastRuntimeByProjectId,
      }),
    }
  )
);

export function selectArtifactState(conversationId: string | null) {
  return (state: AgentSessionState): AgentArtifactState =>
    conversationId
      ? state.artifactByConversationId[conversationId] ?? DEFAULT_ARTIFACT_STATE
      : DEFAULT_ARTIFACT_STATE;
}

export function selectHasStoredArtifactState(conversationId: string | null) {
  return (state: AgentSessionState): boolean =>
    conversationId
      ? Object.prototype.hasOwnProperty.call(state.artifactByConversationId, conversationId)
      : false;
}
