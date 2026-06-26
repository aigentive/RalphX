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

function fireKeyDownWithShift(key: string) {
  const event = new KeyboardEvent("keydown", { key, metaKey: true, shiftKey: true, bubbles: true });
  window.dispatchEvent(event);
}

function makeProps(overrides: Partial<Parameters<typeof useAppKeyboardShortcuts>[0]> = {}) {
  return {
    currentView: "kanban" as const,
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
    ["2", "ideation"],
    ["3", "graph"],
    ["4", "kanban"],
    ["5", "insights"],
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

    fireKeyDown("2");
    expect(setCurrentView).toHaveBeenCalledWith("ideation");
  });

  it("⌘2 is a no-op when featureFlags.ideationPage is false", () => {
    const setCurrentView = vi.fn();
    const flags: FeatureFlags = { activityPage: true, extensibilityPage: true, ideationPage: false, battleMode: true, teamMode: false, atlassianOauth: false };

    renderHook(() =>
      useAppKeyboardShortcuts(makeProps({ currentView: "agents", setCurrentView, featureFlags: flags }))
    );

    fireKeyDown("2");
    expect(setCurrentView).not.toHaveBeenCalled();
  });

  it("⌘K is unassigned after page chat removal", () => {
    const setCurrentView = vi.fn();

    renderHook(() =>
      useAppKeyboardShortcuts(makeProps({ currentView: "graph", setCurrentView }))
    );

    fireKeyDown("k");
    expect(setCurrentView).not.toHaveBeenCalled();
  });

  // ── CMD+SHIFT+B battle mode shortcut ──────────────────────────────────────

  it("⌘⇧B is a no-op when featureFlags.battleMode is false", () => {
    const onBattleModeToggle = vi.fn();
    const flags: FeatureFlags = { activityPage: true, extensibilityPage: true, ideationPage: true, battleMode: false, teamMode: false, atlassianOauth: false };

    renderHook(() =>
      useAppKeyboardShortcuts(makeProps({ currentView: "graph", onBattleModeToggle, featureFlags: flags }))
    );

    fireKeyDownWithShift("b");
    expect(onBattleModeToggle).not.toHaveBeenCalled();
  });

  it("⌘⇧B calls onBattleModeToggle when flag is enabled and currentView is graph", () => {
    const onBattleModeToggle = vi.fn();
    const flags: FeatureFlags = { activityPage: true, extensibilityPage: true, ideationPage: true, battleMode: true, teamMode: false, atlassianOauth: false };

    renderHook(() =>
      useAppKeyboardShortcuts(makeProps({ currentView: "graph", onBattleModeToggle, featureFlags: flags }))
    );

    fireKeyDownWithShift("b");
    expect(onBattleModeToggle).toHaveBeenCalledOnce();
  });

  it("⌘⇧B is a no-op when flag is enabled but currentView is not graph", () => {
    const onBattleModeToggle = vi.fn();
    const flags: FeatureFlags = { activityPage: true, extensibilityPage: true, ideationPage: true, battleMode: true, teamMode: false, atlassianOauth: false };

    renderHook(() =>
      useAppKeyboardShortcuts(makeProps({ currentView: "kanban", onBattleModeToggle, featureFlags: flags }))
    );

    fireKeyDownWithShift("b");
    expect(onBattleModeToggle).not.toHaveBeenCalled();
  });
});
