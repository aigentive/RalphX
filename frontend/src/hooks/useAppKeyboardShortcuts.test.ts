/**
 * Tests for useAppKeyboardShortcuts — main nav shortcuts and shell actions
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { useAppKeyboardShortcuts } from "./useAppKeyboardShortcuts";
import type { FeatureFlags } from "@/types/feature-flags";

// Mock tauri global shortcut plugin
vi.mock("@tauri-apps/plugin-global-shortcut", () => ({
  register: vi.fn(() => Promise.resolve()),
  unregister: vi.fn(() => Promise.resolve()),
}));

function fireKeyDown(key: string, metaKey = true) {
  const event = new KeyboardEvent("keydown", { key, metaKey, bubbles: true });
  window.dispatchEvent(event);
}

function makeProps(overrides: Partial<Parameters<typeof useAppKeyboardShortcuts>[0]> = {}) {
  return {
    currentView: "agents" as const,
    setCurrentView: vi.fn(),
    ...overrides,
  };
}

describe("useAppKeyboardShortcuts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it.each([
    ["1", "agents"],
    ["2", "automations"],
    ["3", "insights"],
  ] as const)("⌘%s navigates to %s", (key, view) => {
    const setCurrentView = vi.fn();

    renderHook(() =>
      useAppKeyboardShortcuts(makeProps({ setCurrentView }))
    );

    fireKeyDown(key);
    expect(setCurrentView).toHaveBeenCalledWith(view);
  });

  it("still handles main nav shortcuts from Agents view", () => {
    const setCurrentView = vi.fn();

    renderHook(() =>
      useAppKeyboardShortcuts(makeProps({ currentView: "agents", setCurrentView }))
    );

    fireKeyDown("3");
    expect(setCurrentView).toHaveBeenCalledWith("insights");
  });

  it("⌘2 is a no-op when the Automations page is disabled", () => {
    const setCurrentView = vi.fn();
    const flags: FeatureFlags = { activityPage: true, extensibilityPage: true, automationsPage: false, atlassianOauth: false };

    renderHook(() =>
      useAppKeyboardShortcuts(makeProps({ currentView: "agents", setCurrentView, featureFlags: flags }))
    );

    fireKeyDown("2");
    expect(setCurrentView).not.toHaveBeenCalled();
  });

  it("⌘K is unassigned after page chat removal", () => {
    const setCurrentView = vi.fn();

    renderHook(() =>
      useAppKeyboardShortcuts(makeProps({ currentView: "agents", setCurrentView }))
    );

    fireKeyDown("k");
    expect(setCurrentView).not.toHaveBeenCalled();
  });

});
