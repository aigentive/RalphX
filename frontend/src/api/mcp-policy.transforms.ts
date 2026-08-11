import type { RawMcpCatalog } from "./mcp-policy.schemas";
import type { McpCatalog } from "./mcp-policy.types";

export function transformMcpCatalog(raw: RawMcpCatalog): McpCatalog {
  return {
    eligibleProviders: raw.eligible_providers,
    eligibleDefaultProvider: raw.eligible_default_provider ?? null,
    probedAt: raw.probed_at,
    probeStale: raw.probe_stale,
    providerDiagnostics: raw.provider_diagnostics,
    policyDiagnostics: raw.policy_diagnostics,
    servers: raw.servers.map((server) => ({
      provider: server.provider,
      serverId: server.server_id,
      nativeScope: server.native_scope ?? null,
      nativeState: server.native_state,
      effectiveEnabled: server.effective_enabled,
      configuredState: server.configured_state,
      effectiveState: server.effective_state,
      effectiveSource: server.effective_source,
      knownTools: server.known_tools.map((tool) => ({
        toolName: tool.tool_name,
        configuredState: tool.configured_state,
        effectiveState: tool.effective_state,
        effectiveSource: tool.effective_source,
      })),
      disabledTools: server.disabled_tools,
      locked: server.locked,
      lockedReason: server.locked_reason ?? null,
      diagnostic: server.diagnostic ?? null,
      conflictKind: server.conflict_kind ?? null,
      repairStatus: server.repair_status ?? null,
    })),
  };
}
