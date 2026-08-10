/**
 * The five composer states of the spawn-free conversation start (contract §3.2), proven at
 * the level that decides them: the Start affordance gate and the provider-availability
 * projection. Full `AgentsStartComposer` render coverage would be brittle against a
 * 2000-line component; these unit tests pin the exact logic each row turns on.
 *
 * The Start affordance was repointed from the dead `start_agent_conversation` to the
 * spawn-free `request_remote_agent_conversation_start` (§3.1). Because the gate derives
 * `unavailable`/`gated`/`enabled` purely from that op's presence + class in
 * `REMOTE_FACADE_OPS`, the older-host / gated / enabled rows are exercised by mocking the
 * generated manifest — so this suite is independent of whether the host has registered the
 * command yet (Part B).
 */

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  AGENT_CONTROL_DISABLED_HINT,
  AGENT_GATED_AFFORDANCES,
  REMOTE_UNAVAILABLE_HINT,
} from "./agent-gate";
import { buildAgentProviderAvailabilityOptions } from "@/components/agents/agentProviderAvailability";
import type { AgentProviderSettingsResponse } from "@/api/harness-providers";

const GRANTED = ["ui:read", "ui:operate", "ui:agent"];
const WITHOUT_UI_AGENT = ["ui:read", "ui:operate"];

/** A generated-manifest stand-in with the Start op present at the given class. */
function mockManifest(withStartOp: boolean): void {
  vi.doMock("./remote-capabilities.generated", () => ({
    REMOTE_FACADE_OPS: withStartOp
      ? {
          request_remote_agent_conversation_start: {
            command: "request_remote_agent_conversation_start",
            opClass: "agentControl",
          },
        }
      : {},
    REMOTE_CONDITIONAL_CAPABILITIES: {},
    REMOTE_MANIFEST_SCHEMA_VERSION: 2,
  }));
}

async function resolveStart(
  withStartOp: boolean,
  isRemote: boolean,
  scopes: readonly string[] | null,
) {
  vi.resetModules();
  mockManifest(withStartOp);
  const gate = await import("./agent-gate");
  return gate.resolveAffordanceGate("startConversation", isRemote, scopes);
}

afterEach(() => {
  vi.resetModules();
  vi.doUnmock("./remote-capabilities.generated");
});

describe("Start affordance mapping", () => {
  it("fronts the spawn-free start command, not the process-spawn one", () => {
    expect(AGENT_GATED_AFFORDANCES.startConversation).toBe(
      "request_remote_agent_conversation_start",
    );
  });
});

describe("Start gate — the five §3.2 rows", () => {
  it("row 1 — older host (op absent from the manifest) → unavailable", async () => {
    const state = await resolveStart(false, true, GRANTED);
    expect(state.status).toBe("unavailable");
    expect(state.gated).toBe(true);
    expect(state.reason).toBe(REMOTE_UNAVAILABLE_HINT);
  });

  it("row 2 — remote ui:read/ui:operate (no ui:agent) → gated, agent-control hint", async () => {
    const state = await resolveStart(true, true, WITHOUT_UI_AGENT);
    expect(state.status).toBe("gated");
    expect(state.reason).toBe(AGENT_CONTROL_DISABLED_HINT);
  });

  it("row 4 — remote ui:agent, op present → enabled", async () => {
    const state = await resolveStart(true, true, GRANTED);
    expect(state.status).toBe("enabled");
    expect(state.gated).toBe(false);
    expect(state.reason).toBeNull();
  });

  it("local environment is never gated, even before the host registers the command", async () => {
    const state = await resolveStart(false, false, null);
    expect(state.status).toBe("enabled");
  });
});

/**
 * Rows 3 and 4 also differ in the PROVIDER layer: with `ui:agent` the gate is `enabled`, but
 * whether the composer can actually start depends on the host having an enabled provider. The
 * remote availability mode (Part A) drives this — no CLI probe truth is consulted.
 */
function remoteProviderRow(
  overrides: Partial<AgentProviderSettingsResponse> & { provider: string },
): AgentProviderSettingsResponse {
  return {
    provider: overrides.provider,
    enabled: overrides.enabled ?? false,
    isDefault: overrides.isDefault ?? false,
    model: overrides.model ?? null,
    effort: overrides.effort ?? null,
    serviceTier: null,
    approvalPolicy: null,
    sandboxMode: null,
    claudePermissionMode: null,
    claudeDangerouslySkipPermissions: false,
    claudeAllowDangerouslySkipPermissions: false,
    customBinaryEnabled: false,
    customBinaryPath: null,
    customEnvFileEnabled: false,
    customEnvFilePath: null,
    available: false,
    binaryFound: false,
    status: "Configured on this host",
    error: null,
    missingCoreExecFeatures: [],
    ultraSupportedModels: [],
    supportsFastMode: false,
    fastModeSupportedModels: [],
    updatedAt: "",
  };
}

describe("Provider availability — rows 3 and 4", () => {
  it("row 3 — host not onboarded: every provider disabled with host-configuration copy", () => {
    const options = buildAgentProviderAvailabilityOptions({
      providers: [],
      isReady: true,
      mode: "remote",
    });
    expect(options.length).toBeGreaterThan(0);
    expect(options.every((option) => option.disabled)).toBe(true);
    // Honest "configured on the host" copy — never the local "Provider is not configured."
    expect(options.every((option) => option.disabledReason === "Not configured on this host.")).toBe(
      true,
    );
  });

  it("row 4 — host onboarded: an enabled provider is selectable on stored config alone", () => {
    const options = buildAgentProviderAvailabilityOptions({
      providers: [
        remoteProviderRow({
          provider: "codex",
          enabled: true,
          isDefault: true,
          model: "gpt-5.6-sol",
        }),
      ],
      isReady: true,
      mode: "remote",
    });
    const codex = options.find((option) => option.id === "codex");
    // `available`/`binaryFound` are false (the host never probed) yet the provider is offered.
    expect(codex?.disabled).toBeFalsy();
  });
});
