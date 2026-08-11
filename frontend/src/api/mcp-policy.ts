import { typedInvoke } from "@/lib/tauri";

import type { Harness } from "./ideation-harness";
import { McpMutationResponseSchema, RawMcpCatalogSchema } from "./mcp-policy.schemas";
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

export const mcpPolicyApi = {
  get(input: McpScopeInput): Promise<McpCatalog> {
    return catalog("get_mcp_catalog", input);
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
