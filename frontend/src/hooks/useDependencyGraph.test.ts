/**
 * useDependencyGraph hook tests
 *
 * Tests for useDependencyGraph and useDependencyMutations hooks
 * using TanStack Query with mocked API.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import {
  useDependencyGraph,
  useDependencyMutations,
  useDependencyTiers,
  computeDependencyTiers,
  getDependencyReason,
  dependencyKeys,
} from "./useDependencyGraph";
import { ideationApi } from "@/api/ideation";
import type { DependencyGraphResponse } from "@/api/ideation";

// Mock the ideation API
vi.mock("@/api/ideation", () => ({
  ideationApi: {
    dependencies: {
      analyze: vi.fn(),
      add: vi.fn(),
      remove: vi.fn(),
    },
  },
}));

// Create mock data
const mockGraph: DependencyGraphResponse = {
  nodes: [
    { proposalId: "proposal-1", title: "Setup database", inDegree: 0, outDegree: 2 },
    { proposalId: "proposal-2", title: "Create API", inDegree: 1, outDegree: 1 },
    { proposalId: "proposal-3", title: "Build UI", inDegree: 1, outDegree: 0 },
  ],
  edges: [
    { from: "proposal-1", to: "proposal-2", reason: "API needs database schema" },
    { from: "proposal-2", to: "proposal-3", reason: "UI requires API endpoints" },
  ],
  criticalPath: ["proposal-1", "proposal-2", "proposal-3"],
  hasCycles: false,
  cycles: null,
};

const mockGraphWithCycles: DependencyGraphResponse = {
  nodes: [
    { proposalId: "proposal-1", title: "Task A", inDegree: 1, outDegree: 1 },
    { proposalId: "proposal-2", title: "Task B", inDegree: 1, outDegree: 1 },
  ],
  edges: [
    { from: "proposal-1", to: "proposal-2" },
    { from: "proposal-2", to: "proposal-1" },
  ],
  criticalPath: [],
  hasCycles: true,
  cycles: [["proposal-1", "proposal-2", "proposal-1"]],
};

const emptyGraph: DependencyGraphResponse = {
  nodes: [],
  edges: [],
  criticalPath: [],
  hasCycles: false,
  cycles: null,
};

// Test wrapper with QueryClientProvider
function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
      },
    },
  });

  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("dependencyKeys", () => {
  it("should generate correct key for all", () => {
    expect(dependencyKeys.all).toEqual(["dependencies"]);
  });

  it("should generate correct key for graphs", () => {
    expect(dependencyKeys.graphs()).toEqual(["dependencies", "graph"]);
  });

  it("should generate correct key for graph by session", () => {
    expect(dependencyKeys.graph("session-1")).toEqual([
      "dependencies",
      "graph",
      "session-1",
    ]);
  });
});

describe("useDependencyGraph", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should fetch dependency graph for session", async () => {
    vi.mocked(ideationApi.dependencies.analyze).mockResolvedValueOnce(mockGraph);

    const { result } = renderHook(() => useDependencyGraph("session-1"), {
      wrapper: createWrapper(),
    });

    expect(result.current.isLoading).toBe(true);

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual(mockGraph);
    expect(ideationApi.dependencies.analyze).toHaveBeenCalledWith("session-1");
  });

  it("should return empty graph for session with no proposals", async () => {
    vi.mocked(ideationApi.dependencies.analyze).mockResolvedValueOnce(emptyGraph);

    const { result } = renderHook(() => useDependencyGraph("session-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual(emptyGraph);
    expect(result.current.data?.hasCycles).toBe(false);
    expect(result.current.data?.nodes).toHaveLength(0);
  });

  it("should detect cycles in graph", async () => {
    vi.mocked(ideationApi.dependencies.analyze).mockResolvedValueOnce(mockGraphWithCycles);

    const { result } = renderHook(() => useDependencyGraph("session-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data?.hasCycles).toBe(true);
    expect(result.current.data?.cycles).toHaveLength(1);
  });

  it("should return critical path", async () => {
    vi.mocked(ideationApi.dependencies.analyze).mockResolvedValueOnce(mockGraph);

    const { result } = renderHook(() => useDependencyGraph("session-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data?.criticalPath).toEqual([
      "proposal-1",
      "proposal-2",
      "proposal-3",
    ]);
  });

  it("should handle fetch error", async () => {
    const error = new Error("Failed to analyze dependencies");
    vi.mocked(ideationApi.dependencies.analyze).mockRejectedValueOnce(error);

    const { result } = renderHook(() => useDependencyGraph("session-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(result.current.error).toEqual(error);
  });

  it("should not fetch when sessionId is empty", async () => {
    const { result } = renderHook(() => useDependencyGraph(""), {
      wrapper: createWrapper(),
    });

    expect(result.current.isFetching).toBe(false);
    expect(ideationApi.dependencies.analyze).not.toHaveBeenCalled();
  });
});

describe("useDependencyMutations", () => {
  describe("removeDependency", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    afterEach(() => {
      vi.resetAllMocks();
    });

    it("should remove a dependency successfully", async () => {
      vi.mocked(ideationApi.dependencies.remove).mockResolvedValueOnce(undefined);

      const { result } = renderHook(() => useDependencyMutations(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.removeDependency.mutateAsync({
          proposalId: "proposal-2",
          dependsOnId: "proposal-1",
        });
      });

      expect(ideationApi.dependencies.remove).toHaveBeenCalledWith(
        "proposal-2",
        "proposal-1"
      );
    });

    it("should handle remove dependency error", async () => {
      const error = new Error("Failed to remove dependency");
      vi.mocked(ideationApi.dependencies.remove).mockRejectedValueOnce(error);

      const { result } = renderHook(() => useDependencyMutations(), {
        wrapper: createWrapper(),
      });

      await expect(
        act(async () => {
          await result.current.removeDependency.mutateAsync({
            proposalId: "proposal-2",
            dependsOnId: "proposal-1",
          });
        })
      ).rejects.toThrow("Failed to remove dependency");
    });
  });
});

describe("computeDependencyTiers", () => {
  it("should return empty tiers for null graph", () => {
    const result = computeDependencyTiers(null);

    expect(result.tierMap.size).toBe(0);
    expect(result.maxTier).toBe(0);
    expect(result.tierGroups.size).toBe(0);
  });

  it("should return empty tiers for undefined graph", () => {
    const result = computeDependencyTiers(undefined);

    expect(result.tierMap.size).toBe(0);
    expect(result.maxTier).toBe(0);
    expect(result.tierGroups.size).toBe(0);
  });

  it("should return empty tiers for empty graph", () => {
    const result = computeDependencyTiers(emptyGraph);

    expect(result.tierMap.size).toBe(0);
    expect(result.maxTier).toBe(0);
    expect(result.tierGroups.size).toBe(0);
  });

  it("should assign tier 0 to nodes with no dependencies", () => {
    // Graph: A (independent)
    const graph: DependencyGraphResponse = {
      nodes: [
        { proposalId: "A", title: "Task A", inDegree: 0, outDegree: 0 },
      ],
      edges: [],
      criticalPath: [],
      hasCycles: false,
      cycles: null,
    };

    const result = computeDependencyTiers(graph);

    expect(result.tierMap.get("A")).toBe(0);
    expect(result.maxTier).toBe(0);
    expect(result.tierGroups.get(0)).toEqual(["A"]);
  });

  it("should compute tiers for linear dependency chain", () => {
    // Graph edges use "from depends on to" semantics
    // mockGraph edges: proposal-1 depends on proposal-2, proposal-2 depends on proposal-3
    // Expected tiers: proposal-3=0, proposal-2=1, proposal-1=2
    const result = computeDependencyTiers(mockGraph);

    expect(result.tierMap.get("proposal-1")).toBe(2); // Setup database
    expect(result.tierMap.get("proposal-2")).toBe(1); // Create API
    expect(result.tierMap.get("proposal-3")).toBe(0); // Build UI
    expect(result.maxTier).toBe(2);
  });

  it("should group multiple independent proposals in tier 0", () => {
    // Graph: A, B (independent) → C depends on both
    // Edge semantics: from → to means "to depends on from"
    const graph: DependencyGraphResponse = {
      nodes: [
        { proposalId: "A", title: "Task A", inDegree: 0, outDegree: 1 },
        { proposalId: "B", title: "Task B", inDegree: 0, outDegree: 1 },
        { proposalId: "C", title: "Task C", inDegree: 2, outDegree: 0 },
      ],
      edges: [
        { from: "A", to: "C" }, // A depends on C
        { from: "B", to: "C" }, // B depends on C
      ],
      criticalPath: ["A", "C"],
      hasCycles: false,
      cycles: null,
    };

    const result = computeDependencyTiers(graph);

    expect(result.tierMap.get("C")).toBe(0);
    expect(result.tierMap.get("A")).toBe(1);
    expect(result.tierMap.get("B")).toBe(1);
    expect(result.maxTier).toBe(1);
    expect(result.tierGroups.get(0)).toEqual(["C"]);
    expect(result.tierGroups.get(1)).toContain("A");
    expect(result.tierGroups.get(1)).toContain("B");
    expect(result.tierGroups.get(1)?.length).toBe(2);
  });

  it("should handle diamond dependency pattern", () => {
    // Graph edges use "from depends on to" semantics
    // Expected: D=0, B=1, C=1, A=2
    const graph: DependencyGraphResponse = {
      nodes: [
        { proposalId: "A", title: "Task A", inDegree: 0, outDegree: 2 },
        { proposalId: "B", title: "Task B", inDegree: 1, outDegree: 1 },
        { proposalId: "C", title: "Task C", inDegree: 1, outDegree: 1 },
        { proposalId: "D", title: "Task D", inDegree: 2, outDegree: 0 },
      ],
      edges: [
        { from: "A", to: "B" }, // A depends on B
        { from: "A", to: "C" }, // A depends on C
        { from: "B", to: "D" }, // B depends on D
        { from: "C", to: "D" }, // C depends on D
      ],
      criticalPath: ["A", "B", "D"],
      hasCycles: false,
      cycles: null,
    };

    const result = computeDependencyTiers(graph);

    expect(result.tierMap.get("A")).toBe(2);
    expect(result.tierMap.get("B")).toBe(1);
    expect(result.tierMap.get("C")).toBe(1);
    expect(result.tierMap.get("D")).toBe(0);
    expect(result.maxTier).toBe(2);
  });

  it("should handle cycles gracefully", () => {
    // Graph with cycle: A ↔ B (both depend on each other)
    const result = computeDependencyTiers(mockGraphWithCycles);

    // Both should be assigned a tier (don't crash)
    expect(result.tierMap.has("proposal-1")).toBe(true);
    expect(result.tierMap.has("proposal-2")).toBe(true);
    // Exact tier values may vary, but function should complete
    expect(result.maxTier).toBeGreaterThanOrEqual(0);
  });

  it("should handle partial cycles with non-cyclic nodes", () => {
    // Graph: A (independent) → B ↔ C (B and C form a cycle)
    const graph: DependencyGraphResponse = {
      nodes: [
        { proposalId: "A", title: "Task A", inDegree: 0, outDegree: 1 },
        { proposalId: "B", title: "Task B", inDegree: 2, outDegree: 1 },
        { proposalId: "C", title: "Task C", inDegree: 1, outDegree: 1 },
      ],
      edges: [
        { from: "B", to: "A" }, // B depends on A
        { from: "B", to: "C" }, // B depends on C (cycle)
        { from: "C", to: "B" }, // C depends on B (cycle)
      ],
      criticalPath: [],
      hasCycles: true,
      cycles: [["B", "C", "B"]],
    };

    const result = computeDependencyTiers(graph);

    // A should be tier 0 (no dependencies)
    expect(result.tierMap.get("A")).toBe(0);
    // B and C should have tiers (don't crash)
    expect(result.tierMap.has("B")).toBe(true);
    expect(result.tierMap.has("C")).toBe(true);
  });

  it("should create tier groups correctly", () => {
    const result = computeDependencyTiers(mockGraph);

    // Tier 0: proposal-3
    expect(result.tierGroups.get(0)).toEqual(["proposal-3"]);
    // Tier 1: proposal-2
    expect(result.tierGroups.get(1)).toEqual(["proposal-2"]);
    // Tier 2: proposal-1
    expect(result.tierGroups.get(2)).toEqual(["proposal-1"]);
    // Should have exactly 3 tier groups
    expect(result.tierGroups.size).toBe(3);
  });
});

describe("useDependencyTiers", () => {
  it("should return tier assignment using useMemo", () => {
    const { result } = renderHook(() => useDependencyTiers(mockGraph), {
      wrapper: createWrapper(),
    });

    expect(result.current.tierMap.get("proposal-1")).toBe(2);
    expect(result.current.tierMap.get("proposal-2")).toBe(1);
    expect(result.current.tierMap.get("proposal-3")).toBe(0);
    expect(result.current.maxTier).toBe(2);
  });

  it("should handle null graph", () => {
    const { result } = renderHook(() => useDependencyTiers(null), {
      wrapper: createWrapper(),
    });

    expect(result.current.tierMap.size).toBe(0);
    expect(result.current.maxTier).toBe(0);
    expect(result.current.tierGroups.size).toBe(0);
  });

  it("should memoize result", () => {
    const { result, rerender } = renderHook(
      ({ graph }) => useDependencyTiers(graph),
      {
        wrapper: createWrapper(),
        initialProps: { graph: mockGraph },
      }
    );

    const firstResult = result.current;
    rerender({ graph: mockGraph });
    const secondResult = result.current;

    // Same reference if input unchanged
    expect(firstResult).toBe(secondResult);
  });
});

describe("getDependencyReason", () => {
  it("should return reason for existing edge", () => {
    const reason = getDependencyReason(mockGraph, "proposal-1", "proposal-2");
    expect(reason).toBe("API needs database schema");
  });

  it("should return reason for another existing edge", () => {
    const reason = getDependencyReason(mockGraph, "proposal-2", "proposal-3");
    expect(reason).toBe("UI requires API endpoints");
  });

  it("should return undefined for non-existent edge", () => {
    const reason = getDependencyReason(mockGraph, "proposal-1", "proposal-3");
    expect(reason).toBeUndefined();
  });

  it("should return undefined for reversed edge direction", () => {
    // Edge exists from proposal-1 to proposal-2, not the reverse
    const reason = getDependencyReason(mockGraph, "proposal-2", "proposal-1");
    expect(reason).toBeUndefined();
  });

  it("should return undefined for null graph", () => {
    const reason = getDependencyReason(null, "proposal-1", "proposal-2");
    expect(reason).toBeUndefined();
  });

  it("should return undefined for undefined graph", () => {
    const reason = getDependencyReason(undefined, "proposal-1", "proposal-2");
    expect(reason).toBeUndefined();
  });

  it("should return undefined when edge has no reason field", () => {
    const graphWithNoReasons: DependencyGraphResponse = {
      ...mockGraphWithCycles,
      edges: [
        { from: "proposal-1", to: "proposal-2" },
        { from: "proposal-2", to: "proposal-1" },
      ],
    };
    const reason = getDependencyReason(graphWithNoReasons, "proposal-1", "proposal-2");
    expect(reason).toBeUndefined();
  });

  it("should return undefined when edge reason is null", () => {
    const graphWithNullReason: DependencyGraphResponse = {
      nodes: [
        { proposalId: "A", title: "Task A", inDegree: 0, outDegree: 1 },
        { proposalId: "B", title: "Task B", inDegree: 1, outDegree: 0 },
      ],
      edges: [
        { from: "A", to: "B", reason: null },
      ],
      criticalPath: [],
      hasCycles: false,
      cycles: null,
    };
    const reason = getDependencyReason(graphWithNullReason, "A", "B");
    expect(reason).toBeUndefined();
  });
});
