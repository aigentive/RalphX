import { describe, expect, it, vi } from "vitest";

const { openUrlMock, toastErrorMock } = vi.hoisted(() => ({
  openUrlMock: vi.fn(),
  toastErrorMock: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));
vi.mock("sonner", () => ({ toast: { error: toastErrorMock } }));

import { openExternalUrl } from "./open-external";

describe("openExternalUrl", () => {
  it("opens http urls through the app opener", async () => {
    openUrlMock.mockReset().mockResolvedValueOnce(undefined);
    toastErrorMock.mockReset();

    await openExternalUrl("https://github.com/x/y/pull/42");

    expect(openUrlMock).toHaveBeenCalledWith("https://github.com/x/y/pull/42");
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("rejects non-http urls before invoking the opener", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    openUrlMock.mockReset();
    toastErrorMock.mockReset();

    await openExternalUrl("file:///Users/example/.ssh/id_rsa");

    expect(openUrlMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalledWith(
      "Blocked external URL with unsupported scheme",
      { url: "file:///Users/example/.ssh/id_rsa" },
    );

    warn.mockRestore();
  });

  it("surfaces a toast instead of throwing when the open fails", async () => {
    openUrlMock.mockReset().mockRejectedValueOnce(new Error("opener unavailable"));
    toastErrorMock.mockReset();

    await expect(openExternalUrl("https://github.com/x/y/pull/42")).resolves.toBeUndefined();

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
  });
});
