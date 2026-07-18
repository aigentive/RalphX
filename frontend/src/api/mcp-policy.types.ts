import { type z } from "zod";

import type { Harness } from "./ideation-harness";
import type {
  McpOverrideStateSchema,
  McpPolicySourceSchema,
  NativeMcpStateSchema,
} from "./mcp-policy.schemas";

export type McpOverrideState = z.infer<typeof McpOverrideStateSchema>;
export type McpPolicySource = z.infer<typeof McpPolicySourceSchema>;
export type NativeMcpState = z.infer<typeof NativeMcpStateSchema>;

export interface McpTool {
  toolName: string;
  configuredState: McpOverrideState;
  effectiveState: McpOverrideState;
  effectiveSource: McpPolicySource;
}

export interface McpServer {
  provider: Harness;
  serverId: string;
  nativeScope: string | null;
  nativeState: NativeMcpState;
  effectiveEnabled: boolean;
  configuredState: McpOverrideState;
  effectiveState: McpOverrideState;
  effectiveSource: McpPolicySource;
  knownTools: McpTool[];
  disabledTools: string[];
  locked: boolean;
  lockedReason: string | null;
  diagnostic: string | null;
}

export interface McpCatalog {
  eligibleProviders: Harness[];
  eligibleDefaultProvider: Harness | null;
  probedAt: string;
  probeStale: boolean;
  providerDiagnostics: Record<string, string>;
  servers: McpServer[];
}

export interface McpScopeInput {
  projectId: string | null;
  provider?: Harness | null;
}

export interface McpServerOverrideInput extends McpScopeInput {
  provider: Harness;
  serverId: string;
  state: McpOverrideState;
}

export interface McpToolOverrideInput extends McpServerOverrideInput {
  toolName: string;
}
