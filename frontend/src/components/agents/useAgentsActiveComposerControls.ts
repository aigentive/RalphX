import { useCallback, useMemo, useState } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
} from "@/api/chat";
import { invalidateConversationDataQueries } from "@/hooks/useChat";
import type {
  AgentEffort,
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import type { Project } from "@/types/project";
import type { AgentModelRegistry } from "@/lib/agent-models";

import type { AgentConversation } from "./agentConversations";
import { resolveConversationAgentMode } from "./agentConversationMode";
import {
  DEFAULT_AGENT_RUNTIME,
  defaultEffortForModel,
  defaultModelForProvider,
  normalizeRuntimeSelection,
} from "./agentOptions";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";

interface UseAgentsActiveComposerControlsArgs {
  activeConversation: AgentConversation | null;
  activeProjectId: string | null;
  activeWorkspace: AgentConversationWorkspace | null;
  defaultProjectId: string | null;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
  lastRuntimeByProjectId: Record<string, AgentRuntimeSelection>;
  modelRegistry: AgentModelRegistry;
  normalizedActiveRuntime: AgentRuntimeSelection;
  projects: Project[];
  queryClient: QueryClient;
  runtimeByConversationId: Record<string, AgentRuntimeSelection>;
  selectedConversationId: string | null;
  setRuntimeForConversation: (
    conversationId: string,
    projectId: string,
    runtime: AgentRuntimeSelection
  ) => void;
}

export function useAgentsActiveComposerControls({
  activeConversation,
  activeProjectId,
  activeWorkspace,
  defaultProjectId,
  invalidateProjectConversations,
  lastRuntimeByProjectId,
  modelRegistry,
  normalizedActiveRuntime,
  projects,
  queryClient,
  runtimeByConversationId,
  selectedConversationId,
  setRuntimeForConversation,
}: UseAgentsActiveComposerControlsArgs) {
  const [switchingConversationModeId, setSwitchingConversationModeId] = useState<string | null>(null);
  const defaultRuntime =
    (defaultProjectId ? lastRuntimeByProjectId[defaultProjectId] : null) ??
    (selectedConversationId ? runtimeByConversationId[selectedConversationId] : null) ??
    DEFAULT_AGENT_RUNTIME;

  const activeProjectOptions = useMemo(
    () =>
      activeProjectId
        ? projects
            .filter((project) => project.id === activeProjectId)
            .map((project) => ({
              id: project.id,
              label: project.name,
              description: project.workingDirectory,
            }))
        : [],
    [activeProjectId, projects]
  );

  const handleActiveProviderChange = useCallback(
    (
      provider: AgentProvider,
      providerSupportedEfforts?: readonly string[] | null,
    ) => {
      if (!selectedConversationId || !activeProjectId) {
        return;
      }
      const modelId = defaultModelForProvider(provider, modelRegistry);
      setRuntimeForConversation(
        selectedConversationId,
        activeProjectId,
        normalizeRuntimeSelection(
          {
            provider,
            modelId,
            effort: defaultEffortForModel(provider, modelId, modelRegistry),
          },
          modelRegistry,
          providerSupportedEfforts
        )
      );
    },
    [
      activeProjectId,
      modelRegistry,
      selectedConversationId,
      setRuntimeForConversation,
    ]
  );

  const handleActiveModelChange = useCallback(
    (modelId: string, providerSupportedEfforts?: readonly string[] | null) => {
      if (!selectedConversationId || !activeProjectId) {
        return;
      }
      setRuntimeForConversation(
        selectedConversationId,
        activeProjectId,
        normalizeRuntimeSelection(
          {
            provider: normalizedActiveRuntime.provider,
            modelId,
            effort: defaultEffortForModel(
              normalizedActiveRuntime.provider,
              modelId,
              modelRegistry
            ),
          },
          modelRegistry,
          providerSupportedEfforts
        )
      );
    },
    [
      activeProjectId,
      modelRegistry,
      normalizedActiveRuntime.provider,
      selectedConversationId,
      setRuntimeForConversation,
    ]
  );

  const handleActiveEffortChange = useCallback(
    (effort: string, providerSupportedEfforts?: readonly string[] | null) => {
      if (!selectedConversationId || !activeProjectId) {
        return;
      }
      setRuntimeForConversation(
        selectedConversationId,
        activeProjectId,
        normalizeRuntimeSelection(
          {
            provider: normalizedActiveRuntime.provider,
            modelId: normalizedActiveRuntime.modelId,
            effort: effort as AgentEffort,
          },
          modelRegistry,
          providerSupportedEfforts
        ),
      );
    },
    [
      activeProjectId,
      modelRegistry,
      normalizedActiveRuntime.modelId,
      normalizedActiveRuntime.provider,
      selectedConversationId,
      setRuntimeForConversation,
    ]
  );

  const handleActiveConversationModeChange = useCallback(
    async (mode: AgentConversationWorkspaceMode) => {
      if (
        !selectedConversationId ||
        !activeProjectId ||
        !activeConversation ||
        activeConversation.contextType !== "project"
      ) {
        return;
      }

      const currentMode = resolveConversationAgentMode(activeConversation, activeWorkspace);
      if (currentMode === mode) {
        return;
      }

      setSwitchingConversationModeId(selectedConversationId);
      try {
        await chatApi.switchAgentConversationMode({
          conversationId: selectedConversationId,
          mode,
        });
        await Promise.all([
          queryClient.invalidateQueries({
            queryKey: ["agents", "conversation-workspace", selectedConversationId],
          }),
          invalidateProjectConversations(activeProjectId),
          invalidateConversationDataQueries(queryClient, selectedConversationId),
        ]);
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to change agent mode");
      } finally {
        setSwitchingConversationModeId(null);
      }
    },
    [
      activeConversation,
      activeProjectId,
      activeWorkspace,
      invalidateProjectConversations,
      queryClient,
      selectedConversationId,
    ]
  );

  const handleActiveConversationModeMenuOpen = useCallback(() => {
    if (!selectedConversationId) {
      return;
    }
    void queryClient.refetchQueries({
      queryKey: agentWorkspaceKeys.workspace(selectedConversationId),
      exact: true,
    });
  }, [queryClient, selectedConversationId]);

  return {
    activeProjectOptions,
    defaultRuntime,
    handleActiveConversationModeChange,
    handleActiveConversationModeMenuOpen,
    handleActiveEffortChange,
    handleActiveModelChange,
    handleActiveProviderChange,
    switchingConversationModeId,
  };
}
