/**
 * TaskGraphView - React Flow-based task dependency graph visualization
 *
 * Displays tasks as nodes with dependencies as edges.
 * Uses dagre for hierarchical layout computation.
 * Custom TaskNode and DependencyEdge components provide rich visualization.
 *
 * Layout: Uses GraphSplitLayout for split-screen view with:
 * - Left: Graph canvas with FloatingGraphFilters overlay
 * - Right: FloatingTimeline
 * - Footer: Optional ExecutionControlBar
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  Controls,
  type Node,
  type Edge,
  type NodeTypes,
  type EdgeTypes,
  type OnNodesChange,
  type OnEdgesChange,
} from "@xyflow/react";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import "@xyflow/react/dist/style.css";

import { useTaskGraph } from "./hooks/useTaskGraph";
import { useTaskGraphLayout, type PlanBranchNodeContext } from "./hooks/useTaskGraphLayout";
import { useTaskGraphViewport } from "./hooks/useTaskGraphViewport";
import { useGraphSelectionController } from "./hooks/useGraphSelectionController";
import { type TaskNodeHandlers } from "./nodes/TaskNode";
import { UnifiedTaskNode } from "./nodes/UnifiedTaskNode";
import { DependencyEdge } from "./edges/DependencyEdge";
import { MARKER_IDS, NORMAL_STROKE, CRITICAL_STROKE, EDGE_FADE_COLOR } from "./edges/edgeStyles";
import { PlanGroup, PLAN_GROUP_NODE_TYPE, type PlanGroupData } from "./groups/PlanGroup";
import { TierGroup, TIER_GROUP_NODE_TYPE, type TierGroupData } from "./groups/TierGroup";
import { getPlanGroupNodeId, getTierGroupNodeId } from "./groups/groupTypes";
import { FloatingTimeline } from "./timeline/FloatingTimeline";
import { GraphLegend } from "./controls/GraphLegend";
import { FloatingGraphFilters } from "./controls/FloatingGraphFilters";
import { PlanSelectorInline } from "@/components/plan/PlanSelectorInline";
import type { SelectionSource } from "@/api/plan";
import {
  COMPACT_MODE_THRESHOLD,
  DEFAULT_GRAPH_FILTERS,
  DEFAULT_LAYOUT_DIRECTION,
  DEFAULT_GROUPING,
  type NodeMode,
  type GraphFilters,
  type LayoutDirection,
  type GroupingState,
} from "./controls/graph-controls";
import { GraphSplitLayout } from "@/components/layout/GraphSplitLayout";
import type { TaskGraphNode, TaskGraphEdge, PlanGroupInfo } from "@/api/task-graph.types";
import type { InternalStatus } from "@/types/status";
import { useUiStore } from "@/stores/uiStore";
import { usePlanStore, selectActivePlanId, selectActiveExecutionPlanId } from "@/stores/planStore";
import { useTaskMutation } from "@/hooks/useTaskMutation";
import { useNavCompactBreakpoint } from "@/hooks";
import { usePersistedNodeMode } from "@/hooks/usePersistedNodeMode";
import { taskGraphKeys } from "./hooks/useTaskGraph";
import { api } from "@/lib/tauri";
import { openTaskInAgents } from "@/lib/navigation";
import { toast } from "sonner";
import { AlertCircle, Filter, Loader2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { buildTierGroups, UNGROUPED_PLAN_ID } from "./groups/tierGroupUtils";
import type { GroupInfo } from "@/lib/task-actions";

// ============================================================================
// Types
// ============================================================================

export interface TaskGraphViewProps {
  projectId: string;
  /** Optional ideation session ID to filter tasks by plan */
  ideationSessionId?: string | null;
  /** Hide the global active-plan selector for a session-scoped host. */
  hidePlanSelector?: boolean;
  /** Optional footer to render at the bottom of the left section (e.g., ExecutionControlBar) */
  footer?: React.ReactNode;
  /** Optional host-owned task selection handler for embedded task surfaces. */
  onTaskSelect?: (taskId: string) => void;
  /** Opens the global plan quick switcher with source attribution */
  onOpenPlanQuickSwitcher?: (source: SelectionSource) => void;
  readOnly?: boolean;
}

// ============================================================================
// Constants
// ============================================================================

/** Empty layout config - defined as constant to prevent re-renders */
// Layout config is now computed dynamically based on node mode

function areSetsEqual(a: Set<string>, b: Set<string>): boolean {
  if (a.size !== b.size) return false;
  for (const value of a) {
    if (!b.has(value)) return false;
  }
  return true;
}

/**
 * Apply filters to graph data
 * Filters nodes based on status and plan selections.
 * Archived filtering is handled by the backend via `include_archived` param.
 * Filters edges to only include those where both source and target are visible.
 */
function applyGraphFilters(
  nodes: TaskGraphNode[],
  edges: TaskGraphEdge[],
  planGroups: PlanGroupInfo[],
  filters: GraphFilters
): { nodes: TaskGraphNode[]; edges: TaskGraphEdge[]; planGroups: PlanGroupInfo[] } {
  // Short-circuit: if no client-side filters are active, return original references
  const noFilters = filters.statuses.length === 0;
  if (noFilters) {
    return { nodes, edges, planGroups };
  }

  // Filter nodes
  const filteredNodes = nodes.filter((node) => {
    const status = node.internalStatus as InternalStatus;

    // Check status filter (empty = show all)
    if (filters.statuses.length > 0 && !filters.statuses.includes(status)) {
      return false;
    }

    return true;
  });

  // Build set of visible node IDs for edge filtering
  const visibleNodeIds = new Set(filteredNodes.map((n) => n.taskId));

  // Filter edges - only keep edges where both source and target are visible
  const filteredEdges = edges.filter(
    (edge) => visibleNodeIds.has(edge.source) && visibleNodeIds.has(edge.target)
  );

  // Update plan groups with filtered task lists
  const filteredPlanGroups = planGroups.map((group) => ({
    ...group,
    taskIds: group.taskIds.filter((taskId) => visibleNodeIds.has(taskId)),
  }));

  return {
    nodes: filteredNodes,
    edges: filteredEdges,
    planGroups: filteredPlanGroups,
  };
}

// ============================================================================
// Custom Node and Edge Types
// ============================================================================

// ============================================================================
// Edge Marker Definitions
// ============================================================================

/**
 * SVG definitions for edge arrows and gradients
 * Arrows are slightly larger for better visibility
 * Gradients fade from stroke color to near-background at the end
 */
function EdgeMarkerDefinitions() {
  return (
    <svg style={{ position: "absolute", width: 0, height: 0 }}>
      <defs>
        {/* Gradient definitions for edge fade effect */}
        <linearGradient id="edge-gradient-normal" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stopColor={NORMAL_STROKE} />
          <stop offset="70%" stopColor={NORMAL_STROKE} />
          <stop offset="100%" stopColor={EDGE_FADE_COLOR} stopOpacity="0.3" />
        </linearGradient>
        <linearGradient id="edge-gradient-critical" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stopColor={CRITICAL_STROKE} />
          <stop offset="70%" stopColor={CRITICAL_STROKE} />
          <stop offset="100%" stopColor={EDGE_FADE_COLOR} stopOpacity="0.3" />
        </linearGradient>
        <linearGradient id="edge-gradient-active" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stopColor={CRITICAL_STROKE} />
          <stop offset="70%" stopColor={CRITICAL_STROKE} />
          <stop offset="100%" stopColor={EDGE_FADE_COLOR} stopOpacity="0.3" />
        </linearGradient>

        {/* Normal edge arrow - muted gray, larger for visibility */}
        <marker
          id={MARKER_IDS.normal}
          viewBox="0 0 10 10"
          refX={9}
          refY={5}
          markerWidth={10}
          markerHeight={8}
          markerUnits="userSpaceOnUse"
          orient="auto-start-reverse"
        >
          <path
            d="M 0 0 L 10 5 L 0 10 z"
            fill={NORMAL_STROKE}
            fillOpacity="0.8"
          />
        </marker>

        {/* Critical path edge arrow - orange, larger for visibility */}
        <marker
          id={MARKER_IDS.critical}
          viewBox="0 0 10 10"
          refX={9}
          refY={5}
          markerWidth={10}
          markerHeight={8}
          markerUnits="userSpaceOnUse"
          orient="auto-start-reverse"
        >
          <path
            d="M 0 0 L 10 5 L 0 10 z"
            fill={CRITICAL_STROKE}
          />
        </marker>

        {/* Active edge arrow - orange, larger for visibility */}
        <marker
          id={MARKER_IDS.active}
          viewBox="0 0 10 10"
          refX={9}
          refY={5}
          markerWidth={10}
          markerHeight={8}
          markerUnits="userSpaceOnUse"
          orient="auto-start-reverse"
        >
          <path
            d="M 0 0 L 10 5 L 0 10 z"
            fill={CRITICAL_STROKE}
          />
        </marker>
      </defs>
    </svg>
  );
}

/**
 * Unified node types - single TaskNode component reads per-node mode from data.nodeMode.
 * IMPORTANT: Defined outside component to prevent unnecessary re-renders.
 */
const unifiedNodeTypes: NodeTypes = {
  task: UnifiedTaskNode,
  [PLAN_GROUP_NODE_TYPE]: PlanGroup,
  [TIER_GROUP_NODE_TYPE]: TierGroup,
};

/**
 * Register custom edge types for React Flow
 * IMPORTANT: Defined outside component to prevent unnecessary re-renders
 */
const edgeTypes: EdgeTypes = {
  dependency: DependencyEdge,
};

// ============================================================================
// Inner Component (has access to useReactFlow)
// ============================================================================

interface TaskGraphViewInnerProps {
  projectId: string;
  /** Optional ideation session ID to filter tasks by plan */
  ideationSessionId?: string | null;
  /** Hide the global active-plan selector for a session-scoped host. */
  hidePlanSelector?: boolean;
  /** Optional footer to render at the bottom of the left section (e.g., ExecutionControlBar) */
  footer?: React.ReactNode;
  /** Optional host-owned task selection handler for embedded task surfaces. */
  onTaskSelect?: (taskId: string) => void;
  /** Opens the global plan quick switcher with source attribution */
  onOpenPlanQuickSwitcher?: (source: SelectionSource) => void;
  readOnly?: boolean;
}

function TaskGraphViewInner({
  projectId,
  ideationSessionId,
  hidePlanSelector = false,
  footer,
  onTaskSelect,
  onOpenPlanQuickSwitcher,
  readOnly = false,
}: TaskGraphViewInnerProps) {
  // Graph filter state (declared early so showArchived is available for useTaskGraph)
  const [filters, setFilters] = useState<GraphFilters>(DEFAULT_GRAPH_FILTERS);

  // Get active plan ID from plan store (used for empty state check and plan selector)
  const activePlanId = usePlanStore(selectActivePlanId(projectId));
  // Get active execution plan ID for graph filtering
  const activeExecutionPlanId = usePlanStore(selectActiveExecutionPlanId(projectId));

  // Load active plan from backend on mount or project change
  useEffect(() => {
    usePlanStore.getState().loadActivePlan(projectId);
  }, [projectId]);

  const effectiveExecutionPlanId = ideationSessionId ? null : activeExecutionPlanId;
  const { data: graphData, isLoading, error } = useTaskGraph(
    projectId,
    filters.showArchived,
    effectiveExecutionPlanId,
    ideationSessionId ?? null,
  );
  const {
    fitNodeInView,
    fitNode,
    centerOnNode,
    centerOnNodeObject,
    fitViewDefault,
    zoomBy,
  } = useTaskGraphViewport();
  const graphRightPanelUserOpen = useUiStore((s) => s.graphRightPanelUserOpen);
  const graphRightPanelCompactOpen = useUiStore((s) => s.graphRightPanelCompactOpen);
  const { isNavCompact } = useNavCompactBreakpoint();
  const queryClient = useQueryClient();
  const clearGraphSelection = useUiStore((s) => s.clearGraphSelection);
  const [overlayClosing, setOverlayClosing] = useState(false);
  const overlayCloseTimeoutRef = useRef<number | null>(null);
  const pendingTierAutoCenterRef = useRef<string | null>(null);
  const lastAutoCenteredPlanRef = useRef<string | null>(null);
  const previousActivePlanIdRef = useRef<string | null>(activePlanId);
  const expandedPlanIdRef = useRef<string | null>(null);
  const initialFitDoneRef = useRef(false);

  const graphReady = Boolean(graphData && graphData.nodes.length > 0);
  const requiresActivePlan = !ideationSessionId;

  // Collapse state for plan groups
  const [collapsedPlanIds, setCollapsedPlanIds] = useState<Set<string>>(
    new Set()
  );
  const [collapsedTierIds, setCollapsedTierIds] = useState<Set<string>>(
    new Set()
  );

  // Track user-toggled groups so data refreshes don't override manual expand/collapse.
  // Two sets per axis: one for explicit expansions, one for explicit collapses.
  // Refs (not state) because they annotate existing state transitions — no extra renders.
  const userExpandedPlanIds = useRef(new Set<string>());
  const userCollapsedPlanIds = useRef(new Set<string>());
  const userExpandedTierIds = useRef(new Set<string>());
  const userCollapsedTierIds = useRef(new Set<string>());

  const [grouping, setGrouping] = useState<GroupingState>(DEFAULT_GROUPING);

  // Clear user-tracking refs on project switch (stale IDs from previous project)
  /* eslint-disable react-hooks/refs -- ref read/write during render is intentional ("adjust state during render" pattern for synchronous collapse-state derivation) */
  const prevProjectIdRef = useRef(projectId);
  if (projectId !== prevProjectIdRef.current) {
    prevProjectIdRef.current = projectId;
    userExpandedPlanIds.current.clear();
    userCollapsedPlanIds.current.clear();
    userExpandedTierIds.current.clear();
    userCollapsedTierIds.current.clear();
  }

  // ---- Synchronous collapsed-state derivation --------------------------------
  // React "adjust state during render" pattern: compute which plan/tier groups
  // should be collapsed BEFORE children render.  This ensures dagre receives
  // the correct collapsed set on the very first committed render, eliminating
  // the visible "nodes flying into place" flash on initial load.
  // See: react.dev/reference/react/useState#storing-information-from-previous-renders

  // Shared: determine which plan group should be expanded by default
  const { expandedPlanId, planGroupInfos } = useMemo(() => {
    if (!graphData) {
      return {
        expandedPlanId: null as string | null,
        planGroupInfos: [] as Array<{ id: string; total: number; completed: number }>,
      };
    }

    const pgs = graphData.planGroups ?? [];
    const allNodes = graphData.nodes ?? [];

    const groupedTaskIds = new Set<string>();
    for (const pg of pgs) {
      for (const taskId of pg.taskIds) {
        groupedTaskIds.add(taskId);
      }
    }
    const ungroupedTasks = allNodes.filter((n) => !groupedTaskIds.has(n.taskId));

    const allGroups: Array<{ id: string; total: number; completed: number }> = [];
    for (const pg of pgs) {
      allGroups.push({
        id: pg.planArtifactId,
        total: pg.taskIds.length,
        completed: pg.statusSummary.completed,
      });
    }
    if (ungroupedTasks.length > 0) {
      const completedCount = ungroupedTasks.filter((t) =>
        ["approved", "merged", "completed"].includes(t.internalStatus)
      ).length;
      allGroups.push({
        id: UNGROUPED_PLAN_ID,
        total: ungroupedTasks.length,
        completed: completedCount,
      });
    }

    if (allGroups.length === 0) {
      return { expandedPlanId: null as string | null, planGroupInfos: allGroups };
    }

    let expandedGroupId: string | null = null;
    for (const g of allGroups) {
      if (g.total === 0 || g.completed < g.total) {
        expandedGroupId = g.id;
        break;
      }
    }
    if (!expandedGroupId) {
      expandedGroupId = allGroups[allGroups.length - 1]?.id ?? null;
    }

    return { expandedPlanId: expandedGroupId, planGroupInfos: allGroups };
  }, [graphData]);

  // Keep ref in sync (used by initial-fit effect)
  useEffect(() => {
    expandedPlanIdRef.current = expandedPlanId;
  }, [expandedPlanId]);

  // Plan group collapse: recompute defaults synchronously when graphData changes.
  // Respects user-toggled groups: manually expanded groups stay expanded,
  // manually collapsed groups stay collapsed, regardless of the heuristic.
  const [prevPlanCollapseData, setPrevPlanCollapseData] = useState<typeof graphData>(undefined);
  if (graphData !== prevPlanCollapseData) {
    setPrevPlanCollapseData(graphData);
    if (planGroupInfos.length > 0 && expandedPlanId !== null) {
      const toCollapse = new Set<string>();
      for (const g of planGroupInfos) {
        // User explicitly expanded → keep expanded (don't collapse)
        if (userExpandedPlanIds.current.has(g.id)) continue;
        // User explicitly collapsed → keep collapsed
        if (userCollapsedPlanIds.current.has(g.id)) {
          toCollapse.add(g.id);
          continue;
        }
        // Heuristic default: expand only the first incomplete group
        if (g.id !== expandedPlanId) {
          toCollapse.add(g.id);
        }
      }
      setCollapsedPlanIds(toCollapse);
    } else {
      setCollapsedPlanIds(new Set());
    }
  }

  const centerOnPlanGroup = useCallback(
    (planArtifactId: string, duration = 200, zoom = 0.9): boolean =>
      centerOnNode(getPlanGroupNodeId(planArtifactId), {
        duration,
        zoom,
        fallbackWidth: 320,
        fallbackHeight: 120,
      }),
    [centerOnNode]
  );

  // Tier group collapse: recompute defaults synchronously when inputs change
  let tierAutoCenterCandidate: string | null = null;
  const [prevTierCollapseInputs, setPrevTierCollapseInputs] = useState<{
    graphData: typeof graphData;
    byPlan: boolean;
    byTier: boolean;
    showUncategorized: boolean;
  } | null>(null);

  const tierInputsChanged = prevTierCollapseInputs === null
    || graphData !== prevTierCollapseInputs.graphData
    || grouping.byPlan !== prevTierCollapseInputs.byPlan
    || grouping.byTier !== prevTierCollapseInputs.byTier
    || grouping.showUncategorized !== prevTierCollapseInputs.showUncategorized;

  let expandedPlanHasTiers = false;
  if (tierInputsChanged) {
    // Detect if grouping mode changed (not just a data refresh).
    // On mode change, clear all user tracking refs — fresh heuristic.
    const groupingModeChanged = prevTierCollapseInputs !== null && (
      grouping.byPlan !== prevTierCollapseInputs.byPlan
      || grouping.byTier !== prevTierCollapseInputs.byTier
      || grouping.showUncategorized !== prevTierCollapseInputs.showUncategorized
    );
    if (groupingModeChanged) {
      userExpandedPlanIds.current.clear();
      userCollapsedPlanIds.current.clear();
      userExpandedTierIds.current.clear();
      userCollapsedTierIds.current.clear();
    }

    setPrevTierCollapseInputs({
      graphData,
      byPlan: grouping.byPlan,
      byTier: grouping.byTier,
      showUncategorized: grouping.showUncategorized,
    });

    if (!graphData || !grouping.byTier) {
      setCollapsedTierIds((prev) => prev.size > 0 ? new Set<string>() : prev);
    } else {
      const pgs = graphData.planGroups ?? [];
      const allNodes = graphData.nodes ?? [];
      const computedTierGroups = buildTierGroups(allNodes, pgs, {
        enabled: grouping.byTier,
        includeUngrouped: grouping.showUncategorized || !grouping.byPlan,
      });

      if (computedTierGroups.length === 0) {
        setCollapsedTierIds((prev) => prev.size > 0 ? new Set<string>() : prev);
      } else {
        const completedStatuses = new Set(["approved", "merged", "completed"]);
        const nodeMap = new Map(allNodes.map((node) => [node.taskId, node]));
        const tiersByPlanLocal = new Map<string, typeof computedTierGroups>();

        for (const tg of computedTierGroups) {
          const existing = tiersByPlanLocal.get(tg.planArtifactId);
          if (existing) {
            existing.push(tg);
          } else {
            tiersByPlanLocal.set(tg.planArtifactId, [tg]);
          }
        }

        const toCollapseTiers = new Set<string>();
        for (const [planArtifactId, tiers] of tiersByPlanLocal) {
          // For the expanded plan group, all tiers default to expanded.
          // But respect user-collapsed tiers within it.
          if (planArtifactId === expandedPlanId) {
            for (const tg of tiers) {
              if (userCollapsedTierIds.current.has(tg.id)) {
                toCollapseTiers.add(tg.id);
              }
            }
            continue;
          }

          const tierInfos = tiers.map((tg) => {
            let completed = 0;
            for (const taskId of tg.taskIds) {
              const node = nodeMap.get(taskId);
              if (node && completedStatuses.has(node.internalStatus)) {
                completed += 1;
              }
            }
            return { id: tg.id, total: tg.taskIds.length, completed };
          });

          let expandedTierId: string | null = null;
          for (const tier of tierInfos) {
            if (tier.total === 0 || tier.completed < tier.total) {
              expandedTierId = tier.id;
              break;
            }
          }
          if (!expandedTierId) {
            expandedTierId = tierInfos[tierInfos.length - 1]?.id ?? null;
          }

          for (const tier of tierInfos) {
            // User explicitly expanded → keep expanded (don't collapse)
            if (userExpandedTierIds.current.has(tier.id)) continue;
            // User explicitly collapsed → keep collapsed
            if (userCollapsedTierIds.current.has(tier.id)) {
              toCollapseTiers.add(tier.id);
              continue;
            }
            // Heuristic default
            if (tier.id !== expandedTierId) {
              toCollapseTiers.add(tier.id);
            }
          }
        }

        setCollapsedTierIds((prev) => areSetsEqual(prev, toCollapseTiers) ? prev : toCollapseTiers);

        // Track whether expandedPlanId has tiers (used by effect below for auto-center)
        if (expandedPlanId && tiersByPlanLocal.has(expandedPlanId)) {
          tierAutoCenterCandidate = expandedPlanId;
        }
        expandedPlanHasTiers = tiersByPlanLocal.has(expandedPlanId ?? "");
      }
    }
  }

  // Schedule auto-center via effect (refs must not be accessed during render)
  useEffect(() => {
    if (
      tierAutoCenterCandidate &&
      initialFitDoneRef.current &&
      tierAutoCenterCandidate !== lastAutoCenteredPlanRef.current
    ) {
      pendingTierAutoCenterRef.current = tierAutoCenterCandidate;
    }
  });

  /* eslint-enable react-hooks/refs */
  // ---- End synchronous collapsed-state derivation ----------------------------

  // Auto-center on expanded plan group when tier collapse inputs change.
  // Moved out of synchronous block to avoid accessing refs during render.
  useEffect(() => {
    if (
      initialFitDoneRef.current &&
      expandedPlanId &&
      expandedPlanHasTiers &&
      expandedPlanId !== lastAutoCenteredPlanRef.current
    ) {
      pendingTierAutoCenterRef.current = expandedPlanId;
    }
  }, [expandedPlanId, expandedPlanHasTiers, tierInputsChanged]);

  useEffect(() => {
    if (!isNavCompact) {
      if (overlayCloseTimeoutRef.current) {
        window.clearTimeout(overlayCloseTimeoutRef.current);
        overlayCloseTimeoutRef.current = null;
      }
      setOverlayClosing(false);
      return;
    }

    if (graphRightPanelCompactOpen) {
      if (overlayCloseTimeoutRef.current) {
        window.clearTimeout(overlayCloseTimeoutRef.current);
        overlayCloseTimeoutRef.current = null;
      }
      setOverlayClosing(false);
      return;
    }

    setOverlayClosing(true);
    overlayCloseTimeoutRef.current = window.setTimeout(() => {
      setOverlayClosing(false);
      overlayCloseTimeoutRef.current = null;
    }, 200);

    return () => {
      if (overlayCloseTimeoutRef.current) {
        window.clearTimeout(overlayCloseTimeoutRef.current);
        overlayCloseTimeoutRef.current = null;
      }
    };
  }, [isNavCompact, graphRightPanelCompactOpen]);

  const [layoutDirection, setLayoutDirection] = useState<LayoutDirection>(DEFAULT_LAYOUT_DIRECTION);

  // Node mode state (standard or compact), persisted to localStorage
  // null means "auto" - will be determined by task count
  const [nodeModeOverride, setNodeModeOverride] = usePersistedNodeMode();

  // Apply filters to graph data before layout computation
  const filteredGraphData = useMemo(() => {
    if (!graphData) {
      return { nodes: [], edges: [], planGroups: [] };
    }
    return applyGraphFilters(
      graphData.nodes,
      graphData.edges,
      graphData.planGroups,
      filters
    );
  }, [graphData, filters]);

  // Per-group auto-compact: each expanded plan group independently checks taskIds.length >= threshold.
  const autoCompactGroupIds = useMemo(() => {
    const ids = new Set<string>();
    if (!graphData || !grouping.byPlan) return ids;

    const planGroups = filteredGraphData.planGroups;

    for (const pg of planGroups) {
      if (!collapsedPlanIds.has(pg.planArtifactId) && pg.taskIds.length >= COMPACT_MODE_THRESHOLD) {
        ids.add(pg.planArtifactId);
      }
    }

    // Check ungrouped pseudo-group
    if (!collapsedPlanIds.has(UNGROUPED_PLAN_ID)) {
      const groupedTaskIds = new Set(planGroups.flatMap((pg) => pg.taskIds));
      const ungroupedCount = filteredGraphData.nodes.filter((n) => !groupedTaskIds.has(n.taskId)).length;
      if (ungroupedCount >= COMPACT_MODE_THRESHOLD) {
        ids.add(UNGROUPED_PLAN_ID);
      }
    }
    return ids;
  }, [filteredGraphData, collapsedPlanIds, graphData, grouping.byPlan]);

  const hasAnyAutoCompact = autoCompactGroupIds.size > 0;
  const isAutoCompactActive = nodeModeOverride === null && hasAnyAutoCompact;

  // Per-node mode lookup: resolves each node's effective display mode.
  const nodeModeLookup = useMemo(() => {
    const map = new Map<string, NodeMode>();

    // Explicit user choice always wins over auto-compact.
    if (nodeModeOverride === "standard") {
      for (const n of filteredGraphData.nodes) {
        map.set(n.taskId, "standard");
      }
      return map;
    }

    // If user set compact globally → all compact
    if (nodeModeOverride === "compact") {
      for (const n of filteredGraphData.nodes) {
        map.set(n.taskId, "compact");
      }
      return map;
    }

    // Build taskId → groupId mapping
    const taskToGroup = new Map<string, string>();
    for (const pg of filteredGraphData.planGroups) {
      for (const taskId of pg.taskIds) {
        taskToGroup.set(taskId, pg.planArtifactId);
      }
    }

    // Auto mode: per-group threshold decides compactness.
    for (const n of filteredGraphData.nodes) {
      const groupId = taskToGroup.get(n.taskId) ?? UNGROUPED_PLAN_ID;
      const isGroupAutoCompact = autoCompactGroupIds.has(groupId);
      map.set(n.taskId, isGroupAutoCompact ? "compact" : "standard");
    }
    return map;
  }, [filteredGraphData, nodeModeOverride, autoCompactGroupIds]);

  // Derive a global effective mode for controls.
  // Explicit user selection overrides auto-compact.
  const effectiveNodeMode: NodeMode = nodeModeOverride === null
    ? (hasAnyAutoCompact ? "compact" : "standard")
    : nodeModeOverride;

  // Handler for manual node mode toggle
  const handleNodeModeChange = useCallback((mode: NodeMode) => {
    if (mode === "compact") {
      setNodeModeOverride("compact");
      return;
    }
    // For standard: use explicit override while auto-compact would otherwise apply.
    // When auto-compact is inactive, standard can fall back to auto mode.
    if (hasAnyAutoCompact) {
      setNodeModeOverride("standard");
    } else {
      setNodeModeOverride(null);
    }
  }, [hasAnyAutoCompact, setNodeModeOverride]);

  const tierGroups = useMemo(
    () => buildTierGroups(filteredGraphData.nodes, filteredGraphData.planGroups, {
      enabled: grouping.byTier,
      includeUngrouped: grouping.showUncategorized || !grouping.byPlan,
    }),
    [
      filteredGraphData.nodes,
      filteredGraphData.planGroups,
      grouping.byPlan,
      grouping.byTier,
      grouping.showUncategorized,
    ]
  );

  const tierGroupsById = useMemo(() => {
    const map = new Map<string, (typeof tierGroups)[number]>();
    for (const tierGroup of tierGroups) {
      map.set(tierGroup.id, tierGroup);
    }
    return map;
  }, [tierGroups]);

  // Shared viewport centering: try fitNodeInView → centerOnPlanGroup → fitViewDefault
  const centerOnPlanGroupNode = useCallback(
    (planArtifactId: string, duration = 200) => {
      if (fitNodeInView(getPlanGroupNodeId(planArtifactId), { duration, padding: 0.18, maxZoom: 0.95 })) return;
      if (centerOnPlanGroup(planArtifactId, duration, 0.9)) return;
      fitViewDefault({ padding: 0.2, duration });
    },
    [centerOnPlanGroup, fitNodeInView, fitViewDefault]
  );

  const tierGroupsByPlan = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const tierGroup of tierGroups) {
      const existing = map.get(tierGroup.planArtifactId);
      if (existing) {
        existing.push(tierGroup.id);
      } else {
        map.set(tierGroup.planArtifactId, [tierGroup.id]);
      }
    }
    return map;
  }, [tierGroups]);

  // Toggle collapse state for a plan group
  // When expanding, also expand ALL tier groups within it for full visibility
  // When collapsing, also collapse all tier groups for a clean state
  const handleToggleCollapse = useCallback((planArtifactId: string) => {
    let expanding = false;
    setCollapsedPlanIds((prev) => {
      const next = new Set(prev);
      if (next.has(planArtifactId)) {
        next.delete(planArtifactId);
        expanding = true;
      } else {
        next.add(planArtifactId);
      }
      return next;
    });
    // Record user intent so data refreshes don't override this toggle
    if (expanding) {
      userExpandedPlanIds.current.add(planArtifactId);
      userCollapsedPlanIds.current.delete(planArtifactId);
    } else {
      userCollapsedPlanIds.current.add(planArtifactId);
      userExpandedPlanIds.current.delete(planArtifactId);
    }
    const tierIds = tierGroupsByPlan.get(planArtifactId) ?? [];
    if (tierIds.length > 0) {
      setCollapsedTierIds((prev) => {
        const next = new Set(prev);
        if (expanding) {
          for (const tierId of tierIds) {
            next.delete(tierId);
          }
        } else {
          for (const tierId of tierIds) {
            next.add(tierId);
          }
        }
        return next;
      });
      // Also record tier intent when toggling parent plan group
      for (const tierId of tierIds) {
        if (expanding) {
          userExpandedTierIds.current.add(tierId);
          userCollapsedTierIds.current.delete(tierId);
        } else {
          userCollapsedTierIds.current.add(tierId);
          userExpandedTierIds.current.delete(tierId);
        }
      }
    }
    setTimeout(() => centerOnPlanGroupNode(planArtifactId), 50);
  }, [centerOnPlanGroupNode, tierGroupsByPlan]);

  const handleToggleTierCollapse = useCallback((tierGroupId: string) => {
    let expanding = false;
    const planArtifactId = tierGroupsById.get(tierGroupId)?.planArtifactId ?? null;
    setCollapsedTierIds((prev) => {
      const next = new Set(prev);
      if (next.has(tierGroupId)) {
        next.delete(tierGroupId);
        expanding = true;
      } else {
        next.add(tierGroupId);
      }
      return next;
    });
    // Record user intent for this tier
    if (expanding) {
      userExpandedTierIds.current.add(tierGroupId);
      userCollapsedTierIds.current.delete(tierGroupId);
    } else {
      userCollapsedTierIds.current.add(tierGroupId);
      userExpandedTierIds.current.delete(tierGroupId);
    }
    setTimeout(() => {
      if (expanding) {
        // Expanding tier → center on the tier group
        const tierNodeId = getTierGroupNodeId(tierGroupId);
        if (fitNodeInView(tierNodeId, { duration: 200, padding: 0.18, maxZoom: 0.95 })) return;
        if (centerOnNode(tierNodeId, { duration: 200, zoom: 0.95, fallbackWidth: 320, fallbackHeight: 80 })) return;
      }
      // Collapsing tier → center on parent plan group
      if (planArtifactId) {
        centerOnPlanGroupNode(planArtifactId);
        return;
      }
      fitViewDefault({ padding: 0.2, duration: 200 });
    }, 50);
  }, [centerOnNode, centerOnPlanGroupNode, fitNodeInView, fitViewDefault, tierGroupsById]);

  const handleToggleAllTiers = useCallback(
    (planArtifactId: string, action: "expand" | "collapse") => {
      const tierIds = tierGroupsByPlan.get(planArtifactId) ?? [];
      if (tierIds.length === 0) return;
      setCollapsedTierIds((prev) => {
        const next = new Set(prev);
        if (action === "expand") {
          for (const tierId of tierIds) {
            next.delete(tierId);
          }
        } else {
          for (const tierId of tierIds) {
            next.add(tierId);
          }
        }
        return next;
      });
      // Record user intent for all affected tiers
      for (const tierId of tierIds) {
        if (action === "expand") {
          userExpandedTierIds.current.add(tierId);
          userCollapsedTierIds.current.delete(tierId);
        } else {
          userCollapsedTierIds.current.add(tierId);
          userExpandedTierIds.current.delete(tierId);
        }
      }
      setTimeout(() => centerOnPlanGroupNode(planArtifactId), 50);
    },
    [centerOnPlanGroupNode, tierGroupsByPlan]
  );

  // Task mutations for context menu actions
  const {
    moveMutation,
    blockMutation,
    unblockMutation,
    cancelTasksInGroupMutation,
    pauseTasksInGroupMutation,
    resumeTasksInGroupMutation,
    archiveTasksInGroupMutation,
  } = useTaskMutation(projectId);

  // ============================================================================
  // Context Menu Handlers
  // ============================================================================

  // Handler for viewing task details (opens the host-owned Agents detail)
  const handleViewDetails = useCallback((taskId: string) => {
    if (onTaskSelect) {
      onTaskSelect(taskId);
      return;
    }
    void openTaskInAgents(taskId, "graph", { projectId });
  }, [onTaskSelect, projectId]);

  // Handler for starting task execution (move to executing via state machine)
  const handleStartExecution = useCallback(async (taskId: string) => {
    try {
      // Moving to "executing" triggers the scheduler to pick up the task
      await api.tasks.move(taskId, "executing");
      toast.success("Task scheduled for execution");
    } catch (err) {
      toast.error(`Failed to start task: ${err instanceof Error ? err.message : String(err)}`);
    }
  }, []);

  // Handler for blocking a task with reason
  const handleBlockWithReason = useCallback((taskId: string, reason?: string) => {
    // Only pass reason if it's defined (satisfies exactOptionalPropertyTypes)
    if (reason !== undefined) {
      blockMutation.mutate({ taskId, reason });
    } else {
      blockMutation.mutate({ taskId });
    }
  }, [blockMutation]);

  // Handler for unblocking a task
  const handleUnblock = useCallback((taskId: string) => {
    unblockMutation.mutate(taskId);
  }, [unblockMutation]);

  // Handler for approving a task
  const handleApprove = useCallback(async (taskId: string) => {
    try {
      await api.reviews.approveTask({ task_id: taskId });
      toast.success("Task approved");
    } catch (err) {
      toast.error(`Failed to approve task: ${err instanceof Error ? err.message : String(err)}`);
    }
  }, []);

  // Handler for rejecting a task (move to failed)
  const handleReject = useCallback((taskId: string) => {
    moveMutation.mutate({ taskId, toStatus: "failed" });
  }, [moveMutation]);

  // Handler for requesting changes (move to revision_needed)
  const handleRequestChanges = useCallback((taskId: string) => {
    moveMutation.mutate({ taskId, toStatus: "revision_needed" });
  }, [moveMutation]);

  // Handler for marking merge conflict as resolved
  const handleMarkResolved = useCallback(async (taskId: string) => {
    try {
      await api.tasks.move(taskId, "pending_merge");
      toast.success("Conflict marked as resolved");
    } catch (err) {
      toast.error(`Failed to mark resolved: ${err instanceof Error ? err.message : String(err)}`);
    }
  }, []);

  // Task removal handler (context menu "Remove" — confirmation handled by TaskContextMenuItems)
  const handleRemoveTask = useCallback(
    async (taskId: string) => {
      try {
        await api.tasks.cleanupTask(taskId);
        clearGraphSelection();
        queryClient.invalidateQueries({ queryKey: taskGraphKeys.graphPrefix(projectId) });
        toast.success("Task removed");
      } catch {
        toast.error("Failed to remove task");
      }
    },
    [clearGraphSelection, queryClient, projectId]
  );

  // Memoized handlers object for nodes
  const nodeHandlers: TaskNodeHandlers = useMemo(() => ({
    onViewDetails: handleViewDetails,
    onStartExecution: handleStartExecution,
    onBlockWithReason: handleBlockWithReason,
    onUnblock: handleUnblock,
    onApprove: handleApprove,
    onReject: handleReject,
    onRequestChanges: handleRequestChanges,
    onMarkResolved: handleMarkResolved,
    onRemove: handleRemoveTask,
  }), [
    handleViewDetails,
    handleStartExecution,
    handleBlockWithReason,
    handleUnblock,
    handleApprove,
    handleReject,
    handleRequestChanges,
    handleMarkResolved,
    handleRemoveTask,
  ]);

  // Layout config with per-node mode lookup for mixed dimensions.
  // Tighten spacing automatically for dense graphs to avoid an over-wide canvas.
  const layoutConfig = useMemo(() => {
    const taskCount = filteredGraphData.nodes.length;

    const tierCounts = new Map<number, number>();
    for (const node of filteredGraphData.nodes) {
      const current = tierCounts.get(node.tier) ?? 0;
      tierCounts.set(node.tier, current + 1);
    }
    const maxTierBreadth = Math.max(0, ...tierCounts.values());

    const groupedTaskIds = new Set(filteredGraphData.planGroups.flatMap((group) => group.taskIds));
    const ungroupedCount = filteredGraphData.nodes.filter((node) => !groupedTaskIds.has(node.taskId)).length;
    const maxPlanGroupSize = Math.max(
      0,
      ...filteredGraphData.planGroups.map((group) => group.taskIds.length),
      ungroupedCount
    );

    const isDenseGraph =
      taskCount >= 20 || maxTierBreadth >= 4 || maxPlanGroupSize >= 12 || hasAnyAutoCompact;
    const isUltraDenseGraph = taskCount >= 55 || maxTierBreadth >= 8 || maxPlanGroupSize >= 24;

    return {
      nodeModeLookup,
      nodesep: isUltraDenseGraph ? 16 : isDenseGraph ? 24 : 40,
      ranksep: isUltraDenseGraph ? 42 : isDenseGraph ? 56 : 72,
      marginx: isUltraDenseGraph ? 10 : isDenseGraph ? 16 : 24,
      marginy: isUltraDenseGraph ? 14 : isDenseGraph ? 22 : 32,
    };
  }, [filteredGraphData.nodes, filteredGraphData.planGroups, hasAnyAutoCompact, nodeModeLookup]);

  const handleCancelAllInGroup = useCallback(
    async (sessionId: string) => {
      try {
        const isUncategorized = sessionId === "";
        await cancelTasksInGroupMutation.mutateAsync({
          groupKind: isUncategorized ? "uncategorized" : "session",
          groupId: isUncategorized ? "" : sessionId,
          projectId,
        });
        queryClient.invalidateQueries({ queryKey: taskGraphKeys.graphPrefix(projectId) });
      } catch {
        // Error toast is handled by the mutation's onError
      }
    },
    [cancelTasksInGroupMutation, projectId, queryClient]
  );

  const handlePauseAllInGroup = useCallback(
    async (sessionId: string) => {
      try {
        const isUncategorized = sessionId === "";
        await pauseTasksInGroupMutation.mutateAsync({
          groupKind: isUncategorized ? "uncategorized" : "session",
          groupId: isUncategorized ? "" : sessionId,
          projectId,
        });
        queryClient.invalidateQueries({ queryKey: taskGraphKeys.graphPrefix(projectId) });
      } catch {
        // Error toast is handled by the mutation's onError
      }
    },
    [pauseTasksInGroupMutation, projectId, queryClient]
  );

  const handleResumeAllInGroup = useCallback(
    async (sessionId: string) => {
      try {
        const isUncategorized = sessionId === "";
        await resumeTasksInGroupMutation.mutateAsync({
          groupKind: isUncategorized ? "uncategorized" : "session",
          groupId: isUncategorized ? "" : sessionId,
          projectId,
        });
        queryClient.invalidateQueries({ queryKey: taskGraphKeys.graphPrefix(projectId) });
      } catch {
        // Error toast is handled by the mutation's onError
      }
    },
    [resumeTasksInGroupMutation, projectId, queryClient]
  );

  const handleArchiveAllInGroup = useCallback(
    async (sessionId: string) => {
      try {
        const isUncategorized = sessionId === "";
        await archiveTasksInGroupMutation.mutateAsync({
          groupKind: isUncategorized ? "uncategorized" : "session",
          groupId: isUncategorized ? "" : sessionId,
          projectId,
        });
        queryClient.invalidateQueries({ queryKey: taskGraphKeys.graphPrefix(projectId) });
      } catch {
        // Error toast is handled by the mutation's onError
      }
    },
    [archiveTasksInGroupMutation, projectId, queryClient]
  );

  const hasVisiblePlanMergeNode = useMemo(
    () => (filteredGraphData.nodes ?? []).some((node) => node.category === "plan_merge"),
    [filteredGraphData.nodes]
  );

  // Fetch project plan branches once for graph merge context. The plan-id command intentionally
  // hides merged/abandoned rows, but historical plan_merge nodes need those rows by merge_task_id.
  const planBranchQuery = useQuery({
    queryKey: ["plan-branches", "project", projectId] as const,
    queryFn: () => api.planBranches.getByProject(projectId),
    enabled: hasVisiblePlanMergeNode,
    staleTime: 5_000,
  });

  const planBranchContextMap = useMemo(() => {
    const map = new Map<string, PlanBranchNodeContext>();
    for (const branch of planBranchQuery.data ?? []) {
      const context = {
        mergeTarget: branch.baseBranchOverride ?? branch.sourceBranch,
        prNumber: branch.prNumber,
        prStatus: branch.prStatus,
        status: branch.status,
      };
      map.set(branch.planArtifactId, context);
      if (branch.mergeTaskId) {
        map.set(branch.mergeTaskId, context);
      }
    }
    return map;
  }, [planBranchQuery.data]);

  // Compute layout using dagre (includes plan grouping)
  const { nodes: layoutNodes, edges: layoutEdges, groupNodes } = useTaskGraphLayout(
    filteredGraphData.nodes,
    filteredGraphData.edges,
    graphData?.criticalPath ?? [],
    filteredGraphData.planGroups,
    grouping,
    layoutConfig,
    collapsedPlanIds,
    collapsedTierIds,
    handleToggleCollapse,
    handleToggleTierCollapse,
    handleToggleAllTiers,
    projectId,
    handleViewDetails,
    readOnly ? undefined : handleCancelAllInGroup,
    planBranchContextMap
  );

  useEffect(() => {
    const pendingPlanId = pendingTierAutoCenterRef.current;
    if (!pendingPlanId) return;
    if (isLoading || !graphReady) return;
    if (!groupNodes.some((node) => node.id === getPlanGroupNodeId(pendingPlanId))) {
      return;
    }
    pendingTierAutoCenterRef.current = null;
    lastAutoCenteredPlanRef.current = pendingPlanId;
    requestAnimationFrame(() => {
      requestAnimationFrame(() => centerOnPlanGroupNode(pendingPlanId));
    });
  }, [centerOnPlanGroupNode, graphReady, groupNodes, isLoading]);

  const {
    focusedNodeId,
    highlightedTaskId,
    graphSelection,
    select,
    focusSelectionInView,
    containerRef,
    onNodeClick,
    onNodeDoubleClick,
    onPaneClick,
    onTimelineTaskClick,
    onKeyDown,
  } = useGraphSelectionController({
    nodes: layoutNodes,
    edges: layoutEdges,
    layoutNodes,
    groupNodes,
    planGroups: filteredGraphData.planGroups,
    tierGroups,
    grouping,
    collapsedPlanIds,
    collapsedTierIds,
    onToggleCollapse: handleToggleCollapse,
    onToggleTierCollapse: handleToggleTierCollapse,
    onToggleAllTiers: handleToggleAllTiers,
    onTaskSelect: handleViewDetails,
    centerOnPlanGroup,
    centerOnNode,
    centerOnNodeObject,
    fitNode,
    fitViewDefault,
    zoomBy,
    graphReady,
    graphError: error ?? null,
    isLoading,
    selectionScope: {
      projectId,
      ideationSessionId: ideationSessionId ?? null,
    },
    keyboardNavigationEnabled: true,
  });

  const activePlanArtifactId = useMemo(
    () =>
      filteredGraphData.planGroups.find((group) => group.sessionId === activePlanId)?.planArtifactId ?? null,
    [activePlanId, filteredGraphData.planGroups]
  );

  // Re-center when active plan switches in Graph using the same selection/focus API path
  // used elsewhere (select -> focusSelectionInView), preserving center+zoom behavior.
  useEffect(() => {
    if (!activePlanId) {
      previousActivePlanIdRef.current = null;
      return;
    }
    if (!initialFitDoneRef.current || isLoading || !graphReady) return;
    if (!activePlanArtifactId) return;
    if (previousActivePlanIdRef.current === activePlanId) return;

    previousActivePlanIdRef.current = activePlanId;
    lastAutoCenteredPlanRef.current = null;
    pendingTierAutoCenterRef.current = null;
    select({ kind: "planGroup", id: activePlanArtifactId });
  }, [activePlanArtifactId, activePlanId, graphReady, isLoading, select]);

  // Compute visible nodes and edges (controlled mode).
  // React Flow v12 StoreUpdater triggers setNodes() whenever the nodes array
  // reference changes. Upstream hooks produce new array references even when
  // content is unchanged (web mode mock layer triggers this). We stabilize
  // by comparing node IDs and visual state, returning the previous array ref
  // when structurally unchanged to prevent infinite re-render loops.
  //
  // IMPORTANT: useRef + useMemo is intentional here. DO NOT convert to
  // useState-during-render — it causes infinite re-render cascades when
  // combined with the plan/tier collapse setState-during-render blocks above.
  // The ref reads inside useMemo are a well-known React stabilization pattern.

  // Build task-to-group lookup for injecting groupInfo into task node context menus
  // eslint-disable-next-line react-hooks/preserve-manual-memoization -- depends on plan-group shape plus injected action handlers for stable task menu wiring
  const taskGroupInfoMap = useMemo(() => {
    const map = new Map<string, GroupInfo>();
    if (readOnly || !filteredGraphData?.planGroups) return map;

    for (const pg of filteredGraphData.planGroups) {
      const isUncategorized = pg.planArtifactId === UNGROUPED_PLAN_ID;
      const groupKind = isUncategorized ? "uncategorized" as const : "plan" as const;
      const groupLabel = pg.sessionTitle ?? "Unnamed plan";
      const sessionId = pg.sessionId;

      const groupSessionId = isUncategorized ? "" : sessionId;
      for (const taskId of pg.taskIds) {
        map.set(taskId, {
          groupLabel,
          groupKind,
          taskCount: pg.taskIds.length,
          groupId: groupSessionId,
          projectId,
          onCancelAll: () => handleCancelAllInGroup(groupSessionId),
          onPauseAll: () => handlePauseAllInGroup(groupSessionId),
          onResumeAll: () => handleResumeAllInGroup(groupSessionId),
          onArchiveAll: () => handleArchiveAllInGroup(groupSessionId),
        });
      }
    }
    return map;
  }, [filteredGraphData?.planGroups, projectId, handleCancelAllInGroup, handlePauseAllInGroup, handleResumeAllInGroup, handleArchiveAllInGroup, readOnly]);

  const prevNodesRef = useRef<Node[]>([]);
  const prevEdgesRef = useRef<Edge[]>([]);

  /* eslint-disable react-hooks/refs -- ref read/write in useMemo is intentional (stabilization pattern to prevent infinite re-renders) */
  const nodes = useMemo<Node[]>(() => {
    const groupNodesWithSelection = groupNodes.map((node) => {
      if (node.type === PLAN_GROUP_NODE_TYPE) {
        const planArtifactId = (node.data as PlanGroupData | undefined)?.planArtifactId;
        const isSelected = graphSelection?.kind === "planGroup" && graphSelection.id === planArtifactId;
        return {
          ...node,
          data: {
            ...node.data,
            isSelected,
          },
        };
      }
      if (node.type === TIER_GROUP_NODE_TYPE) {
        const tierGroupId = (node.data as TierGroupData | undefined)?.tierGroupId;
        const isSelected = graphSelection?.kind === "tierGroup" && graphSelection.id === tierGroupId;
        return {
          ...node,
          data: {
            ...node.data,
            isSelected,
          },
        };
      }
      return node;
    });
    const taskNodesWithData = layoutNodes.map((node) => {
      const gi = taskGroupInfoMap.get(node.id);
      return {
        ...node,
        data: {
          ...node.data,
          nodeMode: nodeModeLookup.get(node.id) ?? "standard",
          isHighlighted: node.id === highlightedTaskId,
          isFocused: node.id === focusedNodeId,
          ...(readOnly ? {} : { handlers: nodeHandlers }),
          ...(!readOnly && gi !== undefined && { groupInfo: gi }),
        },
        selected: graphSelection?.kind === "task" && graphSelection.id === node.id,
      };
    });
    const next = [...groupNodesWithSelection, ...taskNodesWithData];
    // Lightweight identity check — IDs, positions, visual state, task data.
    // Must include positions so dagre re-layout after expand/collapse propagates.
    // Must include internalStatus so real-time status changes propagate to nodes.
    const prev = prevNodesRef.current;
    if (next.length === prev.length) {
      let unchanged = true;
      for (let i = 0; i < next.length; i++) {
        const n = next[i]!;
        const p = prev[i]!;
        const nd = n.data as Record<string, unknown>;
        const pd = p.data as Record<string, unknown>;
        if (
          n.id !== p.id
          || n.position.x !== p.position.x
          || n.position.y !== p.position.y
          || n.selected !== p.selected
          || n.type !== p.type
          || nd?.isSelected !== pd?.isSelected
          || nd?.isHighlighted !== pd?.isHighlighted
          || nd?.isFocused !== pd?.isFocused
          || nd?.internalStatus !== pd?.internalStatus
          || nd?.label !== pd?.label
          || nd?.category !== pd?.category
          || nd?.mergeTarget !== pd?.mergeTarget
          || nd?.prNumber !== pd?.prNumber
          || nd?.prStatus !== pd?.prStatus
          || nd?.planBranchStatus !== pd?.planBranchStatus
          || nd?.isCriticalPath !== pd?.isCriticalPath
          || nd?.nodeMode !== pd?.nodeMode
        ) {
          unchanged = false;
          break;
        }
      }
      if (unchanged) return prev;
    }
    prevNodesRef.current = next;
    return next;
  }, [layoutNodes, groupNodes, graphSelection, highlightedTaskId, focusedNodeId, nodeHandlers, readOnly, taskGroupInfoMap, nodeModeLookup]);

  const edges = useMemo<Edge[]>(() => {
    const prev = prevEdgesRef.current;
    if (layoutEdges.length === prev.length && layoutEdges.every((e, i) =>
      e.id === prev[i]!.id
      && (e.data as Record<string, unknown>)?.isCriticalPath === (prev[i]!.data as Record<string, unknown>)?.isCriticalPath
    )) {
      return prev;
    }
    prevEdgesRef.current = layoutEdges;
    return layoutEdges;
  }, [layoutEdges]);
  /* eslint-enable react-hooks/refs */

  // Handle node changes (for selection, dragging etc.) in controlled mode
  const onNodesChange: OnNodesChange = useCallback(() => {
    // We don't allow user-driven node changes (positions come from dagre layout)
    // Selection is handled via onNodeClick
  }, []);

  // Handle edge changes in controlled mode
  const onEdgesChange: OnEdgesChange = useCallback(() => {
    // We don't allow user-driven edge changes (edges are computed from dependencies)
  }, []);

  // One-time initial select + center on expanded group after dagre layout is ready.
  // Polls until the group node is measured by React Flow, then selects it via
  // the controller's stable `select()` API. Falls back to fitViewDefault.
  // Hide ReactFlow until initial viewport positioning is complete.
  // Prevents visible flash of default viewport before centering on expanded group.
  const [initialViewReady, setInitialViewReady] = useState(false);
  useEffect(() => {
    if (initialFitDoneRef.current) return;
    if (!graphReady || layoutNodes.length === 0) return;
    const hasPositions = layoutNodes.some((n) => n.position.x !== 0 || n.position.y !== 0);
    if (!hasPositions && groupNodes.length === 0) return;
    initialFitDoneRef.current = true;
    // Cancel any pending tier auto-center — initial fit owns viewport positioning
    pendingTierAutoCenterRef.current = null;

    const expandedId = expandedPlanIdRef.current;
    if (expandedId) {
      lastAutoCenteredPlanRef.current = expandedId;
    }

    // Extra frame after positioning to let React Flow paint at correct viewport
    const settle = () => {
      requestAnimationFrame(() => setInitialViewReady(true));
    };

    const attemptFocus = (attempt: number) => {
      if (attempt > 15) {
        fitViewDefault({ padding: 0.2, duration: 0 });
        settle();
        return;
      }
      if (expandedId) {
        const nodeId = getPlanGroupNodeId(expandedId);
        if (fitNodeInView(nodeId, { duration: 0, padding: 0.18, maxZoom: 0.95 })) {
          select({ kind: "planGroup", id: expandedId }, { skipFocus: true });
          settle();
          return;
        }
      } else {
        fitViewDefault({ padding: 0.2, duration: 0 });
        settle();
        return;
      }
      requestAnimationFrame(() => attemptFocus(attempt + 1));
    };
    requestAnimationFrame(() => {
      requestAnimationFrame(() => attemptFocus(0));
    });
  }, [graphReady, layoutNodes, groupNodes, fitNodeInView, centerOnPlanGroup, fitViewDefault, select]);

  const graphRightPanelVisible = isNavCompact
    ? graphRightPanelCompactOpen
    : graphRightPanelUserOpen;
  const rightPanelMode = !graphRightPanelVisible
    ? "hidden"
    : isNavCompact
      ? "overlay"
      : "split";

  // Re-center current selection when selection or panel layout changes.
  // Uses ref to avoid re-firing when focusSelectionInView ref changes.
  // Skips on first graphSelection change (initial load handles its own positioning).
  const focusInViewRef = useRef(focusSelectionInView);
  useEffect(() => {
    focusInViewRef.current = focusSelectionInView;
  }, [focusSelectionInView]);
  const prevGraphSelectionRef = useRef<typeof graphSelection>(undefined);
  const prevEffectiveNodeModeRef = useRef<NodeMode | null>(null);
  const prevGraphRightPanelVisibleRef = useRef<boolean | null>(null);

  useEffect(() => {
    if (!graphSelection) return;
    const isFirstSelection = prevGraphSelectionRef.current === undefined;
    prevGraphSelectionRef.current = graphSelection;
    if (isFirstSelection) return;
    focusInViewRef.current(graphSelection);
  }, [graphSelection, graphRightPanelVisible, isNavCompact]);

  // Re-center when opening the right panel (Cmd+L timeline toggle) with no active selection.
  // Keep viewport behavior consistent with capped focus logic.
  useEffect(() => {
    if (!initialFitDoneRef.current || !graphReady || isLoading) return;

    const previousVisible = prevGraphRightPanelVisibleRef.current;
    prevGraphRightPanelVisibleRef.current = graphRightPanelVisible;
    if (previousVisible === null) return;

    const panelVisibilityChanged = previousVisible !== graphRightPanelVisible;
    if (!panelVisibilityChanged) return;
    if (graphSelection) return;

    if (activePlanArtifactId) {
      focusInViewRef.current({ kind: "planGroup", id: activePlanArtifactId });
      return;
    }

    fitViewDefault({ padding: 0.2, duration: 200 });
  }, [
    activePlanArtifactId,
    fitViewDefault,
    graphReady,
    graphRightPanelVisible,
    graphSelection,
    isLoading,
  ]);

  // Re-center when switching Compact/Standard mode.
  // Uses the same viewport focus API path so fit/zoom capping behavior remains consistent.
  useEffect(() => {
    if (!initialFitDoneRef.current || !graphReady || isLoading) return;

    const previousMode = prevEffectiveNodeModeRef.current;
    prevEffectiveNodeModeRef.current = effectiveNodeMode;
    if (previousMode === null || previousMode === effectiveNodeMode) return;

    if (graphSelection && graphSelection.kind !== "customGroup") {
      focusInViewRef.current(graphSelection);
      return;
    }

    if (activePlanArtifactId) {
      focusInViewRef.current({ kind: "planGroup", id: activePlanArtifactId });
      return;
    }

    fitViewDefault({ padding: 0.2, duration: 200 });
  }, [
    activePlanArtifactId,
    effectiveNodeMode,
    fitViewDefault,
    graphReady,
    graphSelection,
    isLoading,
  ]);

  // No plan selected — bypass project graph rendering. Embedded ideation graphs
  // are already scoped by ideationSessionId and do not require a global plan.
  if (requiresActivePlan && !activePlanId) {
    return (
      <div className="flex items-center justify-center h-full" data-testid="graph-empty-state">
        <EmptyState
          variant="neutral"
          icon={<AlertCircle />}
          title="No plan selected"
          description="Select a plan to view work on the Graph."
          action={
            <PlanSelectorInline
              projectId={projectId}
              source="graph_inline"
              onOpenPalette={(source) => onOpenPlanQuickSwitcher?.(source)}
            />
          }
        />
      </div>
    );
  }

  // Loading state
  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
      </div>
    );
  }

  // Error state
  if (error) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <p className="text-destructive mb-2">Failed to load task graph</p>
          <p className="text-sm text-muted-foreground">{error.message}</p>
        </div>
      </div>
    );
  }

  // Empty state (no tasks at all)
  if (!graphData || graphData.nodes.length === 0) {
    return (
      <div className="flex items-center justify-center h-full" data-testid="graph-empty-state">
        <div className="text-center">
          <p className="text-muted-foreground mb-2">No tasks to display</p>
          <p className="text-sm text-muted-foreground">
            Create tasks from the Ideation view to see them here
          </p>
        </div>
      </div>
    );
  }

  // Check if client-side filters might hide tasks (showArchived is a backend filter, not counted here)
  const hasActiveFilters =
    filters.statuses.length > 0;

  return (
    <>
    <GraphSplitLayout
      projectId={projectId}
      footer={footer}
      timelineContent={
        <FloatingTimeline
          projectId={projectId}
          executionPlanId={activeExecutionPlanId}
          onTaskClick={onTimelineTaskClick}
          highlightedTaskId={highlightedTaskId}
          variant={rightPanelMode === "overlay" || overlayClosing ? "overlay" : "panel"}
        />
      }
      rightPanelMode={rightPanelMode}
    >
      {/* Graph canvas container — hidden until initial viewport positioning completes */}
      <div
        className="h-full w-full relative outline-none"
        data-testid="task-graph-view"
        tabIndex={0}
        ref={containerRef}
        onKeyDown={onKeyDown}
        style={initialViewReady ? undefined : { visibility: "hidden" }}
      >
        {/* Floating filter controls - positioned over canvas */}
        <FloatingGraphFilters
          filters={filters}
          onFiltersChange={setFilters}
          layoutDirection={layoutDirection}
          onLayoutDirectionChange={setLayoutDirection}
          nodeMode={effectiveNodeMode}
          onNodeModeChange={handleNodeModeChange}
          isAutoCompact={isAutoCompactActive}
          grouping={grouping}
          onGroupingChange={setGrouping}
          isCompact={isNavCompact}
        />

        {/* Plan selector control (only when a plan is active) */}
        {!hidePlanSelector && activePlanId && requiresActivePlan && (
          <div className="absolute top-4 left-1/2 -translate-x-1/2 z-10">
            <PlanSelectorInline
              projectId={projectId}
              source="graph_inline"
              onOpenPalette={(source) => onOpenPlanQuickSwitcher?.(source)}
            />
          </div>
        )}

        {filteredGraphData.nodes.length === 0 && hasActiveFilters ? (
          <div className="flex items-center justify-center h-full">
            <EmptyState
              variant="neutral"
              icon={<Filter />}
              title="No tasks match current filters"
              action={
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setFilters(DEFAULT_GRAPH_FILTERS)}
                >
                  <X className="w-3 h-3 mr-1" />
                  Clear filters
                </Button>
              }
            />
          </div>
        ) : (
          <ReactFlow
            className="ralphx-task-graph"
            // eslint-disable-next-line react-hooks/refs -- nodes/edges are ref-stabilized (see useMemo blocks above)
            nodes={nodes}
            edges={edges} // eslint-disable-line react-hooks/refs
            nodeTypes={unifiedNodeTypes}
            edgeTypes={edgeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onNodeClick={onNodeClick}
            onNodeDoubleClick={onNodeDoubleClick}
            onPaneClick={onPaneClick}
            minZoom={0.6}
            maxZoom={1}
            zoomOnDoubleClick={false}
            proOptions={{ hideAttribution: true }}
          >
            {/* SVG marker definitions for edge arrows */}
            <EdgeMarkerDefinitions />
            <Background color="var(--border-subtle)" gap={24} />
            <Controls
              showInteractive={false}
              style={{
                background: "var(--bg-surface)",
                border: "1px solid var(--overlay-weak)",
                borderRadius: 8,
              }}
            />
            {/* Status Legend - positioned to right of Controls */}
            <div className="absolute bottom-4 left-14 z-10">
              <GraphLegend defaultCollapsed={true} />
            </div>
          </ReactFlow>
        )}

      </div>
    </GraphSplitLayout>
    </>
  );
}

// ============================================================================
// Main Component (provides ReactFlowProvider)
// ============================================================================

export function TaskGraphView({
  projectId,
  ideationSessionId,
  hidePlanSelector = false,
  footer,
  onTaskSelect,
  onOpenPlanQuickSwitcher,
  readOnly = false,
}: TaskGraphViewProps) {
  return (
    <ReactFlowProvider>
      <TaskGraphViewInner
        projectId={projectId}
        ideationSessionId={ideationSessionId ?? null}
        hidePlanSelector={hidePlanSelector}
        footer={footer}
        readOnly={readOnly}
        {...(onTaskSelect ? { onTaskSelect } : {})}
        {...(onOpenPlanQuickSwitcher
          ? { onOpenPlanQuickSwitcher }
          : {})}
      />
    </ReactFlowProvider>
  );
}
