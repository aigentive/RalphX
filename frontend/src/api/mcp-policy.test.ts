import { describe, expect, it } from "vitest";

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
          command: "/secret/provider-command",
          env: { TOKEN: "secret" },
        },
      ],
    });

    expect(raw.servers[0]).not.toHaveProperty("command");
    expect(raw.servers[0]).not.toHaveProperty("env");
    expect(transformMcpCatalog(raw)).toMatchObject({
      eligibleDefaultProvider: "codex",
      servers: [
        {
          serverId: "github",
          configuredState: "follow",
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
});
