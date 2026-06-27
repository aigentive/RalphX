import { create } from "zustand";
import { persist } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";

import {
  normalizeAgentRuntimeSelection,
  type AgentEffort,
  type AgentProvider,
  type AgentRuntimeSelection,
} from "@/lib/agent-models";
import type {
  AgentConversationWorkspaceMode,
  ComposerArtifactReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
} from "@/api/chat";
import type { BranchBaseOption } from "@/components/shared/branchBaseOptions";

export type { AgentEffort, AgentProvider, AgentRuntimeSelection } from "@/lib/agent-models";

export type AgentArtifactTab =
  | "review"
  | "issues"
  | "plan"
  | "verification"
  | "proposal"
  | "tasks"
  | "jira"
  | "linear"
  | "granola"
  | "publish";
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

export interface AgentArtifactState {
  isOpen: boolean;
  activeTab: AgentArtifactTab;
  taskMode: AgentTaskArtifactMode;
}

export interface AgentBranchBaseCacheEntry {
  options: BranchBaseOption[];
  selectedKey: string;
  loadedAt: string;
}

export interface AgentStartConversationDraft {
  projectId: string;
  content: string;
  mode: AgentConversationWorkspaceMode;
  composerArtifactReferences?: ComposerArtifactReference[];
  composerProjectReferences?: ComposerProjectReference[];
  composerIntegrationReferences?: ComposerIntegrationReference[];
}

interface AgentSessionState {
  focusedProjectId: string | null;
  selectedProjectId: string | null;
  selectedConversationId: string | null;
  startConversationDraft: AgentStartConversationDraft | null;
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
  branchBaseCacheByProjectId: Record<string, AgentBranchBaseCacheEntry>;
  lastBranchBaseSelectionByProjectId: Record<string, string>;
  lastModelEffortByProvider: Record<AgentProvider, { modelId: string; effort: AgentEffort }>;
}

interface AgentSessionActions {
  setFocusedProject: (projectId: string | null) => void;
  selectConversation: (projectId: string, conversationId: string) => void;
  clearSelection: () => void;
  setStartConversationDraft: (draft: AgentStartConversationDraft) => void;
  consumeStartConversationDraft: () => AgentStartConversationDraft | null;
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
  setBranchBaseCacheForProject: (
    projectId: string,
    options: BranchBaseOption[],
    selectedKey: string
  ) => void;
  setLastBranchBaseSelectionForProject: (projectId: string, selectedKey: string) => void;
}

const DEFAULT_ARTIFACT_STATE: AgentArtifactState = {
  isOpen: false,
  activeTab: "plan",
  taskMode: "graph",
};
const DEFAULT_SHOW_ALL_PROJECTS = true;
export const DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS: AgentSidebarPublicationState[] = [
  "active",
  "draft",
  "merged",
  "closed",
  "uncommitted",
  "unpushed",
];
const AGENT_SESSION_STORE_VERSION = 6;

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

  if (version < 3) {
    nextState.branchBaseCacheByProjectId = {};
    nextState.lastBranchBaseSelectionByProjectId = {};
  }

  if (version < 4) {
    nextState.sidebarGroupBy = "project";
    nextState.sidebarProjectFilterIds = [];
    nextState.sidebarPublicationStateFilters = [
      ...DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS,
    ];
    nextState.pinnedConversationIds = {};
  }

  if (version < 5) {
    nextState.lastModelEffortByProvider = {};
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

function cloneStartConversationDraft(
  draft: AgentStartConversationDraft,
): AgentStartConversationDraft {
  return {
    projectId: draft.projectId,
    content: draft.content,
    mode: draft.mode,
    ...(draft.composerProjectReferences
      ? {
          composerProjectReferences: draft.composerProjectReferences.map(
            (reference) => ({ ...reference }),
          ),
        }
      : {}),
    ...(draft.composerIntegrationReferences
      ? {
          composerIntegrationReferences: draft.composerIntegrationReferences.map(
            (reference) => ({ ...reference }),
          ),
        }
      : {}),
    ...(draft.composerArtifactReferences
      ? {
          composerArtifactReferences: draft.composerArtifactReferences.map(
            (reference) => ({ ...reference }),
          ),
        }
      : {}),
  };
}

export const useAgentSessionStore = create<AgentSessionState & AgentSessionActions>()(
  persist(
    immer((set) => ({
      focusedProjectId: null,
      selectedProjectId: null,
      selectedConversationId: null,
      startConversationDraft: null,
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
      branchBaseCacheByProjectId: {},
      lastBranchBaseSelectionByProjectId: {},
      lastModelEffortByProvider: {} as Record<AgentProvider, { modelId: string; effort: AgentEffort }>,

      setFocusedProject: (projectId) =>
        set((state) => {
          state.focusedProjectId = projectId;
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

      setStartConversationDraft: (draft) =>
        set((state) => {
          state.startConversationDraft = draft;
        }),

      consumeStartConversationDraft: () => {
        let consumedDraft: AgentStartConversationDraft | null = null;
        set((state) => {
          consumedDraft = state.startConversationDraft
            ? cloneStartConversationDraft(state.startConversationDraft)
            : null;
          state.startConversationDraft = null;
        });
        return consumedDraft;
      },

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
          const normalizedRuntime = normalizeAgentRuntimeSelection(runtime);
          state.runtimeByConversationId[conversationId] = normalizedRuntime;
          state.lastRuntimeByProjectId[projectId] = normalizedRuntime;
          state.lastModelEffortByProvider[normalizedRuntime.provider] = {
            modelId: normalizedRuntime.modelId,
            effort: normalizedRuntime.effort,
          };
        }),

      setLastRuntimeForProject: (projectId, runtime) =>
        set((state) => {
          const normalizedRuntime = normalizeAgentRuntimeSelection(runtime);
          state.lastRuntimeByProjectId[projectId] = normalizedRuntime;
          state.lastModelEffortByProvider[normalizedRuntime.provider] = {
            modelId: normalizedRuntime.modelId,
            effort: normalizedRuntime.effort,
          };
        }),

      setBranchBaseCacheForProject: (projectId, options, selectedKey) =>
        set((state) => {
          state.branchBaseCacheByProjectId[projectId] = {
            options,
            selectedKey,
            loadedAt: new Date().toISOString(),
          };
        }),

      setLastBranchBaseSelectionForProject: (projectId, selectedKey) =>
        set((state) => {
          state.lastBranchBaseSelectionByProjectId[projectId] = selectedKey;
          const cached = state.branchBaseCacheByProjectId[projectId];
          if (cached?.options.some((option) => option.key === selectedKey)) {
            cached.selectedKey = selectedKey;
          }
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
        branchBaseCacheByProjectId: state.branchBaseCacheByProjectId,
        lastBranchBaseSelectionByProjectId: state.lastBranchBaseSelectionByProjectId,
        lastModelEffortByProvider: state.lastModelEffortByProvider,
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
