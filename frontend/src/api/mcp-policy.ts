import { typedInvoke } from "@/lib/tauri";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";

import type { Harness } from "./ideation-harness";
import {
  McpMutationResponseSchema,
  RawMcpCatalogSchema,
  RawRemoteMcpCatalogSchema,
} from "./mcp-policy.schemas";
import { transformMcpCatalog } from "./mcp-policy.transforms";
import type {
  McpCatalog,
  McpScopeInput,
  McpServerOverrideInput,
  McpToolOverrideInput,
  RetryLegacyMcpRepairInput,
} from "./mcp-policy.types";

async function catalog(command: string, input: object): Promise<McpCatalog> {
  const raw = await typedInvoke(command, { input }, RawMcpCatalogSchema);
  return transformMcpCatalog(raw);
}

async function remoteCatalog(input: McpScopeInput): Promise<McpCatalog | null> {
  const raw = await typedInvoke(
    "get_remote_mcp_catalog",
    { input },
    RawRemoteMcpCatalogSchema,
  );
  return raw.snapshot === null ? null : transformMcpCatalog(raw.snapshot);
}

export const mcpPolicyApi = {
  get(input: McpScopeInput): Promise<McpCatalog | null> {
    return isRemoteEnvironmentId(getTransportEnvironmentId())
      ? remoteCatalog(input)
      : catalog("get_mcp_catalog", input);
  },
  refresh(input: McpScopeInput & { provider: Harness }): Promise<McpCatalog> {
    return catalog("refresh_mcp_catalog", input);
  },
  updateServer(input: McpServerOverrideInput) {
    return typedInvoke(
      "update_mcp_server_override",
      { input },
      McpMutationResponseSchema,
    );
  },
  clearServer(input: Omit<McpServerOverrideInput, "state">) {
    return typedInvoke(
      "clear_mcp_server_override",
      { input },
      McpMutationResponseSchema,
    );
  },
  updateTool(input: McpToolOverrideInput) {
    return typedInvoke(
      "update_mcp_tool_override",
      { input },
      McpMutationResponseSchema,
    );
  },
  clearTool(input: Omit<McpToolOverrideInput, "state">) {
    return typedInvoke(
      "clear_mcp_tool_override",
      { input },
      McpMutationResponseSchema,
    );
  },
  retryLegacyRepair(input: RetryLegacyMcpRepairInput) {
    return typedInvoke(
      "retry_legacy_mcp_registration_repair",
      { input },
      McpMutationResponseSchema,
    );
  },
} as const;

export type * from "./mcp-policy.types";
