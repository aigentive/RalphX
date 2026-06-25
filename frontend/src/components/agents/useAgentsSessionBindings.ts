import { useCallback, type Dispatch, type SetStateAction } from "react";

import { useChatStore } from "@/stores/chatStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useProjectStore } from "@/stores/projectStore";

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
  const setLastRuntimeForProject = useAgentSessionStore((s) => s.setLastRuntimeForProject);
  const selectProject = useProjectStore((s) => s.selectProject);
  const selectConversation = useCallback(
    (projectId: string, conversationId: string) => {
      selectProject(projectId);
      selectAgentConversation(projectId, conversationId);
    },
    [selectAgentConversation, selectProject]
  );
  const clearAgentConversationSelection = useCallback(() => {
    setOptimisticSelectedConversationId(null);
    clearSelection();
  }, [clearSelection, setOptimisticSelectedConversationId]);

  return {
    clearAgentConversationSelection,
    focusedProjectId,
    lastRuntimeByProjectId,
    runtimeByConversationId,
    selectConversation,
    selectedProjectId,
    setActiveConversation,
    setFocusedProject,
    setLastRuntimeForProject,
    setRuntimeForConversation,
    storedSelectedConversationId,
  };
}
