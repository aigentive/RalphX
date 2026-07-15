import type {
  AgentConversationBaseSelection,
  AgentConversationWorkspaceMode,
  ComposerArtifactReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
  CapabilityIntent,
  TeamIntent,
} from "@/api/chat";
import type {
  AgentRuntimeProviderContext,
  AgentRuntimeSelection,
  AgentStartConversationRetryInput,
} from "@/stores/agentSessionStore";

export const LINKED_SETUP_FAILURE_MARKER = "[ralphx:linked_setup_failure]";

export interface LinkedSetupFailureDetails {
  message: string;
}

export interface AgentStartConversationRetryInputSource {
  projectId: string;
  content: string;
  runtime: AgentRuntimeSelection;
  runtimeProviderContext?: AgentRuntimeProviderContext | undefined;
  mode: AgentConversationWorkspaceMode;
  base: AgentConversationBaseSelection | null;
  codexFastMode?: boolean | null | undefined;
  capabilityIntent?: CapabilityIntent | null | undefined;
  teamIntent?: TeamIntent | null | undefined;
  composerArtifactReferences?: ComposerArtifactReference[] | undefined;
  composerIntegrationReferences?: ComposerIntegrationReference[] | undefined;
  composerProjectReferences?: ComposerProjectReference[] | undefined;
}

export function parseLinkedSetupFailure(error: unknown): LinkedSetupFailureDetails | null {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : null;
  if (!message?.includes(LINKED_SETUP_FAILURE_MARKER)) {
    return null;
  }
  const cleaned = message.replace(LINKED_SETUP_FAILURE_MARKER, "").trim();
  return {
    message: cleaned || "Linked branch setup failed.",
  };
}

export function buildAgentStartConversationRetryInput(
  input: AgentStartConversationRetryInputSource,
): AgentStartConversationRetryInput {
  const retryInput: AgentStartConversationRetryInput = {
    projectId: input.projectId,
    content: input.content,
    runtime: input.runtime,
    mode: input.mode,
    base: input.base,
  };
  if (input.runtimeProviderContext !== undefined) {
    retryInput.runtimeProviderContext = input.runtimeProviderContext;
  }
  if (input.codexFastMode !== undefined) {
    retryInput.codexFastMode = input.codexFastMode;
  }
  if (input.capabilityIntent !== undefined) {
    retryInput.capabilityIntent = input.capabilityIntent;
  }
  if (input.teamIntent !== undefined) {
    retryInput.teamIntent = input.teamIntent;
  }
  if (input.composerArtifactReferences !== undefined) {
    retryInput.composerArtifactReferences = input.composerArtifactReferences;
  }
  if (input.composerIntegrationReferences !== undefined) {
    retryInput.composerIntegrationReferences = input.composerIntegrationReferences;
  }
  if (input.composerProjectReferences !== undefined) {
    retryInput.composerProjectReferences = input.composerProjectReferences;
  }
  return retryInput;
}
