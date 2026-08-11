import { create } from "zustand";
import type { AgentArtifactState, AgentTaskArtifactMode } from "@/stores/agentSessionStore";

import type {
  AgentPublishSubTab,
  AgentPublishSubTabRequest,
} from "./agentPublishSubTab";

const TASK_MODE_STORAGE_KEY = "ralphx:agents:taskMode";

function loadPersistedTaskMode(): AgentTaskArtifactMode {
  try {
    const stored = localStorage.getItem(TASK_MODE_STORAGE_KEY);
    if (stored === "kanban" || stored === "graph") return stored;
  } catch { /* SSR / privacy mode */ }
  return "graph";
}

export function persistTaskMode(mode: AgentTaskArtifactMode): void {
  try { localStorage.setItem(TASK_MODE_STORAGE_KEY, mode); } catch { /* noop */ }
}

export const DEFAULT_AGENT_ARTIFACT_UI_STATE: AgentArtifactState = {
  isOpen: false,
  activeTab: "plan",
  taskMode: loadPersistedTaskMode(),
  hiddenTabs: [],
};

interface AgentArtifactUiState {
  artifactByConversationId: Record<string, AgentArtifactState>;
  publishSubTabRequest: AgentPublishSubTabRequest | null;
}

interface AgentArtifactUiActions {
  setArtifactState: (conversationId: string, state: AgentArtifactState) => void;
  clearArtifactState: (conversationId: string) => void;
  requestPublishSubTab: (
    conversationId: string,
    tab: AgentPublishSubTab,
  ) => void;
}

export const useAgentArtifactUiStore = create<
  AgentArtifactUiState & AgentArtifactUiActions
>((set) => ({
  artifactByConversationId: {},
  publishSubTabRequest: null,

  setArtifactState: (conversationId, state) =>
    set((current) => ({
      artifactByConversationId: {
        ...current.artifactByConversationId,
        [conversationId]: { ...state },
      },
    })),

  clearArtifactState: (conversationId) =>
    set((current) => {
      const next = { ...current.artifactByConversationId };
      delete next[conversationId];
      return { artifactByConversationId: next };
    }),

  requestPublishSubTab: (conversationId, tab) =>
    set((current) => ({
      publishSubTabRequest: {
        conversationId,
        requestId: (current.publishSubTabRequest?.requestId ?? 0) + 1,
        tab,
      },
    })),
}));

export function selectOptimisticArtifactState(conversationId: string | null) {
  return (state: AgentArtifactUiState): AgentArtifactState | null =>
    conversationId ? state.artifactByConversationId[conversationId] ?? null : null;
}
