import { useCallback } from "react";

import type { AgentArtifactTab } from "@/stores/agentSessionStore";

import type { AgentPublishSubTab } from "./agentPublishSubTab";

interface UseAgentArtifactActionsArgs {
  openArtifactTab: (conversationId: string, tab: AgentArtifactTab) => void;
  onPublishSubTabRequest: (
    conversationId: string,
    tab: AgentPublishSubTab,
  ) => void;
  scheduleArtifactPanePreload: () => void;
  selectedConversationId: string | null;
}

export function useAgentArtifactActions({
  openArtifactTab,
  onPublishSubTabRequest,
  scheduleArtifactPanePreload,
  selectedConversationId,
}: UseAgentArtifactActionsArgs) {
  const handleSelectArtifact = useCallback(
    (tab: AgentArtifactTab) => {
      if (!selectedConversationId) {
        return;
      }
      openArtifactTab(selectedConversationId, tab);
    },
    [openArtifactTab, selectedConversationId]
  );

  const handleOpenPublishPane = useCallback((tab: AgentPublishSubTab = "changes") => {
    if (!selectedConversationId) {
      return;
    }
    onPublishSubTabRequest(selectedConversationId, tab);
    openArtifactTab(selectedConversationId, "publish");
  }, [onPublishSubTabRequest, openArtifactTab, selectedConversationId]);

  const handlePreloadArtifacts = useCallback(() => {
    scheduleArtifactPanePreload();
  }, [scheduleArtifactPanePreload]);

  return {
    handleOpenPublishPane,
    handlePreloadArtifacts,
    handleSelectArtifact,
  };
}
