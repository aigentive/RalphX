/**
 * useDependencyGraph hooks - TanStack Query wrappers for dependency graph
 *
 * Provides hooks for fetching the dependency graph and managing dependencies
 * between proposals within ideation sessions.
 */

import { useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { ideationApi, type DependencyGraphResponse } from "@/api/ideation";
import { proposalKeys } from "./useProposals";
import { ideationKeys } from "./useIdeation";

/**
 * Query key factory for dependencies
 */
export const dependencyKeys = {
  all: ["dependencies"] as const,
  graphs: () => [...dependencyKeys.all, "graph"] as const,
  graph: (sessionId: string) => [...dependencyKeys.graphs(), sessionId] as const,
};

/**
 * Hook to fetch the dependency graph for an ideation session
 *
 * @param sessionId - The session ID to fetch the graph for
 * @returns TanStack Query result with dependency graph
 *
 * @example
 * ```tsx
 * const { data: graph, isLoading, error } = useDependencyGraph("session-123");
 *
 * if (isLoading) return <Loading />;
 * if (error) return <Error message={error.message} />;
 *
 * return (
 *   <div>
 *     <h3>Dependencies</h3>
 *     <p>Nodes: {graph?.nodes.length}</p>
 *     <p>Has cycles: {graph?.hasCycles ? 'Yes' : 'No'}</p>
 *     {graph?.hasCycles && (
 *       <Warning>Circular dependencies detected!</Warning>
 *     )}
 *     <CriticalPath path={graph?.criticalPath ?? []} />
 *   </div>
 * );
 * ```
 */
export function useDependencyGraph(sessionId: string) {
  return useQuery<DependencyGraphResponse, Error>({
    queryKey: dependencyKeys.graph(sessionId),
    queryFn: () => ideationApi.dependencies.analyze(sessionId),
    enabled: Boolean(sessionId),
  });
}

/**
 * Input for dependency mutations
 */
interface DependencyInput {
  proposalId: string;
  dependsOnId: string;
}

/**
 * Hook for dependency management mutations
 *
 * @returns Object with mutation functions for removing dependencies
 *
 * Adding a dependency is deliberately absent: no `add_proposal_dependency` Tauri command
 * has ever existed, so the former `addDependency` mutation failed at runtime on every
 * call. Proposal dependencies are authored by agents through the backend, and the UI reads
 * the graph rather than writing edges.
 *
 * @example
 * ```tsx
 * const { removeDependency } = useDependencyMutations();
 *
 * // Remove a dependency
 * const handleRemove = async () => {
 *   await removeDependency.mutateAsync({
 *     proposalId: "proposal-2",
 *     dependsOnId: "proposal-1",
 *   });
 *   toast.success("Dependency removed");
 * };
 * ```
 */
export function useDependencyMutations() {
  const queryClient = useQueryClient();

  const removeDependency = useMutation<void, Error, DependencyInput>({
    mutationFn: ({ proposalId, dependsOnId }) =>
      ideationApi.dependencies.remove(proposalId, dependsOnId),
    onSuccess: () => {
      // Invalidate all dependency graphs
      queryClient.invalidateQueries({
        queryKey: dependencyKeys.graphs(),
      });
      // Also invalidate proposals since they may show dependency info
      queryClient.invalidateQueries({
        queryKey: proposalKeys.lists(),
      });
      // And session data
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionDetails(),
      });
    },
  });

  return {
    removeDependency,
  };
}

/**
 * Tier assignment result from topological grouping
 */
export interface TierAssignment {
  /** Map of proposalId to tier level (0 = foundation, 1 = core, 2+ = integration) */
  tierMap: Map<string, number>;
  /** Maximum tier level in the graph */
  maxTier: number;
  /** Proposals grouped by tier level */
  tierGroups: Map<number, string[]>;
}

/**
 * Compute topological tiers for proposals based on dependency relationships.
 *
 * Tier 0 (Foundation): Proposals with no dependencies (inDegree === 0)
 * Tier N: Proposals whose tier = max(tier of dependencies) + 1
 *
 * Handles cycles gracefully by assigning cyclic nodes to the highest
 * possible tier based on their non-cyclic dependencies.
 *
 * @param graph - The dependency graph with nodes and edges
 * @returns TierAssignment with tier map, max tier, and grouped proposals
 *
 * @example
 * ```tsx
 * const { data: graph } = useDependencyGraph(sessionId);
 * const tiers = useDependencyTiers(graph);
 *
 * // Get tier for a specific proposal
 * const tier = tiers.tierMap.get(proposalId) ?? 0;
 *
 * // Get all proposals in tier 1
 * const coreTier = tiers.tierGroups.get(1) ?? [];
 * ```
 */
export function computeDependencyTiers(graph: DependencyGraphResponse | null | undefined): TierAssignment {
  const tierMap = new Map<string, number>();
  const tierGroups = new Map<number, string[]>();

  if (!graph || graph.nodes.length === 0) {
    return { tierMap, maxTier: 0, tierGroups };
  }

  // Build adjacency map: proposalId -> list of proposals it depends on
  // Edge semantics: edge.from depends on edge.to (from → to in graph)
  const dependsOn = new Map<string, Set<string>>();
  for (const node of graph.nodes) {
    dependsOn.set(node.proposalId, new Set());
  }
  for (const edge of graph.edges) {
    // edge.from depends on edge.to
    const deps = dependsOn.get(edge.from);
    if (deps) {
      deps.add(edge.to);
    }
  }

  // Track which nodes are in cycles for special handling
  const nodesInCycles = new Set<string>();
  if (graph.cycles) {
    for (const cycle of graph.cycles) {
      for (const nodeId of cycle) {
        nodesInCycles.add(nodeId);
      }
    }
  }

  // Compute tiers using Kahn's algorithm approach
  // Start with nodes that have no dependencies (tier 0)
  const processed = new Set<string>();
  const queue: string[] = [];

  // Initialize tier 0: nodes with no dependencies
  for (const node of graph.nodes) {
    const deps = dependsOn.get(node.proposalId);
    if (!deps || deps.size === 0) {
      tierMap.set(node.proposalId, 0);
      queue.push(node.proposalId);
      processed.add(node.proposalId);
    }
  }

  // Process nodes level by level
  while (queue.length > 0) {
    const current = queue.shift()!;

    // Find all nodes that depend on the current node
    for (const node of graph.nodes) {
      if (processed.has(node.proposalId)) continue;

      const deps = dependsOn.get(node.proposalId);
      if (!deps || !deps.has(current)) continue;

      // Check if all dependencies of this node have been processed
      // For cycle handling: only consider non-cyclic dependencies
      let allDepsProcessed = true;
      let maxDepTier = -1;

      for (const depId of deps) {
        // If this dependency is part of a cycle with current node, skip it
        if (nodesInCycles.has(node.proposalId) && nodesInCycles.has(depId)) {
          // Check if they're in the same cycle
          const inSameCycle = graph.cycles?.some(
            (cycle) => cycle.includes(node.proposalId) && cycle.includes(depId)
          );
          if (inSameCycle) continue;
        }

        if (!processed.has(depId)) {
          allDepsProcessed = false;
          break;
        }
        const depTier = tierMap.get(depId) ?? 0;
        maxDepTier = Math.max(maxDepTier, depTier);
      }

      if (allDepsProcessed && maxDepTier >= 0) {
        const newTier = maxDepTier + 1;
        tierMap.set(node.proposalId, newTier);
        queue.push(node.proposalId);
        processed.add(node.proposalId);
      }
    }
  }

  // Handle any unprocessed nodes (in cycles or with unprocessed deps)
  // Assign them to tier based on their best available information
  for (const node of graph.nodes) {
    if (!processed.has(node.proposalId)) {
      // Find max tier among processed dependencies
      const deps = dependsOn.get(node.proposalId);
      let maxTier = 0;
      if (deps) {
        for (const depId of deps) {
          const depTier = tierMap.get(depId);
          if (depTier !== undefined) {
            maxTier = Math.max(maxTier, depTier + 1);
          }
        }
      }
      tierMap.set(node.proposalId, maxTier);
    }
  }

  // Build tier groups and find max tier
  let maxTier = 0;
  for (const [proposalId, tier] of tierMap) {
    maxTier = Math.max(maxTier, tier);
    const group = tierGroups.get(tier);
    if (group) {
      group.push(proposalId);
    } else {
      tierGroups.set(tier, [proposalId]);
    }
  }

  return { tierMap, maxTier, tierGroups };
}

/**
 * Hook to compute topological tiers for proposals in a dependency graph.
 *
 * Uses useMemo to only recompute when the graph changes.
 *
 * @param graph - The dependency graph (from useDependencyGraph)
 * @returns TierAssignment with tier map, max tier, and grouped proposals
 *
 * @example
 * ```tsx
 * const { data: graph } = useDependencyGraph(sessionId);
 * const { tierMap, tierGroups, maxTier } = useDependencyTiers(graph);
 *
 * return (
 *   <div>
 *     <p>Max tier depth: {maxTier}</p>
 *     {Array.from(tierGroups.entries()).map(([tier, proposals]) => (
 *       <TierSection key={tier} tier={tier} proposals={proposals} />
 *     ))}
 *   </div>
 * );
 * ```
 */
export function useDependencyTiers(graph: DependencyGraphResponse | null | undefined): TierAssignment {
  return useMemo(() => computeDependencyTiers(graph), [graph]);
}

/**
 * Get the reason text for a dependency edge.
 *
 * @param graph - The dependency graph
 * @param fromId - The proposal that is depended on
 * @param toId - The proposal that has the dependency
 * @returns The reason string if found, undefined otherwise
 *
 * @example
 * ```tsx
 * const reason = getDependencyReason(graph, "proposal-1", "proposal-2");
 * // Returns "Needs API types defined first" or undefined
 * ```
 */
export function getDependencyReason(
  graph: DependencyGraphResponse | null | undefined,
  fromId: string,
  toId: string
): string | undefined {
  if (!graph) return undefined;
  const edge = graph.edges.find((e) => e.from === fromId && e.to === toId);
  return edge?.reason ?? undefined;
}
