import { afterEach, describe, expect, it, vi } from "vitest";

import { safelyUnlistenTauri } from "./tauri-listener-cleanup";

describe("safelyUnlistenTauri", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("ignores missing and non-promise cleanup results", () => {
    const returnsNothing = vi.fn();
    const returnsNull = vi.fn(() => null as unknown as void);

    expect(() => safelyUnlistenTauri(undefined, "missing listener")).not.toThrow();
    expect(() => safelyUnlistenTauri(returnsNothing, "sync listener")).not.toThrow();
    expect(() => safelyUnlistenTauri(returnsNull, "null listener")).not.toThrow();

    expect(returnsNothing).toHaveBeenCalledTimes(1);
    expect(returnsNull).toHaveBeenCalledTimes(1);
  });

  it("logs asynchronous cleanup failures without throwing", async () => {
    const cleanupError = new TypeError("listener already removed");
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => undefined);

    safelyUnlistenTauri(() => Promise.reject(cleanupError), "async listener");
    await Promise.resolve();

    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining("Failed to unlisten async listener"),
      cleanupError,
    );
  });
});
