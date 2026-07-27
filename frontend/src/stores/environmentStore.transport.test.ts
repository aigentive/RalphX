/**
 * The store→transport mirror (PR 2.2).
 *
 * `environmentStore` is the only writer of the id the invoke/fetch wrappers route on.
 * If a store path ever set `activeEnvironmentId` without mirroring it, the UI would
 * show one environment while every request went to another — so the mirror is
 * asserted on each path that assigns it, not only the happy one.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { remoteEnvironmentsApi } from "@/api/remote-environments";
import type { RemoteEnvironmentSummary } from "@/api/remote-environments";
import {
  getTransportEnvironmentId,
  resetTransportEnvironmentId,
} from "@/lib/remote/active-environment";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "./environmentStore";

vi.mock("@/api/remote-environments", () => ({
  remoteEnvironmentsApi: {
    pair: vi.fn(),
    list: vi.fn(),
    remove: vi.fn(),
    getActiveEnvironment: vi.fn(),
    setActiveEnvironment: vi.fn(),
  },
}));

const mockedApi = vi.mocked(remoteEnvironmentsApi);

const summary = (id: string): RemoteEnvironmentSummary => ({
  id,
  environmentId: `env-${id}`,
  name: `Host ${id}`,
  baseUrl: "https://mac-studio.tailnet.ts.net",
  candidateUrls: [],
  scopes: ["ui:read", "ui:operate"],
  protocolVersion: 1,
  status: "active",
  createdAt: "2026-07-27T19:15:00+00:00",
  lastConnectedAt: null,
});

beforeEach(() => {
  vi.clearAllMocks();
  resetTransportEnvironmentId();
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: "row-1", name: "Host row-1", kind: "remote", remote: summary("row-1") },
    ],
    connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },
  });
});

afterEach(() => {
  resetTransportEnvironmentId();
});

describe("transport mirror", () => {
  it("starts local", () => {
    expect(getTransportEnvironmentId()).toBe(LOCAL_ENVIRONMENT_ID);
  });

  it("follows an accepted switch", async () => {
    mockedApi.setActiveEnvironment.mockResolvedValue(null);

    await useEnvironmentStore.getState().setActiveEnvironment("row-1");

    expect(getTransportEnvironmentId()).toBe("row-1");
  });

  it("follows the revert when Rust refuses the switch", async () => {
    mockedApi.setActiveEnvironment.mockRejectedValue(new Error("refused"));

    await expect(
      useEnvironmentStore.getState().setActiveEnvironment("row-1")
    ).rejects.toThrow("refused");

    expect(
      getTransportEnvironmentId(),
      "a refused switch must not leave the transport aimed at an environment the proxy rejects"
    ).toBe(LOCAL_ENVIRONMENT_ID);
  });

  it("follows the clamp when the active environment is removed from the registry", () => {
    mockedApi.setActiveEnvironment.mockResolvedValue(null);
    useEnvironmentStore.setState({ activeEnvironmentId: "row-1" });
    useEnvironmentStore.getState().setEnvironments([]);

    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe(
      LOCAL_ENVIRONMENT_ID
    );
    expect(getTransportEnvironmentId()).toBe(LOCAL_ENVIRONMENT_ID);
  });

  it("follows startup hydration of the Rust-side authority", async () => {
    mockedApi.getActiveEnvironment.mockResolvedValue("row-1");

    await useEnvironmentStore.getState().hydrateActiveEnvironment();

    expect(getTransportEnvironmentId()).toBe("row-1");
  });

  it("ignores changes that do not move the active id", () => {
    useEnvironmentStore.getState().setConnectionState("row-1", "backoff");
    expect(getTransportEnvironmentId()).toBe(LOCAL_ENVIRONMENT_ID);
  });
});
