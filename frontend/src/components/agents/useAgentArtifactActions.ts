import { useCallback } from "react";

import type { AgentArtifactTab } from "@/stores/agentSessionStore";

interface UseAgentArtifactActionsArgs {
  openArtifactTab: (conversationId: string, tab: AgentArtifactTab) => void;
  scheduleArtifactPanePreload: () => void;
  selectedConversationId: string | null;
}

export function useAgentArtifactActions({
  openArtifactTab,
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

  const handleOpenPublishPane = useCallback(() => {
    if (!selectedConversationId) {
      return;
    }
    openArtifactTab(selectedConversationId, "publish");
  }, [openArtifactTab, selectedConversationId]);

  const handlePreloadArtifacts = useCallback(() => {
    scheduleArtifactPanePreload();
  }, [scheduleArtifactPanePreload]);

  return {
    handleOpenPublishPane,
    handlePreloadArtifacts,
    handleSelectArtifact,
  };
}
