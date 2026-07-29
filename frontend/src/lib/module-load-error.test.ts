import { describe, expect, it } from "vitest";
import { isModuleLoadError } from "./module-load-error";

describe("isModuleLoadError", () => {
  it.each([
    "Importing a module script failed",
    "Failed to fetch dynamically imported module",
    "error loading dynamically imported module",
    "Unable to preload CSS",
  ])("recognizes dynamic-import transport failure messages: %s", (message) => {
    expect(isModuleLoadError(message)).toBe(true);
    expect(isModuleLoadError(new Error(message.toUpperCase()))).toBe(true);
    expect(isModuleLoadError({ message })).toBe(true);
  });

  it.each([
    new TypeError("Cannot read properties of undefined"),
    new Error("Module initialization failed"),
    "",
    null,
    undefined,
    42,
    {},
    { message: 42 },
    { message: "" },
  ])("rejects unrelated or message-less values: %o", (error) => {
    expect(isModuleLoadError(error)).toBe(false);
  });
});
