import {
  getSeededArtifactTab,
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

export function resolveAgentArtifactState({
  optimistic,
  persisted,
  hasStored,
  hasAutoOpenArtifacts,
}: {
  optimistic: AgentArtifactState | null;
  persisted: AgentArtifactState;
  hasStored: boolean;
  hasAutoOpenArtifacts: boolean;
}): AgentArtifactState {
  if (optimistic) {
    return { ...optimistic, hiddenTabs: optimistic.hiddenTabs ?? [] };
  }
  if (hasStored) {
    return { ...persisted, hiddenTabs: persisted.hiddenTabs ?? [] };
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

export function seedAgentArtifactTab(
  conversationId: string,
  tab: AgentArtifactTab,
  hasAutoOpenArtifacts: boolean,
): void {
  const current = getAgentArtifactStateSnapshot(conversationId, hasAutoOpenArtifacts);
  const activeTab = getSeededArtifactTab(
    current.activeTab,
    tab,
    current.hiddenTabs,
  );
  useAgentArtifactUiStore.getState().setArtifactState(conversationId, {
    ...current,
    activeTab,
    isOpen: true,
  });
}

export function revealAgentArtifactTab(
  conversationId: string,
  tab: AgentArtifactTab,
  hasAutoOpenArtifacts: boolean,
): void {
  const current = getAgentArtifactStateSnapshot(conversationId, hasAutoOpenArtifacts);
  useAgentArtifactUiStore.getState().setArtifactState(conversationId, {
    ...current,
    activeTab: tab,
    isOpen: true,
    hiddenTabs: current.hiddenTabs.filter((hiddenTab) => hiddenTab !== tab),
  });
  useAgentSessionStore.getState().setArtifactTab(conversationId, tab);
}

export function useResolvedAgentArtifactState(
  conversationId: string | null,
  hasAutoOpenArtifacts: boolean,
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
  });
  return {
    artifactState,
    artifactPaneOpen: artifactState.isOpen,
  };
}
