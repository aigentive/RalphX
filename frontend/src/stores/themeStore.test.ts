import { readFileSync } from "node:fs";
import { join } from "node:path";

import { beforeEach, describe, expect, it, vi } from "vitest";

const indexHtmlPath = join(process.cwd(), "index.html");

function installMatchMedia({
  prefersContrast = false,
  prefersLight = false,
}: {
  prefersContrast?: boolean;
  prefersLight?: boolean;
} = {}) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn((query: string) => ({
      matches: query.includes("prefers-contrast: more")
        ? prefersContrast
        : query.includes("prefers-color-scheme: light")
          ? prefersLight
          : false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
}

async function importFreshThemeStore() {
  vi.resetModules();
  return await import("./themeStore");
}

function runInlineThemeBootstrap() {
  const html = readFileSync(indexHtmlPath, "utf8");
  const match = html.match(/<body>\s*<script>([\s\S]*?)<\/script>/);
  const script = match?.[1];
  expect(script).toBeTruthy();
  new Function(script as string)();
}

describe("themeStore", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.classList.remove("dark");
    installMatchMedia();
  });

  it("defaults to dark when no theme is stored, even if the OS prefers light", async () => {
    installMatchMedia({ prefersContrast: true, prefersLight: true });

    const { syncThemeAttributesFromStore, useThemeStore } = await importFreshThemeStore();

    expect(useThemeStore.getState().theme).toBe("dark");
    syncThemeAttributesFromStore();
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(localStorage.getItem("ralphx-theme")).toBeNull();
  });

  it("preserves an explicit stored light preference", async () => {
    localStorage.setItem("ralphx-theme", "light");
    installMatchMedia({ prefersContrast: true });

    const { syncThemeAttributesFromStore, useThemeStore } = await importFreshThemeStore();

    expect(useThemeStore.getState().theme).toBe("light");
    syncThemeAttributesFromStore();
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(localStorage.getItem("ralphx-theme")).toBe("light");
  });

  it("migrates legacy theme values to the dark default", async () => {
    localStorage.setItem("ralphx-theme", "system");

    const { migrateThemePreference, useThemeStore } = await importFreshThemeStore();

    expect(migrateThemePreference("system")).toBeNull();
    expect(useThemeStore.getState().theme).toBe("dark");
    expect(localStorage.getItem("ralphx-theme")).toBeNull();
  });

  it("uses the same dark default in the pre-hydration bootstrap", () => {
    installMatchMedia({ prefersContrast: true, prefersLight: true });

    runInlineThemeBootstrap();

    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(localStorage.getItem("ralphx-theme")).toBeNull();
  });

  it("keeps stored light during pre-hydration bootstrap", () => {
    localStorage.setItem("ralphx-theme", "light");

    runInlineThemeBootstrap();

    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(localStorage.getItem("ralphx-theme")).toBe("light");
  });
});
