import { create } from "zustand";
import { persist } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";

import {
  normalizeAgentRuntimeSelection,
  type AgentRuntimeSelection,
} from "@/lib/agent-models";

export type { AgentEffort, AgentProvider, AgentRuntimeSelection } from "@/lib/agent-models";

export type AgentArtifactTab = "plan" | "verification" | "proposal" | "tasks" | "publish";
export type AgentTaskArtifactMode = "graph" | "kanban";
export type AgentProjectSort = "latest" | "az" | "za";

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
const AGENT_SESSION_STORE_VERSION = 2;

function normalizeRuntimeRecord(value: unknown): Record<string, AgentRuntimeSelection> {
  if (!value || typeof value !== "object") {
    return {};
  }

  return Object.fromEntries(
    Object.entries(value).map(([key, runtime]) => [
      key,
      normalizeAgentRuntimeSelection(runtime),
    ])
  );
}

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

  const nextState: Record<string, unknown> = { ...persistedState };

  if (version < 1) {
    nextState.showAllProjects = DEFAULT_SHOW_ALL_PROJECTS;
  }

  if (version < 2) {
    nextState.runtimeByConversationId = normalizeRuntimeRecord(
      nextState.runtimeByConversationId
    );
    nextState.lastRuntimeByProjectId = normalizeRuntimeRecord(
      nextState.lastRuntimeByProjectId
    );
  }

  return nextState;
}

function ensureArtifactState(state: AgentSessionState, conversationId: string): AgentArtifactState {
  if (!state.artifactByConversationId[conversationId]) {
    state.artifactByConversationId[conversationId] = { ...DEFAULT_ARTIFACT_STATE };
  }
  return state.artifactByConversationId[conversationId];
}

function expandOnlyProject(state: AgentSessionState, projectId: string) {
  for (const expandedProjectId of Object.keys(state.expandedProjectIds)) {
    state.expandedProjectIds[expandedProjectId] = expandedProjectId === projectId;
  }
  state.expandedProjectIds[projectId] = true;
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
            expandOnlyProject(state, projectId);
          }
        }),

      selectConversation: (projectId, conversationId) =>
        set((state) => {
          state.focusedProjectId = projectId;
          state.selectedProjectId = projectId;
          state.selectedConversationId = conversationId;
          state.lastSelectedConversationByProjectId ??= {};
          state.lastSelectedConversationByProjectId[projectId] = conversationId;
          expandOnlyProject(state, projectId);
        }),

      clearSelection: () =>
        set((state) => {
          state.selectedProjectId = null;
          state.selectedConversationId = null;
        }),

      setProjectExpanded: (projectId, expanded) =>
        set((state) => {
          if (expanded) {
            expandOnlyProject(state, projectId);
          } else {
            state.expandedProjectIds[projectId] = false;
          }
        }),

      toggleProjectExpanded: (projectId) =>
        set((state) => {
          const nextExpanded = !(state.expandedProjectIds[projectId] ?? false);
          if (nextExpanded) {
            expandOnlyProject(state, projectId);
          } else {
            state.expandedProjectIds[projectId] = false;
          }
        }),

      setShowAllProjects: (showAllProjects) =>
        set((state) => {
          state.showAllProjects = showAllProjects;
        }),

      setProjectSort: (projectSort) =>
        set((state) => {
          state.projectSort = projectSort;
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
          const normalizedRuntime = normalizeAgentRuntimeSelection(runtime);
          state.runtimeByConversationId[conversationId] = normalizedRuntime;
          state.lastRuntimeByProjectId[projectId] = normalizedRuntime;
        }),

      setLastRuntimeForProject: (projectId, runtime) =>
        set((state) => {
          state.lastRuntimeByProjectId[projectId] =
            normalizeAgentRuntimeSelection(runtime);
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
