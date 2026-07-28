import { beforeEach, describe, expect, it } from "vitest";

import { resetTransportEnvironmentId, setTransportEnvironmentId } from "./active-environment";
import { createEnvScopedStorage, runSuppressed } from "./env-scoped-storage";

const name = "ralphx-project-store";
const storage = createEnvScopedStorage<{ activeProjectId: string | null }>(name);

beforeEach(() => {
  localStorage.clear();
  resetTransportEnvironmentId();
});

describe("env-scoped persistence", () => {
  it("keeps local byte-compatible under only the legacy key", () => {
    const value = { state: { activeProjectId: "proj-local" }, version: 3 };
    storage.setItem(name, value);
    expect(storage.getItem(name)).toEqual(value);
    expect(localStorage.getItem(`${name}:local`)).toBeNull();
  });

  it("never falls back to local env fields for a remote read", () => {
    localStorage.setItem(
      name,
      JSON.stringify({ state: { activeProjectId: "proj-local" }, version: 2 }),
    );
    setTransportEnvironmentId("env-b");
    expect(storage.getItem(name)?.state).toEqual({});
    localStorage.setItem(
      `${name}:env-b`,
      JSON.stringify({ state: { activeProjectId: "proj-b" }, version: 2 }),
    );
    expect(storage.getItem(name)?.state).toEqual({ activeProjectId: "proj-b" });
  });

  it("splits remote writes without clobbering local env fields or restamping their version", () => {
    localStorage.setItem(
      name,
      JSON.stringify({ state: { activeProjectId: "proj-local" }, version: 1 }),
    );
    setTransportEnvironmentId("env-b");
    storage.setItem(name, { state: { activeProjectId: "proj-b" }, version: 4 });
    expect(JSON.parse(localStorage.getItem(name) ?? "null")).toEqual({
      state: { activeProjectId: "proj-local" },
      version: 1,
    });
    expect(JSON.parse(localStorage.getItem(`${name}:env-b`) ?? "null")).toEqual({
      state: { activeProjectId: "proj-b" },
      version: 4,
    });

    setTransportEnvironmentId("local");
    storage.setItem(name, { state: { activeProjectId: "proj-local-current" }, version: 5 });
    expect(JSON.parse(localStorage.getItem(name) ?? "null")).toEqual({
      state: { activeProjectId: "proj-local-current" },
      version: 5,
    });
  });

  it("suppresses nested writes and releases after exceptions", () => {
    expect(() =>
      runSuppressed(() =>
        runSuppressed(() => {
          storage.setItem(name, { state: { activeProjectId: "blocked" }, version: 1 });
          throw new Error("boom");
        }),
      ),
    ).toThrow("boom");
    expect(localStorage.getItem(name)).toBeNull();
    storage.setItem(name, { state: { activeProjectId: "allowed" }, version: 1 });
    expect(localStorage.getItem(name)).toContain("allowed");
  });

  it("uses the oldest slice version on merged remote reads", () => {
    localStorage.setItem(name, JSON.stringify({ state: {}, version: 7 }));
    localStorage.setItem(
      `${name}:env-b`,
      JSON.stringify({ state: { activeProjectId: "b" }, version: 3 }),
    );
    setTransportEnvironmentId("env-b");
    expect(storage.getItem(name)?.version).toBe(3);
  });
});
