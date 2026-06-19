import {
  selectArtifactState,
  selectHasStoredArtifactState,
  useAgentSessionStore,
  type AgentArtifactTab,
  type AgentArtifactState,
} from "@/stores/agentSessionStore";

import {
  DEFAULT_AGENT_ARTIFACT_UI_STATE,
  selectOptimisticArtifactState,
  useAgentArtifactUiStore,
} from "./agentArtifactUiStore";
import type { IdeationArtifactTab } from "./agentArtifactTabs";

function preferredIdeationArtifactTab(
  availableTabs: readonly IdeationArtifactTab[],
): IdeationArtifactTab | null {
  if (availableTabs.includes("tasks")) {
    return "tasks";
  }
  if (availableTabs.includes("plan")) {
    return "plan";
  }
  return availableTabs[0] ?? null;
}

function sanitizeStoredArtifactState(
  state: AgentArtifactState,
  availableTabs: readonly IdeationArtifactTab[] | undefined,
): AgentArtifactState {
  const preferredTab = availableTabs ? preferredIdeationArtifactTab(availableTabs) : null;
  if (!preferredTab) {
    return state;
  }
  const staleExternalTabs: readonly AgentArtifactTab[] = ["jira", "linear", "publish"];
  if (!staleExternalTabs.includes(state.activeTab)) {
    return state;
  }
  return {
    ...state,
    activeTab: preferredTab,
  };
}

export function resolveAgentArtifactState({
  optimistic,
  persisted,
  hasStored,
  hasAutoOpenArtifacts,
  availableTabs,
}: {
  optimistic: AgentArtifactState | null;
  persisted: AgentArtifactState;
  hasStored: boolean;
  hasAutoOpenArtifacts: boolean;
  availableTabs?: readonly IdeationArtifactTab[] | undefined;
}): AgentArtifactState {
  if (optimistic) {
    return optimistic;
  }
  if (hasStored) {
    return sanitizeStoredArtifactState(persisted, availableTabs);
  }
  return {
    ...DEFAULT_AGENT_ARTIFACT_UI_STATE,
    isOpen: hasAutoOpenArtifacts,
  };
}

export function getAgentArtifactStateSnapshot(
  conversationId: string,
  hasAutoOpenArtifacts: boolean,
): AgentArtifactState {
  const optimistic =
    useAgentArtifactUiStore.getState().artifactByConversationId[conversationId] ?? null;
  const persisted =
    useAgentSessionStore.getState().artifactByConversationId[conversationId] ?? null;
  return resolveAgentArtifactState({
    optimistic,
    persisted: persisted ?? DEFAULT_AGENT_ARTIFACT_UI_STATE,
    hasStored: Boolean(persisted),
    hasAutoOpenArtifacts,
  });
}

export function useResolvedAgentArtifactState(
  conversationId: string | null,
  hasAutoOpenArtifacts: boolean,
  availableTabs?: readonly IdeationArtifactTab[] | undefined,
) {
  const optimisticArtifactState = useAgentArtifactUiStore(
    selectOptimisticArtifactState(conversationId),
  );
  const persistedArtifactState = useAgentSessionStore(selectArtifactState(conversationId));
  const hasStoredArtifactState = useAgentSessionStore(
    selectHasStoredArtifactState(conversationId),
  );
  const artifactState = resolveAgentArtifactState({
    optimistic: optimisticArtifactState,
    persisted: persistedArtifactState,
    hasStored: hasStoredArtifactState,
    hasAutoOpenArtifacts,
    availableTabs,
  });
  return {
    artifactState,
    artifactPaneOpen: artifactState.isOpen,
  };
}
