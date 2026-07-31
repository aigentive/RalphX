export type RuntimeSource =
  | "composer_selection"
  | "conversation_override"
  | "role_default"
  | "project_default"
  | "harness_fallback";

export interface AgentRunAttribution {
  id: string;
  conversationId: string;
  status: string;
  startedAt: string;
  completedAt: string | null;
  harness: string | null;
  upstreamProvider: string | null;
  providerProfile: string | null;
  providerSessionId: string | null;
  logicalModel: string | null;
  effectiveModelId: string | null;
  logicalEffort: string | null;
  effectiveEffort: string | null;
  serviceTier: string | null;
  approvalPolicy: string | null;
  sandboxMode: string | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheCreationTokens: number | null;
  cacheReadTokens: number | null;
  estimatedUsd: number | null;
  runChainId: string | null;
  actionKind: string | null;
  personaSlug: string | null;
  agentName: string | null;
  launchRole: string | null;
  runtimeSource: RuntimeSource | null;
}
