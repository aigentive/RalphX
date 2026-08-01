/**
 * The remote half of `permissionApi.resolveRequest` (Phase 1 — the permission brake).
 *
 * Like `chat-remote-stop.test.ts`, these exercise the REAL transport wrapper —
 * `@tauri-apps/api/core` is aliased to `lib/remote/invoke`, so a remote environment routes
 * through `networkInvoke` and out via the `remote_invoke` primitive. Mocking the API module
 * would prove nothing about the routing, which is the only thing in question here.
 *
 * What is pinned: a paired device NEVER names `resolve_permission_request` (the facade does
 * not register the dual-decision command — one op that allows OR denies by argument would
 * have to sit at `agentControl`, taking the deny brake away from every default pairing). It
 * names the server-pinned split instead: `deny_permission_request` (`operate`) and
 * `approve_permission_request` (`agentControl`). And the local environment is byte-identical
 * to what it was before the split existed.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.unmock("@tauri-apps/api/core");

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import { permissionApi } from "@/api/permission";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";
const REQUEST_ID = "perm-req-1";

/** The DEFAULT pairing: read + operate, no `ui:agent`. */
const DEFAULT_PAIRED = ["ui:read", "ui:operate"];
const AGENT_GRANTED = ["ui:read", "ui:operate", "ui:agent"];

function useRemoteEnvironment(scopes: string[] = DEFAULT_PAIRED): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: REMOTE_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: REMOTE_ID, name: "Studio Mac", kind: "remote" },
    ],
    connectionPresentations: {
      [REMOTE_ID]: {
        presentation: "connected",
        blockedFailure: null,
        blockedMessage: null,
      },
    },
    effectiveScopes: { [REMOTE_ID]: scopes },
  });
}

function useLocalEnvironment(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    effectiveScopes: {},
    connectionPresentations: {},
  });
}

function wireInput(callIndex = 0): Record<string, unknown> {
  const args = primitiveInvoke.mock.calls[callIndex]?.[1] as {
    input: Record<string, unknown>;
  };
  return args.input;
}

function wireCmd(callIndex = 0): string {
  return wireInput(callIndex).cmd as string;
}

function routeRemote(): void {
  primitiveInvoke.mockResolvedValue({
    outcome: "ok",
    result: { success: true, message: null },
  });
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  useLocalEnvironment();
});

describe("remote permission-gate routing", () => {
  it("routes deny to the pinned deny op from the DEFAULT pairing", async () => {
    useRemoteEnvironment(DEFAULT_PAIRED);
    routeRemote();

    await permissionApi.resolveRequest({
      requestId: REQUEST_ID,
      decision: "deny",
    });

    expect(
      primitiveInvoke.mock.calls.every((call) => call[0] === "remote_invoke"),
    ).toBe(true);
    expect(wireCmd(0)).toBe("deny_permission_request");
  });

  it("routes approve to the pinned approve op", async () => {
    useRemoteEnvironment(AGENT_GRANTED);
    routeRemote();

    await permissionApi.resolveRequest({
      requestId: REQUEST_ID,
      decision: "allow",
    });

    expect(wireCmd(0)).toBe("approve_permission_request");
  });

  it("never names the unregistered dual-decision command on a paired device", async () => {
    useRemoteEnvironment(AGENT_GRANTED);
    routeRemote();

    await permissionApi.resolveRequest({ requestId: REQUEST_ID, decision: "allow" });
    await permissionApi.resolveRequest({ requestId: REQUEST_ID, decision: "deny" });

    const named = primitiveInvoke.mock.calls.map((_call, index) => wireCmd(index));
    expect(named).not.toContain("resolve_permission_request");
    expect(named).toEqual([
      "approve_permission_request",
      "deny_permission_request",
    ]);
  });

  it("sends the unchanged local payload shape — only the command name differs", async () => {
    useRemoteEnvironment(AGENT_GRANTED);
    routeRemote();

    await permissionApi.resolveRequest({
      requestId: REQUEST_ID,
      decision: "allow",
      message: "ok",
    });
    await permissionApi.resolveRequest({
      requestId: REQUEST_ID,
      decision: "deny",
      message: "ok",
    });

    // The remote branch must not fork the argument shape: `approve_/deny_` dispatch to the
    // SAME target fn as the local command, and `extract_pinned_arg` overwrites `decision`
    // server-side from the op name — so the field on the wire carries no authority and the
    // payload stays exactly what the local path sends.
    expect(wireInput(0).args).toEqual({
      args: { request_id: REQUEST_ID, decision: "allow", message: "ok" },
    });
    expect(wireInput(1).args).toEqual({
      args: { request_id: REQUEST_ID, decision: "deny", message: "ok" },
    });
  });

  it("surfaces a host refusal instead of resolving silently", async () => {
    useRemoteEnvironment(DEFAULT_PAIRED);
    primitiveInvoke.mockResolvedValue({
      outcome: "error",
      error: { code: "REMOTE_FORBIDDEN", message: "not allowed" },
    });

    await expect(
      permissionApi.resolveRequest({ requestId: REQUEST_ID, decision: "allow" }),
    ).rejects.toBeDefined();
  });

  it("keeps the local command byte-identical on a local environment", async () => {
    useLocalEnvironment();
    primitiveInvoke.mockResolvedValue(undefined);

    await permissionApi.resolveRequest({
      requestId: REQUEST_ID,
      decision: "deny",
      message: "no",
    });

    expect(primitiveInvoke).toHaveBeenCalledWith(
      "resolve_permission_request",
      { args: { request_id: REQUEST_ID, decision: "deny", message: "no" } },
      undefined,
    );
  });

  it("keeps the local allow path on the dual-decision command too", async () => {
    useLocalEnvironment();
    primitiveInvoke.mockResolvedValue(undefined);

    await permissionApi.resolveRequest({ requestId: REQUEST_ID, decision: "allow" });

    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("resolve_permission_request");
  });
});
