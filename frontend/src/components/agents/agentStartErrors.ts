import { z } from "zod";

import type { AutomationAuthoringMode } from "@/api/automations";
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
export const MCP_SETUP_PREFLIGHT_MARKER = "[ralphx:mcp_setup_preflight]";

const McpSetupPreflightPayloadSchema = z
  .object({
    provider: z.enum(["claude", "codex"]),
    server_id: z.string().min(1),
    scope: z.string().nullable(),
    conflict_kind: z.enum([
      "ambiguous_reserved_id",
      "legacy_registration",
      "legacy_repair_failed",
    ]),
    repair_status: z.enum(["repairable", "repaired", "failed", "manual_only"]),
  })
  .strict();

export interface McpSetupPreflightFailureDetails {
  provider: "claude" | "codex";
  serverId: string;
  scope: string | null;
  conflictKind:
    | "ambiguous_reserved_id"
    | "legacy_registration"
    | "legacy_repair_failed";
  repairStatus: "repairable" | "repaired" | "failed" | "manual_only";
}

export interface LinkedSetupFailureDetails {
  message: string;
}

export interface AgentStartConversationRetryInputSource {
  projectId: string | null;
  content: string;
  runtime: AgentRuntimeSelection;
  runtimeProviderContext?: AgentRuntimeProviderContext | undefined;
  useRoleDefault?: boolean | undefined;
  mode: AgentConversationWorkspaceMode;
  automationAuthoringMode?: AutomationAuthoringMode | undefined;
  base: AgentConversationBaseSelection | null;
  codexFastMode?: boolean | null | undefined;
  personaId?: string | null | undefined;
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

export function parseMcpSetupPreflightFailure(
  error: unknown,
): McpSetupPreflightFailureDetails | null {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : null;
  const markerIndex = message?.indexOf(MCP_SETUP_PREFLIGHT_MARKER) ?? -1;
  if (!message || markerIndex < 0) {
    return null;
  }
  try {
    const parsed = McpSetupPreflightPayloadSchema.safeParse(
      JSON.parse(message.slice(markerIndex + MCP_SETUP_PREFLIGHT_MARKER.length)),
    );
    if (!parsed.success) {
      return null;
    }
    return {
      provider: parsed.data.provider,
      serverId: parsed.data.server_id,
      scope: parsed.data.scope,
      conflictKind: parsed.data.conflict_kind,
      repairStatus: parsed.data.repair_status,
    };
  } catch {
    return null;
  }
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
  if (input.automationAuthoringMode !== undefined) {
    retryInput.automationAuthoringMode = input.automationAuthoringMode;
  }
  if (input.runtimeProviderContext !== undefined) {
    retryInput.runtimeProviderContext = input.runtimeProviderContext;
  }
  if (input.useRoleDefault !== undefined) {
    retryInput.useRoleDefault = input.useRoleDefault;
  }
  if (input.codexFastMode !== undefined) {
    retryInput.codexFastMode = input.codexFastMode;
  }
  if (input.personaId !== undefined) {
    retryInput.personaId = input.personaId;
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
