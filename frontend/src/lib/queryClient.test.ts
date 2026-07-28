import { afterEach, describe, expect, it } from "vitest";

import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";
import { getQueryClient, resetQueryClient } from "./queryClient";

afterEach(() => {
  resetQueryClient();
  resetTransportEnvironmentId();
});

describe("getQueryClient", () => {
  it("retains one isolated client and cache per environment", () => {
    const environmentA = getQueryClient("env-a");
    const environmentAAgain = getQueryClient("env-a");
    const environmentB = getQueryClient("env-b");

    environmentA.setQueryData(["shared-key"], "environment-a-data");

    expect(environmentAAgain).toBe(environmentA);
    expect(environmentB).not.toBe(environmentA);
    expect(environmentA.getQueryData(["shared-key"])).toBe("environment-a-data");
    expect(environmentB.getQueryData(["shared-key"])).toBeUndefined();
  });

  it("uses the transport environment for its default argument", () => {
    const localClient = getQueryClient();

    setTransportEnvironmentId("env-b");

    expect(getQueryClient()).toBe(getQueryClient("env-b"));
    expect(getQueryClient()).not.toBe(localClient);
  });

  it("drops every retained client when reset", () => {
    const environmentA = getQueryClient("env-a");
    const environmentB = getQueryClient("env-b");

    resetQueryClient();

    expect(getQueryClient("env-a")).not.toBe(environmentA);
    expect(getQueryClient("env-b")).not.toBe(environmentB);
  });
});
