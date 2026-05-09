import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useTaskGraphLayout, type PlanBranchNodeContext } from "./useTaskGraphLayout";
import type { TaskGraphNode } from "@/api/task-graph.types";
import { useThemeStore } from "@/stores/themeStore";

function makeNode(overrides: Partial<TaskGraphNode> = {}): TaskGraphNode {
  return {
    taskId: "merge-task",
    title: "Merge plan into main",
    description: "Auto-created merge task",
    category: "plan_merge",
    internalStatus: "merged",
    priority: 0,
    inDegree: 0,
    outDegree: 0,
    tier: 1,
    planArtifactId: "plan-1",
    sourceProposalId: null,
    executionPlanId: null,
    ...overrides,
  };
}

describe("useTaskGraphLayout", () => {
  it("hydrates plan merge node context from its plan artifact id", () => {
    const planBranchMap = new Map<string, PlanBranchNodeContext>([
      [
        "plan-1",
        {
          mergeTarget: "main",
          prNumber: 68,
          prStatus: "Merged",
          status: "merged",
        },
      ],
    ]);

    const { result } = renderHook(() =>
      useTaskGraphLayout(
        [makeNode()],
        [],
        [],
        [],
        { byPlan: false, byTier: false, showUncategorized: true },
        {},
        new Set(),
        new Set(),
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        planBranchMap,
      ),
    );

    expect(result.current.nodes[0]?.data).toMatchObject({
      mergeTarget: "main",
      prNumber: 68,
      prStatus: "Merged",
      planBranchStatus: "merged",
    });
  });

  it("hydrates plan merge node context from merge task id when plan artifact id is absent", () => {
    const planBranchMap = new Map<string, PlanBranchNodeContext>([
      [
        "merge-task",
        {
          mergeTarget: "main",
          prNumber: 68,
          prStatus: "Merged",
          status: "merged",
        },
      ],
    ]);

    const { result } = renderHook(() =>
      useTaskGraphLayout(
        [makeNode({ planArtifactId: null })],
        [],
        [],
        [],
        { byPlan: false, byTier: false, showUncategorized: true },
        {},
        new Set(),
        new Set(),
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        planBranchMap,
      ),
    );

    expect(result.current.nodes[0]?.data).toMatchObject({
      mergeTarget: "main",
      prNumber: 68,
      prStatus: "Merged",
      planBranchStatus: "merged",
    });
  });

  it("prefers merge task context over plan artifact context for merged branches", () => {
    const planBranchMap = new Map<string, PlanBranchNodeContext>([
      [
        "plan-1",
        {
          mergeTarget: "main",
          status: "active",
        },
      ],
      [
        "merge-task",
        {
          mergeTarget: "main",
          prNumber: 68,
          prStatus: "Open",
          status: "merged",
        },
      ],
    ]);

    const { result } = renderHook(() =>
      useTaskGraphLayout(
        [makeNode()],
        [],
        [],
        [],
        { byPlan: false, byTier: false, showUncategorized: true },
        {},
        new Set(),
        new Set(),
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        planBranchMap,
      ),
    );

    expect(result.current.nodes[0]?.data).toMatchObject({
      prNumber: 68,
      prStatus: "Open",
      planBranchStatus: "merged",
    });
  });

  describe("font scale invalidation", () => {
    afterEach(() => {
      useThemeStore.setState({ fontScale: "default" });
    });

    it("recomputes node dimensions when fontScale changes between renders", () => {
      useThemeStore.setState({ fontScale: "default" });

      const { result, rerender } = renderHook(() =>
        useTaskGraphLayout(
          [makeNode({ taskId: "task-a", category: "execute", planArtifactId: null })],
          [],
          [],
          [],
          { byPlan: false, byTier: false, showUncategorized: true },
          {},
        ),
      );

      const initialWidth = result.current.nodes[0]?.width;
      expect(initialWidth).toBeGreaterThan(0);

      act(() => {
        useThemeStore.setState({ fontScale: "xl" });
      });
      rerender();

      const scaledWidth = result.current.nodes[0]?.width;
      expect(scaledWidth).toBeGreaterThan(initialWidth ?? 0);
    });
  });

  it("returns empty layout when there are no graph nodes", () => {
    const { result } = renderHook(() =>
      useTaskGraphLayout(
        [],
        [],
        [],
        [],
        { byPlan: false, byTier: false, showUncategorized: true },
        {},
      ),
    );

    expect(result.current.nodes).toEqual([]);
    expect(result.current.edges).toEqual([]);
    expect(result.current.groupNodes).toEqual([]);
  });

  it("uses LR layout direction for handle positions", () => {
    const { result } = renderHook(() =>
      useTaskGraphLayout(
        [
          makeNode({ taskId: "a", category: "execute", planArtifactId: null, tier: 0 }),
          makeNode({ taskId: "b", category: "execute", planArtifactId: null, tier: 1 }),
        ],
        [{ source: "a", target: "b", isCriticalPath: false }],
        [],
        [],
        { byPlan: false, byTier: false, showUncategorized: true },
        { direction: "LR" },
      ),
    );

    expect(result.current.nodes).toHaveLength(2);
    // LR direction → sourcePosition Right ("right"), targetPosition Left ("left")
    expect(result.current.nodes[0]?.sourcePosition).toBe("right");
    expect(result.current.nodes[0]?.targetPosition).toBe("left");
  });
});
