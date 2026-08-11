import { useCallback, useMemo, useState } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
  CapabilityIntent,
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
  runtimeConversationId: string | null;
  runtimeByConversationId: Record<string, AgentRuntimeSelection>;
  selectedConversationId: string | null;
  setComposerRuntimeForConversation: (
    conversationId: string,
    projectId: string | null,
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
  runtimeByConversationId,
  selectedConversationId,
  setComposerRuntimeForConversation,
}: UseAgentsActiveComposerControlsArgs) {
  const [switchingConversationModeId, setSwitchingConversationModeId] = useState<string | null>(null);
  const [updatingCapabilityConversationId, setUpdatingCapabilityConversationId] =
    useState<string | null>(null);
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
      providerSupportedModelAliases?: readonly string[] | null,
    ) => {
      if (!runtimeConversationId) {
        return null;
      }
      const defaultModelId = defaultModelForProvider(
        provider,
        modelRegistry,
        providerSupportedModelAliases,
      );
      const runtime = {
        provider,
        modelId: defaultModelId,
        effort: defaultEffortForModel(
          provider,
          defaultModelId,
          modelRegistry,
        ),
      };
      const normalizedRuntime = normalizeRuntimeSelection(
        runtime,
        modelRegistry,
        providerSupportedEfforts,
        providerSupportedModelAliases,
      );
      setComposerRuntimeForConversation(
        runtimeConversationId,
        activeProjectId,
        normalizedRuntime,
      );
      return normalizedRuntime;
    },
    [
      activeProjectId,
      modelRegistry,
      runtimeConversationId,
      setComposerRuntimeForConversation,
    ]
  );

  const handleActiveModelChange = useCallback(
    (
      modelId: string,
      providerSupportedEfforts?: readonly string[] | null,
      providerSupportedModelAliases?: readonly string[] | null,
    ) => {
      if (!runtimeConversationId) {
        return null;
      }
      const normalizedRuntime = normalizeRuntimeSelection(
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
      );
      setComposerRuntimeForConversation(
        runtimeConversationId,
        activeProjectId,
        normalizedRuntime,
      );
      return normalizedRuntime;
    },
    [
      activeProjectId,
      modelRegistry,
      normalizedActiveRuntime.provider,
      runtimeConversationId,
      setComposerRuntimeForConversation,
    ]
  );

  const handleActiveEffortChange = useCallback(
    (
      effort: string,
      providerSupportedEfforts?: readonly string[] | null,
      providerSupportedModelAliases?: readonly string[] | null,
    ) => {
      if (!runtimeConversationId) {
        return null;
      }
      const normalizedRuntime = normalizeRuntimeSelection(
        {
          provider: normalizedActiveRuntime.provider,
          modelId: normalizedActiveRuntime.modelId,
          effort: effort as AgentEffort,
        },
        modelRegistry,
        providerSupportedEfforts,
        providerSupportedModelAliases
      );
      setComposerRuntimeForConversation(
        runtimeConversationId,
        activeProjectId,
        normalizedRuntime,
      );
      return normalizedRuntime;
    },
    [
      activeProjectId,
      modelRegistry,
      normalizedActiveRuntime.modelId,
      normalizedActiveRuntime.provider,
      runtimeConversationId,
      setComposerRuntimeForConversation,
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

  const handleActiveCapabilityChange = useCallback(
    async (coordinationMode: CapabilityIntent["coordinationMode"]) => {
      if (
        !selectedConversationId ||
        !activeProjectId ||
        !activeConversation ||
        activeConversation.contextType !== "project"
      ) {
        return;
      }

      if (activeConversation.coordinationMode === coordinationMode) {
        return;
      }

      setUpdatingCapabilityConversationId(selectedConversationId);
      try {
        await chatApi.updateAgentConversationCoordinationMode({
          conversationId: selectedConversationId,
          coordinationMode,
          ...(coordinationMode === "codex_native_ultra"
            ? { modelOverride: normalizedActiveRuntime.modelId }
            : {}),
        });
        await Promise.all([
          invalidateProjectConversations(activeProjectId),
          invalidateConversationDataQueries(queryClient, selectedConversationId),
        ]);
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to change capability");
      } finally {
        setUpdatingCapabilityConversationId(null);
      }
    },
    [
      activeConversation,
      activeProjectId,
      invalidateProjectConversations,
      normalizedActiveRuntime.modelId,
      queryClient,
      selectedConversationId,
    ],
  );

  return {
    activeProjectOptions,
    defaultRuntime,
    handleActiveConversationModeChange,
    handleActiveConversationModeMenuOpen,
    handleActiveCapabilityChange,
    handleActiveEffortChange,
    handleActiveModelChange,
    handleActiveProviderChange,
    switchingConversationModeId,
    updatingCapabilityConversationId,
  };
}
