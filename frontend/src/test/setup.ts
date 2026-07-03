/**
 * Vitest test setup file
 * This file runs before each test file
 */

import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

// Mock __UI_DEBUG__ global used by logger
(globalThis as unknown as { __UI_DEBUG__: boolean }).__UI_DEBUG__ = false;

// Mock ResizeObserver for Radix UI components (ScrollArea, etc.)
class ResizeObserverMock implements ResizeObserver {
  constructor(_callback: ResizeObserverCallback) {}

  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
}
global.ResizeObserver = ResizeObserverMock;

// Mock window.matchMedia for hooks that use media queries (e.g., usePlanBrowserLayout)
// Individual tests can override this with Object.defineProperty if they need specific breakpoints.
Object.defineProperty(window, "matchMedia", {
  writable: true,
  configurable: true,
  value: vi.fn((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

// Node 25 ships an experimental global `localStorage`/`sessionStorage` that can
// shadow jsdom's Storage and lacks its methods, crashing any test that touches
// storage (e.g. `window.localStorage.clear()`). Install a standard in-memory
// Storage only when the environment's implementation is missing/unusable, so
// CI (Node 20 + jsdom) is unaffected while local Node 25 runs stay green.
function installMemoryStorage(prop: "localStorage" | "sessionStorage") {
  const existing = (window as unknown as Record<string, Storage | undefined>)[prop];
  if (existing && typeof existing.clear === "function") {
    return;
  }
  const store = new Map<string, string>();
  const storage = {
    get length() {
      return store.size;
    },
    clear() {
      store.clear();
    },
    getItem(key: string) {
      return store.has(key) ? (store.get(key) as string) : null;
    },
    key(index: number) {
      return Array.from(store.keys())[index] ?? null;
    },
    removeItem(key: string) {
      store.delete(key);
    },
    setItem(key: string, value: string) {
      store.set(String(key), String(value));
    },
  } as Storage;
  for (const target of [window, globalThis] as object[]) {
    try {
      Object.defineProperty(target, prop, {
        configurable: true,
        writable: true,
        value: storage,
      });
    } catch {
      // Leave the existing implementation if it cannot be redefined.
    }
  }
}
installMemoryStorage("localStorage");
installMemoryStorage("sessionStorage");

// jsdom does not implement canvas rendering. Keep canvas unavailable, but avoid
// noisy "HTMLCanvasElement.getContext() not implemented" warnings in tests.
Object.defineProperty(HTMLCanvasElement.prototype, "getContext", {
  writable: true,
  configurable: true,
  value: vi.fn(() => null),
});

// Cleanup after each test case (e.g., clearing jsdom)
afterEach(() => {
  cleanup();
});

// Mock Tauri's invoke function for testing
// Default returns Promise.resolve(undefined) to match real invoke's Promise-based API
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn((filePath: string) => filePath),
  invoke: vi.fn().mockResolvedValue(undefined),
}));

// Mock Tauri's event module
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
}));

// Mock Tauri's webview drag/drop API.
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn(() => Promise.resolve(() => {})),
  }),
}));

// Mock Tauri's filesystem plugin for tests that render native-drop-aware UI.
vi.mock("@tauri-apps/plugin-fs", () => ({
  readTextFile: vi.fn(() => Promise.reject(new Error("[test] readTextFile not mocked"))),
  readFile: vi.fn(() => Promise.reject(new Error("[test] readFile not mocked"))),
  writeTextFile: vi.fn(() => Promise.resolve()),
  writeFile: vi.fn(() => Promise.resolve()),
  exists: vi.fn(() => Promise.resolve(false)),
  stat: vi.fn(() =>
    Promise.resolve({
      size: 0,
      isFile: true,
      isDirectory: false,
      isSymlink: false,
      mtimeMs: null,
      atimeMs: null,
      birthtimeMs: null,
      readonly: false,
    }),
  ),
  mkdir: vi.fn(() => Promise.resolve()),
  readDir: vi.fn(() => Promise.resolve([])),
  remove: vi.fn(() => Promise.resolve()),
  rename: vi.fn(() => Promise.resolve()),
  copyFile: vi.fn(() => Promise.resolve()),
}));

// Reset all mocks between tests
afterEach(() => {
  vi.clearAllMocks();
});
