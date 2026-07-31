import { useCallback, type Dispatch, type SetStateAction } from "react";

import { useChatStore } from "@/stores/chatStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";

interface UseAgentsSessionBindingsArgs {
  setOptimisticSelectedConversationId: Dispatch<SetStateAction<string | null>>;
}

export function useAgentsSessionBindings({
  setOptimisticSelectedConversationId,
}: UseAgentsSessionBindingsArgs) {
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);

  const focusedProjectId = useAgentSessionStore((s) => s.focusedProjectId);
  const selectedProjectId = useAgentSessionStore((s) => s.selectedProjectId);
  const storedSelectedConversationId = useAgentSessionStore((s) => s.selectedConversationId);
  const runtimeByConversationId = useAgentSessionStore((s) => s.runtimeByConversationId);
  const lastRuntimeByProjectId = useAgentSessionStore((s) => s.lastRuntimeByProjectId);
  const setFocusedProject = useAgentSessionStore((s) => s.setFocusedProject);
  const selectAgentConversation = useAgentSessionStore((s) => s.selectConversation);
  const clearSelection = useAgentSessionStore((s) => s.clearSelection);
  const setRuntimeForConversation = useAgentSessionStore((s) => s.setRuntimeForConversation);
  const setLastRuntimeForProjectMode = useAgentSessionStore(
    (s) => s.setLastRuntimeForProjectMode,
  );
  const setTaskHistoryState = useUiStore((s) => s.setTaskHistoryState);
  const selectProject = useProjectStore((s) => s.selectProject);
  const selectConversation = useCallback(
    (projectId: string | null, conversationId: string) => {
      if (selectedProjectId !== projectId || storedSelectedConversationId !== conversationId) {
        setTaskHistoryState(null);
      }
      if (projectId) {
        selectProject(projectId);
      }
      selectAgentConversation(projectId, conversationId);
    },
    [
      selectAgentConversation,
      selectProject,
      selectedProjectId,
      setTaskHistoryState,
      storedSelectedConversationId,
    ]
  );
  const clearAgentConversationSelection = useCallback(() => {
    setTaskHistoryState(null);
    setOptimisticSelectedConversationId(null);
    clearSelection();
  }, [clearSelection, setOptimisticSelectedConversationId, setTaskHistoryState]);

  return {
    clearAgentConversationSelection,
    focusedProjectId,
    lastRuntimeByProjectId,
    runtimeByConversationId,
    selectConversation,
    selectedProjectId,
    setActiveConversation,
    setFocusedProject,
    setLastRuntimeForProjectMode,
    setRuntimeForConversation,
    storedSelectedConversationId,
  };
}
