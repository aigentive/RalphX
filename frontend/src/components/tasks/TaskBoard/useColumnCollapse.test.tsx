/**
 * Tests for useColumnCollapse hook
 */

import { describe, it, expect, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { WorkflowColumn } from "@/types/workflow";
import { useColumnCollapse } from "./useColumnCollapse";
import { useUiStore } from "@/stores/uiStore";

// Mock columns
const columns: WorkflowColumn[] = [
  { id: "draft", name: "Draft", mapsTo: "backlog" },
  { id: "ready", name: "Ready", mapsTo: "ready" },
  { id: "in_progress", name: "In Progress", mapsTo: "executing" },
  { id: "done", name: "Done", mapsTo: "approved" },
];

function makeCounts(counts: Record<string, number>): Map<string, number> {
  return new Map(Object.entries(counts));
}

describe("useColumnCollapse", () => {
  beforeEach(() => {
    // Reset uiStore collapsed state
    useUiStore.getState().setCollapsedColumns(new Set());
  });

  it("returns isCollapsed, toggleCollapse, and expandColumn", () => {
    const taskCounts = makeCounts({ draft: 3, ready: 2, in_progress: 1, done: 0 });

    const { result } = renderHook(() =>
      useColumnCollapse(columns, taskCounts, "session-1"),
    );

    expect(result.current.isCollapsed).toBeInstanceOf(Function);
    expect(result.current.toggleCollapse).toBeInstanceOf(Function);
    expect(result.current.expandColumn).toBeInstanceOf(Function);
  });

  it("keeps empty columns expanded on initial render", () => {
    const taskCounts = makeCounts({ draft: 3, ready: 0, in_progress: 1, done: 0 });

    const { result } = renderHook(() =>
      useColumnCollapse(columns, taskCounts, "session-1"),
    );

    // v29a keeps the board stable and renders empty states in place.
    expect(result.current.isCollapsed("ready")).toBe(false);
    expect(result.current.isCollapsed("done")).toBe(false);
    expect(result.current.isCollapsed("draft")).toBe(false);
    expect(result.current.isCollapsed("in_progress")).toBe(false);
  });

  it("keeps empty columns expanded on plan change", () => {
    const taskCounts = makeCounts({ draft: 3, ready: 2, in_progress: 1, done: 0 });

    const { result, rerender } = renderHook(
      ({ sessionId, counts }) =>
        useColumnCollapse(columns, counts, sessionId),
      { initialProps: { sessionId: "session-1", counts: taskCounts } },
    );

    expect(result.current.isCollapsed("done")).toBe(false);
    expect(result.current.isCollapsed("ready")).toBe(false);

    // Plan changes — now "ready" is also empty
    const newCounts = makeCounts({ draft: 1, ready: 0, in_progress: 0, done: 0 });
    rerender({ sessionId: "session-2", counts: newCounts });

    expect(result.current.isCollapsed("ready")).toBe(false);
    expect(result.current.isCollapsed("in_progress")).toBe(false);
    expect(result.current.isCollapsed("done")).toBe(false);
  });

  it("auto-expands when count transitions from 0 to N", () => {
    const initialCounts = makeCounts({ draft: 3, ready: 0, in_progress: 1, done: 0 });

    const { result, rerender } = renderHook(
      ({ counts }) => useColumnCollapse(columns, counts, "session-1"),
      { initialProps: { counts: initialCounts } },
    );

    // Empty columns start expanded, but a user can still collapse them.
    act(() => {
      result.current.toggleCollapse("ready");
    });
    expect(result.current.isCollapsed("ready")).toBe(true);

    // Tasks arrive in "ready"
    const updatedCounts = makeCounts({ draft: 3, ready: 2, in_progress: 1, done: 0 });
    rerender({ counts: updatedCounts });

    // Should auto-expand
    expect(result.current.isCollapsed("ready")).toBe(false);
  });

  it("respects user-initiated expand within same plan", () => {
    const emptyCounts = makeCounts({ draft: 3, ready: 0, in_progress: 1, done: 0 });

    const { result, rerender } = renderHook(
      ({ counts, sessionId }) =>
        useColumnCollapse(columns, counts, sessionId),
      { initialProps: { counts: emptyCounts, sessionId: "session-1" } },
    );

    act(() => {
      result.current.toggleCollapse("ready");
    });
    expect(result.current.isCollapsed("ready")).toBe(true);

    // User manually expands "ready"
    act(() => {
      result.current.toggleCollapse("ready");
    });
    expect(result.current.isCollapsed("ready")).toBe(false);

    // Re-render with same plan (counts stay same) — user expand is respected.
    rerender({ counts: emptyCounts, sessionId: "session-1" });

    expect(result.current.isCollapsed("ready")).toBe(false);
  });

  it("toggleCollapse toggles collapsed state", () => {
    const taskCounts = makeCounts({ draft: 3, ready: 0, in_progress: 1, done: 0 });

    const { result } = renderHook(() =>
      useColumnCollapse(columns, taskCounts, "session-1"),
    );

    expect(result.current.isCollapsed("ready")).toBe(false);

    // Toggle to collapsed
    act(() => {
      result.current.toggleCollapse("ready");
    });
    expect(result.current.isCollapsed("ready")).toBe(true);

    // Toggle back to expanded
    act(() => {
      result.current.toggleCollapse("ready");
    });
    expect(result.current.isCollapsed("ready")).toBe(false);
  });

  it("expandColumn expands a collapsed column", () => {
    const taskCounts = makeCounts({ draft: 3, ready: 0, in_progress: 1, done: 0 });

    const { result } = renderHook(() =>
      useColumnCollapse(columns, taskCounts, "session-1"),
    );

    act(() => {
      result.current.toggleCollapse("ready");
    });
    expect(result.current.isCollapsed("ready")).toBe(true);

    // Expand it
    act(() => {
      result.current.expandColumn("ready");
    });
    expect(result.current.isCollapsed("ready")).toBe(false);
  });

  it("does not auto-collapse columns with tasks", () => {
    const taskCounts = makeCounts({ draft: 3, ready: 2, in_progress: 1, done: 5 });

    const { result } = renderHook(() =>
      useColumnCollapse(columns, taskCounts, "session-1"),
    );

    // All columns have tasks, none should be collapsed
    expect(result.current.isCollapsed("draft")).toBe(false);
    expect(result.current.isCollapsed("ready")).toBe(false);
    expect(result.current.isCollapsed("in_progress")).toBe(false);
    expect(result.current.isCollapsed("done")).toBe(false);
  });

  it("clears manual collapse state on plan change", () => {
    const emptyCounts = makeCounts({ draft: 3, ready: 0, in_progress: 1, done: 0 });

    const { result, rerender } = renderHook(
      ({ counts, sessionId }) =>
        useColumnCollapse(columns, counts, sessionId),
      { initialProps: { counts: emptyCounts, sessionId: "session-1" } },
    );

    // User manually collapses "ready"
    act(() => {
      result.current.toggleCollapse("ready");
    });
    expect(result.current.isCollapsed("ready")).toBe(true);

    // Plan changes (different session ID) — stale collapse state is reset.
    rerender({ counts: emptyCounts, sessionId: "session-2" });

    expect(result.current.isCollapsed("ready")).toBe(false);
  });

  it("handles undefined ideationSessionId", () => {
    const taskCounts = makeCounts({ draft: 3, ready: 0, in_progress: 1, done: 0 });

    const { result } = renderHook(() =>
      useColumnCollapse(columns, taskCounts, undefined),
    );

    expect(result.current.isCollapsed("ready")).toBe(false);
    expect(result.current.isCollapsed("draft")).toBe(false);
  });

  it("does not auto-collapse columns that become empty from filter toggle", () => {
    // Start with tasks in "ready"
    const initialCounts = makeCounts({ draft: 3, ready: 2, in_progress: 1, done: 0 });

    const { result, rerender } = renderHook(
      ({ counts }) => useColumnCollapse(columns, counts, "session-1"),
      { initialProps: { counts: initialCounts } },
    );

    // "ready" starts expanded
    expect(result.current.isCollapsed("ready")).toBe(false);

    // Toggle filter causes "ready" to become empty (e.g., those tasks were all merge tasks)
    const filteredCounts = makeCounts({ draft: 3, ready: 0, in_progress: 1, done: 0 });
    rerender({ counts: filteredCounts });

    expect(result.current.isCollapsed("ready")).toBe(false);
  });

  it("auto-expands columns that gain tasks from filter toggle (0→N transition)", () => {
    // Start with "ready" empty and manually collapse it.
    const initialCounts = makeCounts({ draft: 3, ready: 0, in_progress: 1, done: 0 });

    const { result, rerender } = renderHook(
      ({ counts }) => useColumnCollapse(columns, counts, "session-1"),
      { initialProps: { counts: initialCounts } },
    );

    act(() => {
      result.current.toggleCollapse("ready");
    });
    expect(result.current.isCollapsed("ready")).toBe(true);

    // Toggle filter reveals tasks in "ready" (0→2 transition)
    const expandedCounts = makeCounts({ draft: 3, ready: 2, in_progress: 1, done: 0 });
    rerender({ counts: expandedCounts });

    // Auto-expand should kick in
    expect(result.current.isCollapsed("ready")).toBe(false);
  });

  it("ignores manual collapse for columns with tasks", () => {
    const taskCounts = makeCounts({ draft: 3, ready: 2, in_progress: 1, done: 0 });

    const { result } = renderHook(() =>
      useColumnCollapse(columns, taskCounts, "session-1"),
    );

    act(() => {
      result.current.toggleCollapse("ready");
    });

    expect(result.current.isCollapsed("ready")).toBe(false);
  });
});
