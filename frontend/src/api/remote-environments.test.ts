import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { remoteEnvironmentsApi } from "./remote-environments";

const summary = {
  id: "row-1",
  environmentId: "env-1",
  name: "Mac Studio",
  baseUrl: "https://mac-studio.tailnet.ts.net",
  candidateUrls: [],
  scopes: [],
  protocolVersion: 1,
  status: "active",
  createdAt: "2026-07-27T19:15:00+00:00",
  lastConnectedAt: null,
} as const;

describe("remoteEnvironmentsApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("pairs through the wrapped Tauri input contract", async () => {
    vi.mocked(invoke).mockResolvedValue(summary);

    await expect(
      remoteEnvironmentsApi.pair(
        "https://mac-studio.tailnet.ts.net",
        "rxp_code",
        "Mac Studio"
      )
    ).resolves.toEqual(summary);
    expect(invoke).toHaveBeenCalledWith("pair_remote_environment", {
      input: {
        url: "https://mac-studio.tailnet.ts.net",
        code: "rxp_code",
        name: "Mac Studio",
      },
    });
  });

  it("rejects an invalid paired environment response", async () => {
    const { protocolVersion: _protocolVersion, ...invalidSummary } = summary;
    vi.mocked(invoke).mockResolvedValue(invalidSummary);

    await expect(
      remoteEnvironmentsApi.pair("https://example.test", "code", "Example")
    ).rejects.toThrow();
  });

  it("lists empty and populated remote environment registries", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]).mockResolvedValueOnce([summary]);

    await expect(remoteEnvironmentsApi.list()).resolves.toEqual([]);
    await expect(remoteEnvironmentsApi.list()).resolves.toEqual([summary]);
    expect(invoke).toHaveBeenNthCalledWith(1, "list_remote_environments", {});
    expect(invoke).toHaveBeenNthCalledWith(2, "list_remote_environments", {});
  });

  it("rejects an invalid environment in the list response", async () => {
    vi.mocked(invoke).mockResolvedValue([{ ...summary, status: "removed" }]);

    await expect(remoteEnvironmentsApi.list()).rejects.toThrow();
  });

  it("removes an environment through the wrapped Tauri input contract", async () => {
    vi.mocked(invoke).mockResolvedValue(null);

    await expect(remoteEnvironmentsApi.remove("row-1")).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith("remove_remote_environment", {
      input: { id: "row-1" },
    });
  });

  it("loads the active environment through the scalar command contract", async () => {
    vi.mocked(invoke).mockResolvedValue("env-1");

    await expect(remoteEnvironmentsApi.getActiveEnvironment()).resolves.toBe("env-1");
    expect(invoke).toHaveBeenCalledWith("get_active_environment", {});
  });

  it("rejects a non-string active environment response", async () => {
    vi.mocked(invoke).mockResolvedValue({ id: "env-1" });

    await expect(remoteEnvironmentsApi.getActiveEnvironment()).rejects.toThrow();
  });

  it("sets the active environment through the wrapped Tauri input contract", async () => {
    vi.mocked(invoke).mockResolvedValue(null);

    await expect(remoteEnvironmentsApi.setActiveEnvironment("env-1")).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith("set_active_environment", {
      input: { id: "env-1" },
    });
  });
});
