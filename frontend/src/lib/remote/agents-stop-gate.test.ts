/**
 * The Stop affordance gate (WP2).
 *
 * Two things this pins that nothing else could:
 *
 * 1. Stop fronts the spawn-free `request_remote_agent_stop`, never `stop_agent`. `stop_agent`
 *    reaches `Command::new(resolve_pkill_cli_path())` on the host and stays unregistered by
 *    the absolute process floor, so pointing the gate at it would render Stop permanently
 *    `unavailable` on every paired device — which is exactly the bug the WP fixes.
 * 2. Stop is a BRAKE, so it must resolve `enabled` for the DEFAULT `ui:read` + `ui:operate`
 *    pairing. If the intent were ever reclassified to `agentControl`, this suite fails rather
 *    than silently taking the brakes away from the devices that most need them.
 *
 * Like the Start suite, the manifest is mocked so these rows hold regardless of whether the
 * checked-in generated mirror has been regenerated yet.
 */

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  AGENT_CONTROL_DISABLED_HINT,
  AGENT_GATED_AFFORDANCES,
  REMOTE_UNAVAILABLE_HINT,
} from "./agent-gate";

const GRANTED = ["ui:read", "ui:operate", "ui:agent"];
const WITHOUT_UI_AGENT = ["ui:read", "ui:operate"];

/** A generated-manifest stand-in with the Stop op present at the given class. */
function mockManifest(withStopOp: boolean, opClass = "operate"): void {
  vi.doMock("./remote-capabilities.generated", () => ({
    REMOTE_FACADE_OPS: withStopOp
      ? {
          request_remote_agent_stop: {
            command: "request_remote_agent_stop",
            opClass,
          },
        }
      : {},
    REMOTE_CONDITIONAL_CAPABILITIES: {},
    REMOTE_MANIFEST_SCHEMA_VERSION: 2,
  }));
}

async function resolveStop(
  withStopOp: boolean,
  isRemote: boolean,
  scopes: readonly string[] | null,
  opClass = "operate",
) {
  vi.resetModules();
  mockManifest(withStopOp, opClass);
  const gate = await import("./agent-gate");
  return gate.resolveAffordanceGate("agentStop", isRemote, scopes);
}

afterEach(() => {
  vi.resetModules();
  vi.doUnmock("./remote-capabilities.generated");
});

describe("Stop affordance mapping", () => {
  it("fronts the spawn-free stop intent, not the pkill-resolving command", () => {
    expect(AGENT_GATED_AFFORDANCES.agentStop).toBe("request_remote_agent_stop");
    expect(AGENT_GATED_AFFORDANCES.agentStop).not.toBe("stop_agent");
  });
});

describe("Stop gate", () => {
  it("older host (op absent) → unavailable hint, not an enabled dead button", async () => {
    const state = await resolveStop(false, true, GRANTED);
    expect(state.status).toBe("unavailable");
    expect(state.gated).toBe(true);
    expect(state.reason).toBe(REMOTE_UNAVAILABLE_HINT);
  });

  it("the DEFAULT pairing can stop — brakes must not require ui:agent", async () => {
    const state = await resolveStop(true, true, WITHOUT_UI_AGENT);
    expect(state.status).toBe("enabled");
    expect(state.gated).toBe(false);
    expect(state.reason).toBeNull();
  });

  it("unknown scopes still resolve enabled for an operate-class brake", async () => {
    // Unknown scopes gate CLOSED for agentControl ops; an `operate` op is reachable by any
    // paired device, so the brake stays live rather than disappearing mid-reconnect.
    const state = await resolveStop(true, true, null);
    expect(state.status).toBe("enabled");
  });

  it("a granted device is of course enabled too", async () => {
    const state = await resolveStop(true, true, GRANTED);
    expect(state.status).toBe("enabled");
  });

  it("local environment is never gated", async () => {
    const state = await resolveStop(false, false, null);
    expect(state.status).toBe("enabled");
  });

  /**
   * The regression guard: if the host ever reclassified the intent to `agentControl`, the
   * device without ui:agent would lose the brake. This documents that consequence so the change is a
   * deliberate contract edit rather than an accident.
   */
  it("would gate a device without ui:agent if the intent were reclassified agentControl", async () => {
    const state = await resolveStop(true, true, WITHOUT_UI_AGENT, "agentControl");
    expect(state.status).toBe("gated");
    expect(state.reason).toBe(AGENT_CONTROL_DISABLED_HINT);
  });
});
