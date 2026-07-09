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
import { workspaceReviewUtilityRuntimeForProvider } from "./agentConversationRuntime";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";

type RuntimeDefaultPolicy = "provider_default" | "workspace_review_utility";

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
  runtimeConversationId: string | null;
  runtimeDefaultPolicy: RuntimeDefaultPolicy;
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
  runtimeConversationId,
  runtimeDefaultPolicy,
  runtimeByConversationId,
  selectedConversationId,
  setRuntimeForConversation,
}: UseAgentsActiveComposerControlsArgs) {
  const [switchingConversationModeId, setSwitchingConversationModeId] = useState<string | null>(null);
  const [updatingTeamConversationId, setUpdatingTeamConversationId] = useState<string | null>(null);
  const defaultRuntime =
    (defaultProjectId ? lastRuntimeByProjectId[defaultProjectId] : null) ??
    (runtimeConversationId ? runtimeByConversationId[runtimeConversationId] : null) ??
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
      if (!runtimeConversationId || !activeProjectId) {
        return;
      }
      const defaultModelId = defaultModelForProvider(provider, modelRegistry);
      const runtime =
        runtimeDefaultPolicy === "workspace_review_utility"
          ? workspaceReviewUtilityRuntimeForProvider(provider)
          : {
              provider,
              modelId: defaultModelId,
              effort: defaultEffortForModel(
                provider,
                defaultModelId,
                modelRegistry,
              ),
            };
      setRuntimeForConversation(
        runtimeConversationId,
        activeProjectId,
        normalizeRuntimeSelection(
          runtime,
          modelRegistry,
          providerSupportedEfforts
        )
      );
    },
    [
      activeProjectId,
      modelRegistry,
      runtimeConversationId,
      runtimeDefaultPolicy,
      setRuntimeForConversation,
    ]
  );

  const handleActiveModelChange = useCallback(
    (
      modelId: string,
      providerSupportedEfforts?: readonly string[] | null,
      providerSupportedModelAliases?: readonly string[] | null,
    ) => {
      if (!runtimeConversationId || !activeProjectId) {
        return;
      }
      setRuntimeForConversation(
        runtimeConversationId,
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
          providerSupportedEfforts,
          providerSupportedModelAliases
        )
      );
    },
    [
      activeProjectId,
      modelRegistry,
      normalizedActiveRuntime.provider,
      runtimeConversationId,
      setRuntimeForConversation,
    ]
  );

  const handleActiveEffortChange = useCallback(
    (
      effort: string,
      providerSupportedEfforts?: readonly string[] | null,
      providerSupportedModelAliases?: readonly string[] | null,
    ) => {
      if (!runtimeConversationId || !activeProjectId) {
        return;
      }
      setRuntimeForConversation(
        runtimeConversationId,
        activeProjectId,
        normalizeRuntimeSelection(
          {
            provider: normalizedActiveRuntime.provider,
            modelId: normalizedActiveRuntime.modelId,
            effort: effort as AgentEffort,
          },
          modelRegistry,
          providerSupportedEfforts,
          providerSupportedModelAliases
        ),
      );
    },
    [
      activeProjectId,
      modelRegistry,
      normalizedActiveRuntime.modelId,
      normalizedActiveRuntime.provider,
      runtimeConversationId,
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

  const handleActiveTeamEnabledChange = useCallback(
    async (enabled: boolean) => {
      if (
        !selectedConversationId ||
        !activeProjectId ||
        !activeConversation ||
        activeConversation.contextType !== "project"
      ) {
        return;
      }

      const coordinationMode = enabled ? "rx_native_team" : "solo";
      if (activeConversation.coordinationMode === coordinationMode) {
        return;
      }

      setUpdatingTeamConversationId(selectedConversationId);
      try {
        await chatApi.updateAgentConversationCoordinationMode({
          conversationId: selectedConversationId,
          coordinationMode,
        });
        await Promise.all([
          invalidateProjectConversations(activeProjectId),
          invalidateConversationDataQueries(queryClient, selectedConversationId),
        ]);
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to change Team setting");
      } finally {
        setUpdatingTeamConversationId(null);
      }
    },
    [
      activeConversation,
      activeProjectId,
      invalidateProjectConversations,
      queryClient,
      selectedConversationId,
    ],
  );

  return {
    activeProjectOptions,
    defaultRuntime,
    handleActiveConversationModeChange,
    handleActiveConversationModeMenuOpen,
    handleActiveTeamEnabledChange,
    handleActiveEffortChange,
    handleActiveModelChange,
    handleActiveProviderChange,
    switchingConversationModeId,
    updatingTeamConversationId,
  };
}
