import { chatApi } from "@/api/chat";
import { isRemoteEnvironmentActive } from "@/hooks/useActiveEnvironment";
import { useEnvironmentStore } from "@/stores/environmentStore";
import { isRemoteTransportError } from "@/lib/remote/transport-errors";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { reconcileAgentConversationRuntimeStatus } from "./agentConversationRuntimeStore";
import { runtimeIndexToConversationStatus } from "./useAgentConversationRuntimeIndex";

let runtimeIndexReconcileGeneration = 0;

function retainsRuntimeIndexAuthority(
  generation: number,
  environmentId: string,
): boolean {
  return (
    generation === runtimeIndexReconcileGeneration &&
    useEnvironmentStore.getState().activeEnvironmentId === environmentId &&
    isRemoteEnvironmentActive()
  );
}

export async function reconcileAgentConversationRuntimeIndexes(
  environmentId: string,
  conversations: AgentConversation[],
  onError?: (error: unknown, conversation: AgentConversation) => void,
): Promise<void> {
  const generation = ++runtimeIndexReconcileGeneration;
  let reportedError = false;

  for (const conversation of conversations) {
    if (!retainsRuntimeIndexAuthority(generation, environmentId)) return;

    try {
      const index = await chatApi.getAgentConversationRuntimeIndex(conversation.id);
      if (!retainsRuntimeIndexAuthority(generation, environmentId)) return;

      reconcileAgentConversationRuntimeStatus(
        conversation.id,
        runtimeIndexToConversationStatus(index),
        { storeKey: getAgentConversationStoreKey(conversation) },
      );
    } catch (error: unknown) {
      if (!retainsRuntimeIndexAuthority(generation, environmentId)) return;
      if (!reportedError) {
        reportedError = true;
        onError?.(error, conversation);
      }
      if (
        isRemoteTransportError(error) &&
        error.code === "REMOTE_COMMAND_UNAVAILABLE"
      ) {
        return;
      }
    }
  }
}
