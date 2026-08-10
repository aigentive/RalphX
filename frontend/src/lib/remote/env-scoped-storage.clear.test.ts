// PR 2.5 / P-27: unpairing must not leave the environment's UI state behind.

import { beforeEach, describe, expect, it } from "vitest";

import { LOCAL_ENVIRONMENT_ID } from "./active-environment";
import { clearEnvScopedStorage } from "./env-scoped-storage";
import { STORE_ISOLATION_INVENTORY } from "./store-isolation-inventory";

const PERSISTED_NAMES = STORE_ISOLATION_INVENTORY.flatMap((entry) =>
  entry.persisted ? [entry.persisted.storageName] : [],
);

describe("clearEnvScopedStorage", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("removes every env-scoped slice for the removed environment", () => {
    for (const name of PERSISTED_NAMES) {
      localStorage.setItem(name, JSON.stringify({ state: {}, version: 0 }));
      localStorage.setItem(`${name}:env-a`, JSON.stringify({ state: {}, version: 0 }));
      localStorage.setItem(`${name}:env-b`, JSON.stringify({ state: {}, version: 0 }));
    }

    clearEnvScopedStorage("env-a");

    for (const name of PERSISTED_NAMES) {
      expect(localStorage.getItem(`${name}:env-a`)).toBeNull();
      // Another environment's state is not collateral damage.
      expect(localStorage.getItem(`${name}:env-b`)).not.toBeNull();
      // Nor is the shared/local slice.
      expect(localStorage.getItem(name)).not.toBeNull();
    }
  });

  it("refuses to clear the local environment", () => {
    const [name] = PERSISTED_NAMES;
    localStorage.setItem(name, JSON.stringify({ state: {}, version: 0 }));

    clearEnvScopedStorage(LOCAL_ENVIRONMENT_ID);

    // Local state is this Mac's own; a remote unpair has no claim on it.
    expect(localStorage.getItem(name)).not.toBeNull();
  });

  it("is a no-op for an environment that never persisted anything", () => {
    expect(() => clearEnvScopedStorage("never-used")).not.toThrow();
    expect(localStorage.length).toBe(0);
  });

  it("is idempotent — a repeated staged removal clears the same keys twice safely", () => {
    const [name] = PERSISTED_NAMES;
    localStorage.setItem(`${name}:env-a`, JSON.stringify({ state: {}, version: 0 }));

    clearEnvScopedStorage("env-a");
    expect(() => clearEnvScopedStorage("env-a")).not.toThrow();
    expect(localStorage.getItem(`${name}:env-a`)).toBeNull();
  });
});
