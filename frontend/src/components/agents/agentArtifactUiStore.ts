import { create } from "zustand";
import type { AgentArtifactState, AgentTaskArtifactMode } from "@/stores/agentSessionStore";

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
};

interface AgentArtifactUiState {
  artifactByConversationId: Record<string, AgentArtifactState>;
}

interface AgentArtifactUiActions {
  setArtifactState: (conversationId: string, state: AgentArtifactState) => void;
  clearArtifactState: (conversationId: string) => void;
}

export const useAgentArtifactUiStore = create<
  AgentArtifactUiState & AgentArtifactUiActions
>((set) => ({
  artifactByConversationId: {},

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
}));

export function selectOptimisticArtifactState(conversationId: string | null) {
  return (state: AgentArtifactUiState): AgentArtifactState | null =>
    conversationId ? state.artifactByConversationId[conversationId] ?? null : null;
}

