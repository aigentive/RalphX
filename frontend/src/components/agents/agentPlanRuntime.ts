import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";
import type { AgentModelRegistry } from "@/lib/agent-models";
import type {
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";

import { defaultEffortForModel, defaultModelForProvider } from "./agentOptions";

export function materializeWorkspaceRuntimeSelection(
  selection: ManualRoleRuntimeSelection,
  registry: AgentModelRegistry,
): AgentRuntimeSelection {
  const provider = selection.provider as AgentProvider;
  const modelId = selection.model ?? defaultModelForProvider(provider, registry);
  return {
    provider,
    modelId,
    effort:
      (selection.effort as AgentRuntimeSelection["effort"] | null) ??
      defaultEffortForModel(provider, modelId, registry),
  };
}
