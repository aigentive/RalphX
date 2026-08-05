import { beforeEach, describe, expect, it, vi } from "vitest";

vi.unmock("@tauri-apps/api/core");

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import { mcpPolicyApi } from "@/api/mcp-policy";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const RAW_CATALOG = {
  eligible_providers: ["codex"],
  eligible_default_provider: "codex",
  probed_at: "2026-08-05T18:00:00+00:00",
  probe_stale: true,
  provider_diagnostics: {},
  policy_diagnostics: [],
  servers: [],
} as const;

function useRemoteEnvironment(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: "remote-1",
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: "remote-1", name: "Host Mac", kind: "remote" },
    ],
    effectiveScopes: { "remote-1": ["ui:read"] },
    connectionPresentations: {},
  });
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    effectiveScopes: {},
    connectionPresentations: {},
  });
});

describe("MCP catalog remote routing", () => {
  it("uses the spawn-free twin and parses the real ok envelope", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        snapshot: RAW_CATALOG,
        captured_at: "2026-08-05T18:01:00+00:00",
      },
    });

    const catalog = await mcpPolicyApi.get({ projectId: null, provider: "codex" });

    expect(primitiveInvoke).toHaveBeenCalledWith("remote_invoke", {
      request: expect.objectContaining({
        command: "get_remote_mcp_catalog",
        args: { input: { projectId: null, provider: "codex" } },
      }),
    });
    expect(catalog?.probeStale).toBe(true);
    expect(catalog?.probedAt).toBe("2026-08-05T18:00:00+00:00");
  });

  it("preserves snapshot absence as null instead of inventing an empty catalog", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: { snapshot: null, captured_at: null },
    });

    await expect(
      mcpPolicyApi.get({ projectId: "project-1", provider: "codex" }),
    ).resolves.toBeNull();
  });

  it("keeps the host-local read on get_mcp_catalog", async () => {
    primitiveInvoke.mockResolvedValue({ ...RAW_CATALOG, probe_stale: false });

    await mcpPolicyApi.get({ projectId: null, provider: "codex" });

    expect(primitiveInvoke).toHaveBeenCalledWith("get_mcp_catalog", {
      input: { projectId: null, provider: "codex" },
    });
  });
});
