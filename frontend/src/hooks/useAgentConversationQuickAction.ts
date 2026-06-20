/**
 * useAgentConversationQuickAction - command palette action for new agent drafts
 */

import { useMemo } from "react";
import { Bot } from "lucide-react";

import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";

import type { QuickAction } from "./useQuickActionFlow";

interface UseAgentConversationQuickActionOptions {
  onClose?: () => void;
}

/**
 * Hook to create the command palette action that opens a prefilled agent composer.
 */
export function useAgentConversationQuickAction(
  projectId: string,
  options: UseAgentConversationQuickActionOptions = {},
): QuickAction {
  const setFocusedProject = useAgentSessionStore((state) => state.setFocusedProject);
  const clearSelection = useAgentSessionStore((state) => state.clearSelection);
  const setStartConversationDraft = useAgentSessionStore(
    (state) => state.setStartConversationDraft
  );
  const setCurrentView = useUiStore((state) => state.setCurrentView);
  const { onClose } = options;

  return useMemo<QuickAction>(
    () => {
      const openAgentComposer = (targetProjectId: string) => {
        setFocusedProject(targetProjectId);
        clearSelection();
        setCurrentView("agents");
        onClose?.();
      };

      return {
        id: "agent-conversation",
        label: "Start new agent conversation",
        icon: Bot,
        description: (query: string) => `"${query}"`,
        isVisible: (query: string) => query.trim().length > 0,
        requiresConfirmation: false,

        execute: async (query: string): Promise<string> => {
          setStartConversationDraft({
            projectId,
            content: query.trim(),
            mode: "edit",
          });
          openAgentComposer(projectId);
          return projectId;
        },

        creatingLabel: "Opening agent composer...",
        successLabel: "Agent composer ready",
        viewLabel: "View Composer",
        navigateTo: openAgentComposer,
      };
    },
    [
      clearSelection,
      onClose,
      projectId,
      setCurrentView,
      setFocusedProject,
      setStartConversationDraft,
    ]
  );
}
