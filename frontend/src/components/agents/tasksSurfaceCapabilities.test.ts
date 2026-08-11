import { describe, expect, it } from "vitest";

import { deriveTasksSurfaceCapabilities } from "./tasksSurfaceCapabilities";

describe("deriveTasksSurfaceCapabilities", () => {
  it("keeps persisted history visible and read-only while Tasks are disabled", () => {
    expect(
      deriveTasksSurfaceCapabilities({
        featureState: "disabled",
        hasHistory: true,
      }),
    ).toEqual({
      hasHistory: true,
      isReadOnly: true,
      canProgress: false,
      canQuiesce: true,
      reason: "tasks_disabled",
    });
  });

  it("fails visible and read-only when history availability cannot be read", () => {
    const capabilities = deriveTasksSurfaceCapabilities({
      featureState: "enabled",
      hasHistory: false,
      historyUnavailable: true,
    });

    expect(capabilities.hasHistory).toBe(true);
    expect(capabilities.isReadOnly).toBe(true);
    expect(capabilities.canProgress).toBe(false);
    expect(capabilities.reason).toBe("history_unavailable");
  });

  it("restores progress without auto-changing history when Tasks are enabled", () => {
    expect(
      deriveTasksSurfaceCapabilities({
        featureState: "enabled",
        hasHistory: true,
      }),
    ).toMatchObject({
      hasHistory: true,
      isReadOnly: false,
      canProgress: true,
      canQuiesce: true,
      reason: null,
    });
  });
});
