import { describe, expect, it, vi } from "vitest";

const { openUrlMock, toastErrorMock } = vi.hoisted(() => ({
  openUrlMock: vi.fn(),
  toastErrorMock: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));
vi.mock("sonner", () => ({ toast: { error: toastErrorMock } }));

import { openExternalTicketUrl } from "./ticketing-open-external";

describe("openExternalTicketUrl", () => {
  it("opens the url through the app opener", async () => {
    openUrlMock.mockClear();
    toastErrorMock.mockClear();
    openUrlMock.mockResolvedValueOnce(undefined);

    await openExternalTicketUrl("https://github.com/x/y/pull/42");

    expect(openUrlMock).toHaveBeenCalledWith("https://github.com/x/y/pull/42");
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("surfaces a toast instead of throwing when the open fails", async () => {
    openUrlMock.mockClear();
    toastErrorMock.mockClear();
    openUrlMock.mockRejectedValueOnce(new Error("opener unavailable"));

    await expect(
      openExternalTicketUrl("https://github.com/x/y/pull/42"),
    ).resolves.toBeUndefined();

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
  });
});
