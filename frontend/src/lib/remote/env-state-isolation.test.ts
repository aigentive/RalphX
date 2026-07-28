import { create } from "zustand";
import { persist } from "zustand/middleware";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createEnvScopedStorage } from "./env-scoped-storage";
import {
  onEnvironmentSwitched,
  registerEnvIsolatedStore,
  resetEnvStateIsolationForTests,
} from "./env-state-isolation";

beforeEach(() => {
  localStorage.clear();
  resetEnvStateIsolationForTests();
});

describe("environment state isolation funnel", () => {
  it("resets before rehydrate and re-registration replaces", () => {
    const order: string[] = [];
    registerEnvIsolatedStore({ name: "store", reset: () => order.push("old") });
    registerEnvIsolatedStore({ name: "store", reset: () => order.push("reset"), rehydrate: () => order.push("rehydrate") });
    onEnvironmentSwitched();
    expect(order).toEqual(["reset", "rehydrate"]);
  });

  it("supports full and fields-only resets while global stores stay untouched", () => {
    const full = create<{ count: number; increment: () => void }>((set) => ({ count: 0, increment: () => set((state) => ({ count: state.count + 1 })) }));
    const mixed = create<{ env: string; global: string }>(() => ({ env: "initial", global: "global" }));
    const untouched = vi.fn();
    full.setState({ count: 9 });
    mixed.setState({ env: "changed", global: "kept" });
    registerEnvIsolatedStore({ name: "full", reset: () => full.setState(full.getInitialState(), true) });
    registerEnvIsolatedStore({ name: "mixed", reset: () => mixed.setState({ env: mixed.getInitialState().env }) });
    onEnvironmentSwitched();
    expect(full.getState().count).toBe(0);
    full.getState().increment();
    expect(full.getState().count).toBe(1);
    expect(mixed.getState()).toEqual({ env: "initial", global: "kept" });
    expect(untouched).not.toHaveBeenCalled();
  });

  it("rehydrates synchronous localStorage before returning", () => {
    const store = create<{ activeProjectId: string | null }>()(
      persist(() => ({ activeProjectId: null }), { name: "ralphx-project-store", storage: createEnvScopedStorage("ralphx-project-store") }),
    );
    localStorage.setItem("ralphx-project-store", JSON.stringify({ state: { activeProjectId: "restored" }, version: 0 }));
    registerEnvIsolatedStore({ name: "persisted", reset: () => store.setState(store.getInitialState(), true), rehydrate: () => { void store.persist.rehydrate(); } });
    onEnvironmentSwitched();
    expect(store.getState().activeProjectId).toBe("restored");
  });
});
