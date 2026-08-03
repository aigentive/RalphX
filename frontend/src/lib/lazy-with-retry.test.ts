import type { ComponentType } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { isModuleLoadError } from "./module-load-error";
import { lazyWithRetry, withRetry } from "./lazy-with-retry";

const TestComponent: ComponentType = () => null;
const moduleLoadError = new Error("Failed to fetch dynamically imported module");

describe("withRetry", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("retries a module-load failure once and resolves the second factory result", async () => {
    vi.useFakeTimers();
    const module = { default: TestComponent };
    const factory = vi
      .fn<() => Promise<{ default: ComponentType }>>()
      .mockRejectedValueOnce(moduleLoadError)
      .mockResolvedValueOnce(module);

    const result = withRetry(factory)();

    await vi.runAllTimersAsync();

    await expect(result).resolves.toBe(module);
    expect(factory).toHaveBeenCalledTimes(2);
  });

  it("rethrows a second module-load failure after retrying once", async () => {
    vi.useFakeTimers();
    const retryError = new Error("Importing a module script failed");
    const factory = vi
      .fn<() => Promise<{ default: ComponentType }>>()
      .mockRejectedValueOnce(moduleLoadError)
      .mockRejectedValueOnce(retryError);

    const result = withRetry(factory)();
    const rejection = expect(result).rejects.toBe(retryError);

    await vi.runAllTimersAsync();

    await rejection;
    expect(isModuleLoadError(retryError)).toBe(true);
    expect(factory).toHaveBeenCalledTimes(2);
  });

  it("rethrows a non-module-load failure without retrying", async () => {
    const error = new Error("Component initialization failed");
    const factory = vi
      .fn<() => Promise<{ default: ComponentType }>>()
      .mockRejectedValueOnce(error);

    await expect(withRetry(factory)()).rejects.toBe(error);
    expect(factory).toHaveBeenCalledTimes(1);
  });

  it("resolves a first-attempt success without retrying", async () => {
    const module = { default: TestComponent };
    const factory = vi
      .fn<() => Promise<{ default: ComponentType }>>()
      .mockResolvedValueOnce(module);

    await expect(withRetry(factory)()).resolves.toBe(module);
    expect(factory).toHaveBeenCalledTimes(1);
  });
});

describe("lazyWithRetry", () => {
  it("returns a React.lazy-shaped component", () => {
    const component = lazyWithRetry(() =>
      Promise.resolve({ default: TestComponent }),
    );

    expect(component).toHaveProperty("$$typeof");
  });
});
