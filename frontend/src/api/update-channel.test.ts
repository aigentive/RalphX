import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { updateChannelApi } from "./update-channel";

describe("updateChannelApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads the persisted update channel through the scalar command contract", async () => {
    vi.mocked(invoke).mockResolvedValue("nightly");

    await expect(updateChannelApi.get()).resolves.toBe("nightly");
    expect(invoke).toHaveBeenCalledWith("get_update_channel", {});
  });

  it("saves a selected channel with the camel-case Tauri argument", async () => {
    vi.mocked(invoke).mockResolvedValue("stable");

    await expect(updateChannelApi.set("stable")).resolves.toBe("stable");
    expect(invoke).toHaveBeenCalledWith("set_update_channel", {
      updateChannel: "stable",
    });
  });

  it("rejects an invalid backend channel instead of widening the frontend contract", async () => {
    vi.mocked(invoke).mockResolvedValue("preview");

    await expect(updateChannelApi.get()).rejects.toThrow();
  });

  it("rejects an invalid persisted save response instead of widening the scalar contract", async () => {
    vi.mocked(invoke).mockResolvedValue("preview");

    await expect(updateChannelApi.set("nightly")).rejects.toThrow();
  });
});
