import { describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

import { mcpPolicyApi } from "./mcp-policy";
import { RawMcpCatalogSchema } from "./mcp-policy.schemas";
import { transformMcpCatalog } from "./mcp-policy.transforms";

describe("MCP policy API contract", () => {
  it("transforms the redacted snake-case catalog without retaining provider definitions", () => {
    const raw = RawMcpCatalogSchema.parse({
      eligible_providers: ["codex"],
      eligible_default_provider: "codex",
      probed_at: "2026-07-18T00:00:00Z",
      probe_stale: false,
      provider_diagnostics: {},
      policy_diagnostics: ["Project MCP policy: invalid server identifier"],
      servers: [
        {
          provider: "codex",
          server_id: "github",
          native_scope: "user",
          native_state: "enabled",
          effective_enabled: true,
          configured_state: "follow",
          effective_state: "enabled",
          effective_source: "provider_native",
          known_tools: [
            {
              tool_name: "search",
              configured_state: "disabled",
              effective_state: "disabled",
              effective_source: "global_ui",
            },
          ],
          disabled_tools: ["search"],
          locked: false,
          conflict_kind: "ambiguous_reserved_id",
          repair_status: "manual_only",
          command: "/secret/provider-command",
          env: { TOKEN: "secret" },
        },
      ],
    });

    expect(raw.servers[0]).not.toHaveProperty("command");
    expect(raw.servers[0]).not.toHaveProperty("env");
    expect(transformMcpCatalog(raw)).toMatchObject({
      eligibleDefaultProvider: "codex",
      policyDiagnostics: ["Project MCP policy: invalid server identifier"],
      servers: [
        {
          serverId: "github",
          configuredState: "follow",
          conflictKind: "ambiguous_reserved_id",
          repairStatus: "manual_only",
          knownTools: [
            {
              toolName: "search",
              configuredState: "disabled",
              effectiveSource: "global_ui",
            },
          ],
        },
      ],
    });
  });

  it("routes a retry through the narrow legacy-cleanup command", async () => {
    vi.mocked(typedInvoke).mockResolvedValue(null);

    await mcpPolicyApi.retryLegacyRepair({
      provider: "claude",
      serverId: "ralphx",
      scope: "user",
    });

    expect(typedInvoke).toHaveBeenCalledWith(
      "retry_legacy_mcp_registration_repair",
      {
        input: {
          provider: "claude",
          serverId: "ralphx",
          scope: "user",
        },
      },
      expect.anything(),
    );
  });
});
