/**
 * Feature-flag dual authority under a remote environment.
 *
 * The regression this guards is quiet: `useFeatureFlags()` follows the active
 * environment (its invoke is host-served and its QueryClient is per-env) while
 * `uiStore.featureFlags` is frozen at the boot-time LOCAL fetch. Reading the wrong
 * one is invisible until someone pairs a host that is configured differently.
 */

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import {
  CLIENT_OWNED_FEATURE_FLAGS,
  getClientOwnedFeatureFlag,
  isClientOwnedFeatureFlag,
  stripClientOwnedFlags,
  useClientOwnedFeatureFlag,
} from "./feature-flag-authority";
import { useUiStore } from "@/stores/uiStore";
import type { FeatureFlags } from "@/types/feature-flags";

const BASE_FLAGS: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  automationsPage: true,
  atlassianOauth: false,
  ticketingDashboard: false,
};

beforeEach(() => {
  useUiStore.getState().setFeatureFlags({ ...BASE_FLAGS });
});

describe("client-owned feature flags", () => {
  it("classifies remoteEnvironments as client-owned", () => {
    expect([...CLIENT_OWNED_FEATURE_FLAGS]).toEqual(["remoteEnvironments"]);
    expect(isClientOwnedFeatureFlag("remoteEnvironments")).toBe(true);
  });

  it("classifies host-behaviour flags as NOT client-owned", () => {
    for (const flag of [
      "activityPage",
      "extensibilityPage",
      "automationsPage",
      "atlassianOauth",
      "ticketingDashboard",
      "agentPersonas",
    ]) {
      expect(isClientOwnedFeatureFlag(flag), flag).toBe(false);
    }
  });

  it("reads a client-owned flag from the local uiStore copy", () => {
    expect(getClientOwnedFeatureFlag("remoteEnvironments")).toBe(false);

    useUiStore.getState().setFeatureFlags({
      ...BASE_FLAGS,
      remoteEnvironments: true,
    });
    expect(getClientOwnedFeatureFlag("remoteEnvironments")).toBe(true);
  });

  it("strips a host-supplied client-owned flag from an env-scoped payload", () => {
    // A host answering `get_ui_feature_flags` cannot flip this client into remote
    // mode through the per-environment query cache.
    const hostPayload = {
      ...BASE_FLAGS,
      remoteEnvironments: true,
      agentPersonas: true,
    } as FeatureFlags;

    const stripped = stripClientOwnedFlags(hostPayload);
    expect("remoteEnvironments" in stripped).toBe(false);
    // Host-behaviour flags survive untouched — the host IS authoritative for those.
    expect(stripped.agentPersonas).toBe(true);
    expect(stripped.activityPage).toBe(true);
  });

  it("re-renders a gate when the boot-time fetch resolves", () => {
    // uiStore's flag arrives from an async module-load invoke. A non-reactive read
    // in a render gate would latch `false` and never recover, which is why render
    // gates use the hook rather than `getClientOwnedFeatureFlag`.
    const { result } = renderHook(() =>
      useClientOwnedFeatureFlag("remoteEnvironments")
    );
    expect(result.current).toBe(false);

    act(() => {
      useUiStore.getState().setFeatureFlags({
        ...BASE_FLAGS,
        remoteEnvironments: true,
      });
    });
    expect(result.current).toBe(true);
  });

  it("does not mutate the payload it strips", () => {
    const hostPayload = { ...BASE_FLAGS, remoteEnvironments: true } as FeatureFlags;
    stripClientOwnedFlags(hostPayload);
    expect(hostPayload.remoteEnvironments).toBe(true);
  });
});

describe("authority wiring", () => {
  it("keeps the remote runtime reading remoteEnvironments from the client copy", async () => {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const source = readFileSync(
      resolve(__dirname, "environment-runtime.ts"),
      "utf8"
    );

    // The composition root must use the authority helper (or `uiStore`), never the
    // env-scoped query — reading the host's answer here would be circular.
    expect(source).toContain('getClientOwnedFeatureFlag("remoteEnvironments")');
    // Match an IMPORT of the query hook, not the prose that explains why it is absent.
    expect(source).not.toMatch(/import[^;]*useFeatureFlags/);
  });

  // The regression this guards actually shipped: all four settings gates read
  // `useFeatureFlags()`, whose data has `remoteEnvironments` stripped, so the panes
  // stayed dark with `remote_environments: true` in config. Their own suites mocked
  // the hook and so proved nothing about the real authority.
  it.each([
    "../../components/settings/SettingsDialog.tsx",
    "../../components/settings/IntegrationsHubSection.tsx",
    "../../components/settings/remote-access/RemoteAccessSection.tsx",
    "../../components/settings/connections/ConnectionsSection.tsx",
  ])("keeps %s off the env-scoped query for remoteEnvironments", async (file) => {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const source = readFileSync(resolve(__dirname, file), "utf8");

    expect(source).toContain('useClientOwnedFeatureFlag("remoteEnvironments")');
    expect(source).not.toMatch(/import[^;]*useFeatureFlags/);
  });

  it("keeps the env-scoped query stripping client-owned flags", async () => {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const source = readFileSync(
      resolve(__dirname, "../../hooks/useFeatureFlags.ts"),
      "utf8"
    );
    expect(source).toContain("stripClientOwnedFlags");
  });
});
