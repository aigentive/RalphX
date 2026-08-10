/**
 * P-28 client consumption: the gate reads the LIVE confirmed scopes, never the
 * pair-time snapshot (PR 2.6-b, A4).
 *
 * The negative test is the important one. A pair-time snapshot containing `ui:agent`
 * is exactly what an environment looks like after the host revokes the grant — the
 * registry row is unchanged, only the live session says otherwise. A gate that read
 * the snapshot would present every steering control as live on a device the host has
 * deliberately demoted, and nothing about the UI would look wrong.
 */

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";
import { useActiveEffectiveScopes, useAgentGate } from "@/hooks/useAgentGate";
import { AGENT_CONTROL_DISABLED_HINT } from "@/lib/remote/agent-gate";

const REMOTE_ID = "env-remote";

/** A registry summary whose PAIR-TIME scopes include `ui:agent`. */
const SNAPSHOT_WITH_AGENT = {
  id: REMOTE_ID,
  name: "Studio Mac",
  scopes: ["ui:read", "ui:operate", "ui:agent"],
} as never;

/**
 * Seeds a CONNECTED presentation alongside the scopes.
 *
 * 2.7-a folded degraded-mode read-only into this same gate, and read-only outranks the
 * scope answer — correctly, since there is nothing to steer while disconnected. These
 * tests are about the SCOPE dimension, so the connection dimension is held healthy;
 * the interaction between the two is pinned in `useEnvironmentWritable.test.ts`.
 */
function seed(effectiveScopes: Record<string, readonly string[] | null>): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: REMOTE_ID,
    connectionPresentations: {
      [REMOTE_ID]: {
        presentation: "connected",
        blockedFailure: null,
        blockedMessage: null,
      },
      "env-other": {
        presentation: "connected",
        blockedFailure: null,
        blockedMessage: null,
      },
    },
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      {
        id: REMOTE_ID,
        name: "Studio Mac",
        kind: "remote",
        remote: SNAPSHOT_WITH_AGENT,
      },
    ],
    effectiveScopes,
  });
}

beforeEach(() => {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    effectiveScopes: {},
    connectionPresentations: {},
  });
});

describe("agent gate scope consumption", () => {
  it("ignores a stale pair-time snapshot that still grants ui:agent", () => {
    seed({ [REMOTE_ID]: ["ui:read", "ui:operate"] });

    const { result } = renderHook(() => useAgentGate());
    expect(result.current.gated).toBe(true);
    expect(result.current.reason).toBe(AGENT_CONTROL_DISABLED_HINT);

    // The snapshot is still sitting right there, saying the opposite.
    expect(
      useEnvironmentStore
        .getState()
        .environments.find((entry) => entry.id === REMOTE_ID)?.remote?.scopes
    ).toContain("ui:agent");
  });

  it("gates when introspection has never confirmed a set", () => {
    seed({});
    expect(renderHook(() => useAgentGate()).result.current.gated).toBe(true);
    expect(renderHook(() => useActiveEffectiveScopes()).result.current).toBeNull();
  });

  it("gates when the confirmed set is explicitly null", () => {
    seed({ [REMOTE_ID]: null });
    expect(renderHook(() => useAgentGate()).result.current.gated).toBe(true);
  });

  it("enables when the live confirmed set includes ui:agent", () => {
    seed({ [REMOTE_ID]: ["ui:read", "ui:operate", "ui:agent"] });
    const gate = renderHook(() => useAgentGate()).result.current;
    expect(gate.gated).toBe(false);
    expect(gate.reason).toBeNull();
  });

  it("never gates the local environment", () => {
    useEnvironmentStore.setState({ effectiveScopes: {} });
    expect(renderHook(() => useAgentGate()).result.current.gated).toBe(false);
  });

  it("flips with reconnect-delivered scopes, with no re-pairing", () => {
    seed({ [REMOTE_ID]: ["ui:read", "ui:operate"] });
    const { result } = renderHook(() => useAgentGate());
    expect(result.current.gated).toBe(true);

    // Reconnect confirms a WIDER set.
    act(() => {
      useEnvironmentStore
        .getState()
        .setEffectiveScopes(REMOTE_ID, ["ui:read", "ui:operate", "ui:agent"]);
    });
    expect(result.current.gated).toBe(false);

    // A later reconnect confirms a NARROWER set — the gate closes again.
    act(() => {
      useEnvironmentStore
        .getState()
        .setEffectiveScopes(REMOTE_ID, ["ui:read", "ui:operate"]);
    });
    expect(result.current.gated).toBe(true);

    // Throughout, the registry row was never touched.
    expect(
      useEnvironmentStore
        .getState()
        .environments.find((entry) => entry.id === REMOTE_ID)?.remote?.scopes
    ).toEqual(["ui:read", "ui:operate", "ui:agent"]);
  });

  it("follows the active environment when switching", () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [
        { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
        { id: REMOTE_ID, name: "Studio Mac", kind: "remote" },
        { id: "env-other", name: "Other Mac", kind: "remote" },
      ],
      effectiveScopes: {
        [REMOTE_ID]: ["ui:read", "ui:operate", "ui:agent"],
        "env-other": ["ui:read"],
      },
      // Both remotes healthy: the scope dimension is what is under test here.
      connectionPresentations: {
        [REMOTE_ID]: {
          presentation: "connected",
          blockedFailure: null,
          blockedMessage: null,
        },
        "env-other": {
          presentation: "connected",
          blockedFailure: null,
          blockedMessage: null,
        },
      },
    });

    const { result } = renderHook(() => useAgentGate());
    expect(result.current.gated).toBe(false); // local

    act(() => {
      useEnvironmentStore.setState({ activeEnvironmentId: REMOTE_ID });
    });
    expect(result.current.gated).toBe(false); // granted

    // A different environment's grant must not leak across the switch.
    act(() => {
      useEnvironmentStore.setState({ activeEnvironmentId: "env-other" });
    });
    expect(result.current.gated).toBe(true);
  });

  it("forgets a removed environment's confirmed scopes", () => {
    seed({ [REMOTE_ID]: ["ui:read", "ui:operate", "ui:agent"] });

    // A re-added row can reuse the id; inheriting the old grant would authorize
    // agent control that was never re-confirmed.
    act(() => {
      useEnvironmentStore.getState().setEnvironments([]);
    });
    expect(useEnvironmentStore.getState().effectiveScopes).toEqual({});
  });
});
