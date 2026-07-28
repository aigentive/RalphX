import { create } from "zustand";
import { persist } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";

import { createEnvScopedStorage } from "@/lib/remote/env-scoped-storage";
import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import {
  normalizeAgentRuntimeForPersistence,
  type AgentEffort,
  type AgentProvider,
  type AgentRuntimeSelection,
} from "@/lib/agent-models";
import type {
  AgentConversationBaseSelection,
  AgentConversationWorkspaceMode,
  CapabilityIntent,
  ComposerArtifactReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
  TeamIntent,
} from "@/api/chat";
import type {
  AutomationAuthoringMode,
  AutomationJudgeState,
  AutomationRunStatus,
} from "@/api/automations";
import type { BranchBaseOption } from "@/components/shared/branchBaseOptions";
import type {
  ManualRoleRuntimeSelection,
  ManualServiceTier,
} from "@/api/manual-role-defaults.types";

export type { AgentEffort, AgentProvider, AgentRuntimeSelection } from "@/lib/agent-models";

export type AgentArtifactTab =
  | "review"
  | "automation"
  | "persona"
  | "issues"
  | "plan"
  | "verification"
  | "tasks"
  | "pr"
  | "jira"
  | "linear"
  | "clickup"
  | "granola"
  | "publish";
export const AGENT_ARTIFACT_TABS: readonly AgentArtifactTab[] = [
  "issues",
  "plan",
  "verification",
  "tasks",
  "automation",
  "persona",
  "pr",
  "jira",
  "linear",
  "clickup",
  "granola",
  "review",
  "publish",
];
export type AgentTaskArtifactMode = "graph" | "kanban";
export type AgentProjectSort = "latest" | "az" | "za";
export type AgentSidebarGroupBy = "project" | "publication" | "automation";
export const AGENT_DEFAULT_START_MODES = [
  "plan",
  "edit",
  "review_pr",
  "chat",
  "automation",
] as const satisfies readonly AgentConversationWorkspaceMode[];
export type AgentDefaultStartMode = (typeof AGENT_DEFAULT_START_MODES)[number];
export const DEFAULT_AGENT_START_MODE: AgentDefaultStartMode = "edit";
export type AgentSidebarPublicationState =
  | "active"
  | "draft"
  | "merged"
  | "closed"
  | "uncommitted"
  | "unpushed";
export type LaunchRuntimeRoleKey =
  | "workspace_reviewer"
  | "workspace_repair"
  | "workspace_edit"
  | "ideation_primary";

export interface AgentArtifactState {
  isOpen: boolean;
  activeTab: AgentArtifactTab;
  taskMode: AgentTaskArtifactMode;
  hiddenTabs: AgentArtifactTab[];
}

export interface AgentTaskArtifactFocusRequest {
  taskId: string;
  requestId: number;
}

export interface AgentAutomationRunFocusRequest {
  projectId: string;
  automationId: string;
  runId: string;
  conversationId: string;
  runStatus: AutomationRunStatus | null;
  judgeState: AutomationJudgeState | null;
  workspaceMode: AgentConversationWorkspaceMode | null;
  hasPlanArtifact: boolean;
  hasPullRequest: boolean;
  seededTab: AgentArtifactTab;
  requestId: number;
}

export interface VisibleAgentScope {
  workspaceConversationId: string;
  visibleConversationId?: string;
  automationRunId?: string;
  automationConversationId?: string;
}

export type AgentAutomationRunFocusRequestInput = Omit<
  AgentAutomationRunFocusRequest,
  "requestId"
>;

export interface AgentBranchBaseCacheEntry {
  options: BranchBaseOption[];
  selectedKey: string;
  loadedAt: string;
}

export interface AgentStartConversationDraft {
  projectId: string | null;
  content?: string;
  mode: AgentConversationWorkspaceMode;
  projectLocked?: boolean;
  sourcePersonaId?: string;
  sourcePersonaName?: string;
  automationAuthoringMode?: AutomationAuthoringMode;
  composerArtifactReferences?: ComposerArtifactReference[];
  composerProjectReferences?: ComposerProjectReference[];
  composerIntegrationReferences?: ComposerIntegrationReference[];
}

export interface AgentStartConversationRetryInput {
  projectId: string | null;
  content: string;
  runtime: AgentRuntimeSelection;
  runtimeProviderContext?: AgentRuntimeProviderContext;
  useRoleDefault?: boolean;
  mode: AgentConversationWorkspaceMode;
  automationAuthoringMode?: AutomationAuthoringMode;
  base: AgentConversationBaseSelection | null;
  codexFastMode?: boolean | null;
  personaId?: string | null;
  capabilityIntent?: CapabilityIntent | null;
  teamIntent?: TeamIntent | null;
  composerArtifactReferences?: ComposerArtifactReference[];
  composerProjectReferences?: ComposerProjectReference[];
  composerIntegrationReferences?: ComposerIntegrationReference[];
}

export interface AgentRuntimeProviderContext {
  supportedEfforts?: string[] | null;
  supportedModelAliases?: string[] | null;
}

export type AgentStartConversationFailure =
  | {
      kind: "linked_setup";
      message: string;
      retryInput: AgentStartConversationRetryInput;
    }
  | {
      kind: "mcp_setup";
      provider: "claude" | "codex";
      serverId: string;
      scope: string | null;
      conflictKind:
        | "ambiguous_reserved_id"
        | "legacy_registration"
        | "legacy_repair_failed";
      repairStatus: "repairable" | "repaired" | "failed" | "manual_only";
      retryInput: AgentStartConversationRetryInput;
    };

interface AgentSessionState {
  focusedProjectId: string | null;
  selectedProjectId: string | null;
  selectedConversationId: string | null;
  /** Exact Agent surface currently rendered; intentionally excluded from persistence. */
  visibleAgentScope: VisibleAgentScope | null;
  startConversationDraft: AgentStartConversationDraft | null;
  startConversationFailure: AgentStartConversationFailure | null;
  defaultStartMode: AgentDefaultStartMode;
  lastSelectedConversationByProjectId: Record<string, string>;
  expandedProjectIds: Record<string, boolean>;
  showAllProjects: boolean;
  showEmptyProjectGroups: boolean;
  projectSort: AgentProjectSort;
  sidebarGroupBy: AgentSidebarGroupBy;
  sidebarProjectFilterIds: string[];
  sidebarPublicationStateFilters: AgentSidebarPublicationState[];
  pinnedConversationIds: Record<string, true>;
  artifactByConversationId: Record<string, AgentArtifactState>;
  taskArtifactFocusRequestByConversationId: Record<
    string,
    AgentTaskArtifactFocusRequest
  >;
  automationRunFocusRequestByConversationId: Record<
    string,
    AgentAutomationRunFocusRequest
  >;
  runtimeByConversationId: Record<string, AgentRuntimeSelection>;
  serviceTierByConversationId: Record<string, ManualServiceTier>;
  roleRuntimeOverridesByConversationId: Record<
    string,
    Partial<Record<LaunchRuntimeRoleKey, ManualRoleRuntimeSelection>>
  >;
  lastRuntimeByProjectId: Record<string, AgentRuntimeSelection>;
  branchBaseCacheByProjectId: Record<string, AgentBranchBaseCacheEntry>;
  lastBranchBaseSelectionByProjectId: Record<string, string>;
  lastModelEffortByProvider: Record<AgentProvider, { modelId: string; effort: AgentEffort }>;
}

interface AgentSessionActions {
  setFocusedProject: (projectId: string | null) => void;
  selectConversation: (projectId: string | null, conversationId: string) => void;
  clearSelection: () => void;
  setVisibleAgentScope: (scope: VisibleAgentScope | null) => void;
  setStartConversationDraft: (draft: AgentStartConversationDraft) => void;
  consumeStartConversationDraft: () => AgentStartConversationDraft | null;
  setStartConversationFailure: (failure: AgentStartConversationFailure | null) => void;
  setDefaultStartMode: (mode: AgentDefaultStartMode) => void;
  setProjectExpanded: (projectId: string, expanded: boolean) => void;
  toggleProjectExpanded: (projectId: string) => void;
  setShowAllProjects: (showAllProjects: boolean) => void;
  setShowEmptyProjectGroups: (showEmptyProjectGroups: boolean) => void;
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
  hideArtifactTab: (
    conversationId: string,
    tab: AgentArtifactTab,
    availableTabs: readonly AgentArtifactTab[],
  ) => void;
  showArtifactTab: (conversationId: string, tab: AgentArtifactTab) => void;
  revealArtifactTab: (conversationId: string, tab: AgentArtifactTab) => void;
  setTaskArtifactMode: (conversationId: string, mode: AgentTaskArtifactMode) => void;
  focusTaskArtifact: (conversationId: string, taskId: string) => void;
  requestAutomationRunFocus: (
    setupConversationId: string,
    request: AgentAutomationRunFocusRequestInput
  ) => void;
  clearAutomationRunFocusRequest: (
    setupConversationId: string,
    requestId?: number
  ) => void;
  setRuntimeForConversation: (
    conversationId: string,
    projectId: string | null,
    runtime: AgentRuntimeSelection
  ) => void;
  setRoleDefaultRuntimeForConversation: (
    conversationId: string,
    projectId: string | null,
    runtime: AgentRuntimeSelection
  ) => void;
  setServiceTierForConversation: (
    conversationId: string,
    serviceTier: ManualServiceTier,
  ) => void;
  clearServiceTierForConversation: (conversationId: string) => void;
  setRoleRuntimeOverride: (
    conversationId: string,
    role: LaunchRuntimeRoleKey,
    value: ManualRoleRuntimeSelection,
  ) => void;
  clearRoleRuntimeOverride: (
    conversationId: string,
    role: LaunchRuntimeRoleKey,
  ) => void;
  setLastRuntimeForProject: (projectId: string, runtime: AgentRuntimeSelection) => void;
  clearLastRuntimeForProject: (projectId: string) => void;
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
  hiddenTabs: [],
};
const DEFAULT_SHOW_ALL_PROJECTS = true;
const DEFAULT_SHOW_EMPTY_PROJECT_GROUPS = true;
export const DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS: AgentSidebarPublicationState[] = [
  "active",
  "draft",
  "merged",
  "closed",
  "uncommitted",
  "unpushed",
];
const AGENT_SESSION_STORE_VERSION = 10;

type LegacyAgentArtifactTab = AgentArtifactTab | "proposal";

export function normalizeAgentArtifactTab(
  tab: LegacyAgentArtifactTab,
): AgentArtifactTab {
  return tab === "proposal" ? "plan" : tab;
}

function normalizeAgentArtifactState(
  artifactState: Omit<AgentArtifactState, "activeTab" | "hiddenTabs"> & {
    activeTab: LegacyAgentArtifactTab;
    hiddenTabs?: unknown;
  },
): AgentArtifactState {
  return {
    ...artifactState,
    activeTab: normalizeAgentArtifactTab(artifactState.activeTab),
    hiddenTabs: normalizeHiddenArtifactTabs(artifactState.hiddenTabs),
  };
}

function isAgentArtifactTab(value: unknown): value is AgentArtifactTab {
  return typeof value === "string" && AGENT_ARTIFACT_TABS.includes(value as AgentArtifactTab);
}

function normalizeHiddenArtifactTabs(value: unknown): AgentArtifactTab[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return Array.from(new Set(value.filter(isAgentArtifactTab)));
}

export function getShownArtifactTabs(
  availableTabs: readonly AgentArtifactTab[],
  hiddenTabs: readonly AgentArtifactTab[],
): AgentArtifactTab[] {
  const hidden = new Set(hiddenTabs);
  return availableTabs.filter((tab) => !hidden.has(tab));
}

export function getArtifactTabFallback(
  availableTabs: readonly AgentArtifactTab[],
  hiddenTabs: readonly AgentArtifactTab[],
  hiddenTab: AgentArtifactTab,
): AgentArtifactTab | undefined {
  const shownTabs = getShownArtifactTabs(availableTabs, hiddenTabs);
  const hiddenIndex = availableTabs.indexOf(hiddenTab);
  return (
    availableTabs
      .slice(0, Math.max(hiddenIndex, 0))
      .reverse()
      .find((candidate) => shownTabs.includes(candidate)) ?? shownTabs[0]
  );
}

export function getSeededArtifactTab(
  activeTab: AgentArtifactTab,
  requestedTab: AgentArtifactTab,
  hiddenTabs: readonly AgentArtifactTab[],
): AgentArtifactTab {
  if (!hiddenTabs.includes(requestedTab)) {
    return requestedTab;
  }
  return activeTab;
}

function normalizeRuntimeRecord(value: unknown): Record<string, AgentRuntimeSelection> {
  if (!value || typeof value !== "object") {
    return {};
  }

  return Object.fromEntries(
    Object.entries(value).map(([key, runtime]) => [
      key,
      normalizeAgentRuntimeForPersistence(runtime),
    ])
  );
}

function normalizeRoleRuntimeOverrides(value: unknown): AgentSessionState["roleRuntimeOverridesByConversationId"] {
  if (!value || typeof value !== "object") return {};
  const roles: LaunchRuntimeRoleKey[] = [
    "workspace_reviewer",
    "workspace_repair",
    "workspace_edit",
    "ideation_primary",
  ];
  const result: AgentSessionState["roleRuntimeOverridesByConversationId"] = {};
  for (const [conversationId, rawRoles] of Object.entries(value)) {
    if (!rawRoles || typeof rawRoles !== "object") continue;
    const normalized: Partial<Record<LaunchRuntimeRoleKey, ManualRoleRuntimeSelection>> = {};
    for (const role of roles) {
      const raw = (rawRoles as Record<string, unknown>)[role];
      if (!raw || typeof raw !== "object") continue;
      const candidate = raw as Record<string, unknown>;
      if (
        typeof candidate.provider !== "string" ||
        candidate.provider.trim() === "" ||
        !(candidate.model === null || typeof candidate.model === "string") ||
        !(candidate.effort === null || typeof candidate.effort === "string") ||
        !(
          candidate.coordinationMode === null ||
          typeof candidate.coordinationMode === "string"
        ) ||
        !(
          candidate.personaId === null ||
          typeof candidate.personaId === "string"
        ) ||
        !["provider_default", "standard", "fast"].includes(
          String(candidate.serviceTier),
        )
      ) continue;
      normalized[role] = candidate as unknown as ManualRoleRuntimeSelection;
    }
    if (Object.keys(normalized).length > 0) result[conversationId] = normalized;
  }
  return result;
}

function normalizeServiceTierRecord(
  value: unknown,
): Record<string, ManualServiceTier> {
  if (!value || typeof value !== "object") return {};
  return Object.fromEntries(
    Object.entries(value).filter((entry): entry is [string, ManualServiceTier] =>
      ["provider_default", "standard", "fast"].includes(String(entry[1])),
    ),
  );
}

export function isAgentDefaultStartMode(
  value: unknown,
): value is AgentDefaultStartMode {
  return AGENT_DEFAULT_START_MODES.includes(value as AgentDefaultStartMode);
}

function migrateArtifactStateRecord(value: unknown): unknown {
  if (!value || typeof value !== "object") {
    return value;
  }

  return Object.fromEntries(
    Object.entries(value).map(([conversationId, artifactState]) => {
      if (!artifactState || typeof artifactState !== "object") {
        return [conversationId, artifactState];
      }
      const state = artifactState as Record<string, unknown>;
      return [
        conversationId,
        {
          ...state,
          activeTab: state.activeTab === "proposal" ? "plan" : state.activeTab,
          hiddenTabs: normalizeHiddenArtifactTabs(state.hiddenTabs),
        },
      ];
    }),
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

  if (version < 8) {
    nextState.artifactByConversationId = migrateArtifactStateRecord(
      nextState.artifactByConversationId,
    );
  }

  if (version < 9) {
    nextState.defaultStartMode = isAgentDefaultStartMode(
      nextState.defaultStartMode,
    )
      ? nextState.defaultStartMode
      : DEFAULT_AGENT_START_MODE;
  }
  if (version < 10) {
    nextState.serviceTierByConversationId = {};
    nextState.roleRuntimeOverridesByConversationId = normalizeRoleRuntimeOverrides(
      nextState.roleRuntimeOverridesByConversationId,
    );
  }

  if (version < 10) {
    nextState.showEmptyProjectGroups = DEFAULT_SHOW_EMPTY_PROJECT_GROUPS;
  }

  return nextState;
}

export function mergeAgentSessionStore(
  persistedState: unknown,
  currentState: AgentSessionState & AgentSessionActions,
): AgentSessionState & AgentSessionActions {
  if (!persistedState || typeof persistedState !== "object") {
    return currentState;
  }
  const persisted = persistedState as Partial<AgentSessionState>;
  return {
    ...currentState,
    ...persisted,
    serviceTierByConversationId: normalizeServiceTierRecord(
      persisted.serviceTierByConversationId,
    ),
    roleRuntimeOverridesByConversationId: normalizeRoleRuntimeOverrides(
      persisted.roleRuntimeOverridesByConversationId,
    ),
  };
}

function ensureArtifactState(state: AgentSessionState, conversationId: string): AgentArtifactState {
  const current =
    state.artifactByConversationId[conversationId] ?? DEFAULT_ARTIFACT_STATE;
  const normalized = normalizeAgentArtifactState(current);
  state.artifactByConversationId[conversationId] = normalized;
  return normalized;
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
    content: draft.content ?? "",
    mode: draft.mode,
    ...(draft.projectLocked !== undefined
      ? { projectLocked: draft.projectLocked }
      : {}),
    ...(draft.sourcePersonaId ? { sourcePersonaId: draft.sourcePersonaId } : {}),
    ...(draft.sourcePersonaName ? { sourcePersonaName: draft.sourcePersonaName } : {}),
    ...(draft.automationAuthoringMode
      ? { automationAuthoringMode: draft.automationAuthoringMode }
      : {}),
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
      visibleAgentScope: null,
      startConversationDraft: null,
      startConversationFailure: null,
      defaultStartMode: DEFAULT_AGENT_START_MODE,
      lastSelectedConversationByProjectId: {},
      expandedProjectIds: {},
      showAllProjects: DEFAULT_SHOW_ALL_PROJECTS,
      showEmptyProjectGroups: DEFAULT_SHOW_EMPTY_PROJECT_GROUPS,
      projectSort: "latest",
      sidebarGroupBy: "project",
      sidebarProjectFilterIds: [],
      sidebarPublicationStateFilters: [...DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS],
      pinnedConversationIds: {},
      artifactByConversationId: {},
      taskArtifactFocusRequestByConversationId: {},
      automationRunFocusRequestByConversationId: {},
      runtimeByConversationId: {},
      serviceTierByConversationId: {},
      roleRuntimeOverridesByConversationId: {},
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
          if (projectId) {
            state.lastSelectedConversationByProjectId ??= {};
            state.lastSelectedConversationByProjectId[projectId] = conversationId;
            expandOnlyProject(state, projectId);
          }
        }),

      clearSelection: () =>
        set((state) => {
          state.selectedProjectId = null;
          state.selectedConversationId = null;
        }),

      setVisibleAgentScope: (scope) =>
        set((state) => {
          state.visibleAgentScope = scope;
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

      setStartConversationFailure: (failure) =>
        set((state) => {
          state.startConversationFailure = failure;
        }),

      setDefaultStartMode: (mode) =>
        set((state) => {
          state.defaultStartMode = mode;
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

      setShowEmptyProjectGroups: (showEmptyProjectGroups) =>
        set((state) => {
          state.showEmptyProjectGroups = showEmptyProjectGroups;
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
          artifactState.activeTab = normalizeAgentArtifactTab(tab);
          artifactState.isOpen = true;
          artifactState.hiddenTabs = artifactState.hiddenTabs.filter(
            (hiddenTab) => hiddenTab !== tab,
          );
        }),

      setArtifactState: (conversationId, artifactState) =>
        set((state) => {
          state.artifactByConversationId[conversationId] =
            normalizeAgentArtifactState(artifactState);
        }),

      hideArtifactTab: (conversationId, tab, availableTabs) =>
        set((state) => {
          const artifactState = ensureArtifactState(state, conversationId);
          if (!artifactState.hiddenTabs.includes(tab)) {
            artifactState.hiddenTabs.push(tab);
          }
          if (artifactState.activeTab !== tab) {
            return;
          }
          const nextActiveTab = getArtifactTabFallback(
            availableTabs,
            artifactState.hiddenTabs,
            tab,
          );
          if (nextActiveTab) {
            artifactState.activeTab = nextActiveTab;
          }
        }),

      showArtifactTab: (conversationId, tab) =>
        set((state) => {
          const artifactState = ensureArtifactState(state, conversationId);
          artifactState.hiddenTabs = artifactState.hiddenTabs.filter(
            (hiddenTab) => hiddenTab !== tab,
          );
        }),

      revealArtifactTab: (conversationId, tab) =>
        set((state) => {
          const artifactState = ensureArtifactState(state, conversationId);
          artifactState.hiddenTabs = artifactState.hiddenTabs.filter(
            (hiddenTab) => hiddenTab !== tab,
          );
          artifactState.activeTab = tab;
        }),

      setTaskArtifactMode: (conversationId, mode) =>
        set((state) => {
          ensureArtifactState(state, conversationId).taskMode = mode;
        }),

      focusTaskArtifact: (conversationId, taskId) =>
        set((state) => {
          const artifactState = ensureArtifactState(state, conversationId);
          artifactState.activeTab = "tasks";
          artifactState.isOpen = true;
          artifactState.hiddenTabs = artifactState.hiddenTabs.filter(
            (hiddenTab) => hiddenTab !== "tasks",
          );
          const current =
            state.taskArtifactFocusRequestByConversationId[conversationId];
          state.taskArtifactFocusRequestByConversationId[conversationId] = {
            taskId,
            requestId: (current?.requestId ?? 0) + 1,
          };
        }),

      requestAutomationRunFocus: (setupConversationId, request) =>
        set((state) => {
          const current =
            state.automationRunFocusRequestByConversationId[setupConversationId];
          state.automationRunFocusRequestByConversationId[setupConversationId] = {
            ...request,
            requestId: (current?.requestId ?? 0) + 1,
          };
        }),

      clearAutomationRunFocusRequest: (setupConversationId, requestId) =>
        set((state) => {
          const current =
            state.automationRunFocusRequestByConversationId[setupConversationId];
          if (!current) {
            return;
          }
          if (requestId !== undefined && current.requestId !== requestId) {
            return;
          }
          delete state.automationRunFocusRequestByConversationId[
            setupConversationId
          ];
        }),

      setRuntimeForConversation: (conversationId, projectId, runtime) =>
        set((state) => {
          const normalizedRuntime = normalizeAgentRuntimeForPersistence(runtime);
          state.runtimeByConversationId[conversationId] = normalizedRuntime;
          if (projectId) {
            state.lastRuntimeByProjectId[projectId] = normalizedRuntime;
          }
          state.lastModelEffortByProvider[normalizedRuntime.provider] = {
            modelId: normalizedRuntime.modelId,
            effort: normalizedRuntime.effort,
          };
        }),

      setRoleDefaultRuntimeForConversation: (conversationId, projectId, runtime) =>
        set((state) => {
          const normalizedRuntime = normalizeAgentRuntimeForPersistence(runtime);
          state.runtimeByConversationId[conversationId] = normalizedRuntime;
          if (projectId) {
            delete state.lastRuntimeByProjectId[projectId];
          }
          state.lastModelEffortByProvider[normalizedRuntime.provider] = {
            modelId: normalizedRuntime.modelId,
            effort: normalizedRuntime.effort,
          };
        }),

      setServiceTierForConversation: (conversationId, serviceTier) =>
        set((state) => {
          state.serviceTierByConversationId[conversationId] = serviceTier;
        }),

      clearServiceTierForConversation: (conversationId) =>
        set((state) => {
          delete state.serviceTierByConversationId[conversationId];
        }),

      setRoleRuntimeOverride: (conversationId, role, value) =>
        set((state) => {
          state.roleRuntimeOverridesByConversationId[conversationId] ??= {};
          state.roleRuntimeOverridesByConversationId[conversationId]![role] = {
            ...value,
          };
        }),

      clearRoleRuntimeOverride: (conversationId, role) =>
        set((state) => {
          const overrides = state.roleRuntimeOverridesByConversationId[conversationId];
          if (!overrides) return;
          delete overrides[role];
          if (Object.keys(overrides).length === 0) {
            delete state.roleRuntimeOverridesByConversationId[conversationId];
          }
        }),

      setLastRuntimeForProject: (projectId, runtime) =>
        set((state) => {
          const normalizedRuntime = normalizeAgentRuntimeForPersistence(runtime);
          state.lastRuntimeByProjectId[projectId] = normalizedRuntime;
          state.lastModelEffortByProvider[normalizedRuntime.provider] = {
            modelId: normalizedRuntime.modelId,
            effort: normalizedRuntime.effort,
          };
        }),

      clearLastRuntimeForProject: (projectId) =>
        set((state) => {
          delete state.lastRuntimeByProjectId[projectId];
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
      storage: createEnvScopedStorage("ralphx-agent-session-store"),
      version: AGENT_SESSION_STORE_VERSION,
      migrate: migrateAgentSessionStore,
      merge: mergeAgentSessionStore,
      partialize: (state) => ({
        focusedProjectId: state.focusedProjectId,
        selectedProjectId: state.selectedProjectId,
        selectedConversationId: state.selectedConversationId,
        defaultStartMode: state.defaultStartMode,
        lastSelectedConversationByProjectId: state.lastSelectedConversationByProjectId,
        expandedProjectIds: state.expandedProjectIds,
        showAllProjects: state.showAllProjects,
        showEmptyProjectGroups: state.showEmptyProjectGroups,
        projectSort: state.projectSort,
        sidebarGroupBy: state.sidebarGroupBy,
        sidebarProjectFilterIds: state.sidebarProjectFilterIds,
        sidebarPublicationStateFilters: state.sidebarPublicationStateFilters,
        pinnedConversationIds: state.pinnedConversationIds,
        artifactByConversationId: state.artifactByConversationId,
        runtimeByConversationId: state.runtimeByConversationId,
        serviceTierByConversationId: state.serviceTierByConversationId,
        roleRuntimeOverridesByConversationId:
          state.roleRuntimeOverridesByConversationId,
        lastRuntimeByProjectId: state.lastRuntimeByProjectId,
        branchBaseCacheByProjectId: state.branchBaseCacheByProjectId,
        lastBranchBaseSelectionByProjectId: state.lastBranchBaseSelectionByProjectId,
        lastModelEffortByProvider: state.lastModelEffortByProvider,
      }),
    }
  )
);

registerEnvIsolatedStore({
  name: "useAgentSessionStore",
  reset: () => useAgentSessionStore.setState(useAgentSessionStore.getInitialState(), true),
  rehydrate: () => {
    void useAgentSessionStore.persist.rehydrate();
  },
});

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
