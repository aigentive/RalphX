/**
 * useTaskGraphLayout hook - Dagre-based hierarchical layout for task graph
 *
 * Uses dagre algorithm to compute node positions for a proper hierarchical layout
 * with configurable spacing and direction. Supports plan grouping with visual
 * region containers.
 *
 * Layout Caching: Dagre computation is expensive. We cache layouts by a structural
 * hash (node IDs + edge pairs + config). When only node data changes (e.g., status)
 * but structure is the same, we reuse cached positions and just update node data.
 */

import { useMemo, useRef } from "react";
import dagre from "@dagrejs/dagre";
import { Position, type Node, type Edge } from "@xyflow/react";
import type { TaskGraphNode, TaskGraphEdge, PlanGroupInfo } from "@/api/task-graph.types";
import {
  expandBoundingBox,
  boundingBoxToGroupNode,
  GROUP_PADDING,
  HEADER_HEIGHT,
  calculateBoundingBox,
  type BoundingBox,
} from "../groups/groupUtils";
import type { PlanGroupNode } from "../groups/PlanGroup";
import type { TierGroupNode } from "../groups/TierGroup";
import {
  buildTierGroups,
  type TierGroupInfo,
  UNGROUPED_PLAN_ID,
} from "../groups/tierGroupUtils";
import {
  buildPlanGroupNodes,
  buildTierGroupNodes,
  COLLAPSED_GROUP_WIDTH,
  COLLAPSED_TIER_WIDTH,
  COLLAPSED_TIER_HEIGHT,
} from "../groups/groupBuilder";
import { getPlanGroupNodeId, getTierGroupNodeId } from "../groups/groupTypes";
import { getScaledNodeDimensions } from "../nodes/nodeStyles";
import { useThemeStore } from "@/stores/themeStore";
import type { NodeMode } from "../controls/graph-controls";

// ============================================================================
// Types
// ============================================================================

export interface LayoutConfig {
  /** Layout direction: TB (top-to-bottom) or LR (left-to-right) */
  direction: "TB" | "LR";
  /** Horizontal spacing between nodes */
  nodesep: number;
  /** Vertical spacing between ranks/tiers */
  ranksep: number;
  /** Horizontal margin */
  marginx: number;
  /** Vertical margin */
  marginy: number;
  /** Per-node mode lookup for mixed standard/compact dimensions */
  nodeModeLookup?: Map<string, NodeMode>;
}

export interface LayoutResult {
  nodes: Node[];
  edges: Edge[];
  groupNodes: Array<PlanGroupNode | TierGroupNode>;
}

export interface GroupingConfig {
  byPlan: boolean;
  byTier: boolean;
  showUncategorized: boolean;
}

export interface PlanBranchNodeContext {
  mergeTarget?: string;
  prNumber?: number | null;
  prStatus?: string | null;
  status?: string;
}

// Use Record<string, unknown> compatible structure for React Flow
// This matches TaskNodeData from nodes/TaskNode.tsx
type TaskNodeData = Record<string, unknown> & {
  label: string;
  taskId: string;
  internalStatus: string;
  priority: number;
  isCriticalPath: boolean;
  description?: string | null;
  category?: string;
  /** Target branch for plan_merge category nodes */
  mergeTarget?: string;
  prNumber?: number | null;
  prStatus?: string | null;
  planBranchStatus?: string;
};

// Edge data for custom DependencyEdge component
type DependencyEdgeData = Record<string, unknown> & {
  isCriticalPath: boolean;
  sourceStatus?: string;
  /** Whether edge crosses plan group boundaries */
  isCrossPlan?: boolean;
  /** Source task title for tooltip */
  sourceLabel?: string;
  /** Target task title for tooltip */
  targetLabel?: string;
  /** Whether this is a synthetic edge connecting groups */
  isGroupConnector?: boolean;
};

// ============================================================================
// Default Configuration
// ============================================================================

const DEFAULT_CONFIG: LayoutConfig = {
  direction: "TB",
  nodesep: 60,
  ranksep: 80,
  marginx: 40,
  marginy: 40,
};


// ============================================================================
// Group Node Creation
// ============================================================================

const TIER_STACK_GAP = 12;

interface SizedNode {
  position: { x: number; y: number };
  width: number;
  height: number;
}

function calculateSizedBoundingBox(nodes: SizedNode[]): BoundingBox | null {
  if (nodes.length === 0) return null;

  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;

  for (const node of nodes) {
    minX = Math.min(minX, node.position.x);
    minY = Math.min(minY, node.position.y);
    maxX = Math.max(maxX, node.position.x + node.width);
    maxY = Math.max(maxY, node.position.y + node.height);
  }

  return {
    minX,
    minY,
    maxX,
    maxY,
    width: maxX - minX,
    height: maxY - minY,
  };
}

/**
 * Create React Flow group nodes for plan groups
 *
 * @param taskNodes - Positioned task nodes from dagre layout
 * @param planGroups - Plan group info from the API
 * @param collapsedPlanIds - Set of plan artifact IDs that are collapsed
 * @param positions - Position map from dagre (includes placeholder positions for collapsed groups)
 * @param onToggleCollapse - Callback when collapse is toggled
 * @returns Array of PlanGroupNode for React Flow
 */
function createGroupNodes(
  taskNodes: Node[],
  planGroups: PlanGroupInfo[],
  collapsedPlanIds: Set<string>,
  tierGroups: TierGroupInfo[],
  collapsedTierIds: Set<string>,
  positions: Map<string, { x: number; y: number }>,
  graphNodes: TaskGraphNode[],
  nodeWidth: number,
  nodeHeight: number,
  onToggleCollapse?: (planArtifactId: string) => void,
  onToggleAllTiers?: (planArtifactId: string, action: "expand" | "collapse") => void,
  includeUncategorized: boolean = true,
  projectId?: string,
  onNavigateToTask?: (taskId: string) => void,
  onCancelAll?: (sessionId: string) => void
): PlanGroupNode[] {
  const args = {
    taskNodes,
    planGroups,
    collapsedPlanIds,
    tierGroups,
    collapsedTierIds,
    positions,
    graphNodes,
    nodeWidth,
    nodeHeight,
    includeUncategorized,
    ...(onToggleCollapse && { onToggleCollapse }),
    ...(onToggleAllTiers && { onToggleAllTiers }),
    ...(projectId && { projectId }),
    ...(onNavigateToTask && { onNavigateToTask }),
    ...(onCancelAll && { onCancelAll }),
  };
  return buildPlanGroupNodes(args);
}

/**
 * Create React Flow group nodes for tier groups within plans.
 */
function createTierGroupNodes(
  taskNodes: Node[],
  tierGroups: TierGroupInfo[],
  collapsedTierIds: Set<string>,
  collapsedPlanIds: Set<string>,
  planGroupBounds: Map<string, { position: { x: number; y: number }; width: number }>,
  positions: Map<string, { x: number; y: number }>,
  nodeWidth: number,
  nodeHeight: number,
  onToggleCollapse?: (tierGroupId: string) => void
): TierGroupNode[] {
  const args = {
    taskNodes,
    tierGroups,
    collapsedTierIds,
    collapsedPlanIds,
    planGroupBounds,
    positions,
    nodeWidth,
    nodeHeight,
    ...(onToggleCollapse && { onToggleCollapse }),
  };
  return buildTierGroupNodes(args);
}

// ============================================================================
// Inter-Group Edge Generation
// ============================================================================

/**
 * Inter-group edge info for layout and rendering
 */
interface InterGroupEdgeInfo {
  /** Task node to use for dagre layout (leaf node from source group) */
  layoutSource: string;
  /** Task node to use for dagre layout (root node from target group) */
  layoutTarget: string;
  /** Group ID for rendering (source group) */
  sourceGroupId: string;
  /** Group ID for rendering (target group) */
  targetGroupId: string;
}

interface InterTierEdgeInfo {
  sourceTierId: string;
  targetTierId: string;
  sourceLabel: string;
  targetLabel: string;
}

/**
 * Generate synthetic edges between consecutive plan groups to stack them vertically.
 * Returns both task-level edges (for dagre layout) and group-level edges (for rendering).
 * Also handles ungrouped tasks as a pseudo-group.
 */
function generateInterGroupEdges(
  graphNodes: TaskGraphNode[],
  planGroups: PlanGroupInfo[],
  includeUncategorized: boolean
): InterGroupEdgeInfo[] {
  // Build node lookup maps
  const nodeMap = new Map<string, TaskGraphNode>();
  for (const node of graphNodes) {
    nodeMap.set(node.taskId, node);
  }

  // Find ungrouped tasks (tasks not in any plan group)
  const groupedTaskIds = new Set<string>();
  for (const pg of planGroups) {
    for (const taskId of pg.taskIds) {
      groupedTaskIds.add(taskId);
    }
  }
  const ungroupedTaskIds = graphNodes
    .filter((n) => !groupedTaskIds.has(n.taskId))
    .map((n) => n.taskId);

  // Build all groups including ungrouped pseudo-group
  interface GroupWithTasks {
    id: string;
    taskIds: string[];
  }
  const allGroups: GroupWithTasks[] = planGroups.map((pg) => ({
    id: pg.planArtifactId,
    taskIds: pg.taskIds,
  }));

  // Add ungrouped as a pseudo-group if there are ungrouped tasks
  if (includeUncategorized && ungroupedTaskIds.length > 0) {
    allGroups.push({
      id: UNGROUPED_PLAN_ID,
      taskIds: ungroupedTaskIds,
    });
  }

  // Need at least 2 groups to create connector edges
  if (allGroups.length < 2) return [];

  // Sort groups by the minimum tier of their tasks (earlier groups first)
  const groupsWithTier = allGroups.map((group) => {
    const tasks = group.taskIds.map((id) => nodeMap.get(id)).filter(Boolean) as TaskGraphNode[];
    const minTier = tasks.length > 0 ? Math.min(...tasks.map((t) => t.tier)) : Infinity;
    return { group, minTier };
  });
  groupsWithTier.sort((a, b) => a.minTier - b.minTier);

  const interGroupEdges: InterGroupEdgeInfo[] = [];

  // For each consecutive pair of groups, create a connector edge
  for (let i = 0; i < groupsWithTier.length - 1; i++) {
    const currentGroupInfo = groupsWithTier[i];
    const nextGroupInfo = groupsWithTier[i + 1];
    if (!currentGroupInfo || !nextGroupInfo) continue;

    const currentGroup = currentGroupInfo.group;
    const nextGroup = nextGroupInfo.group;

    // Find tasks in each group for layout edge
    const currentTasks = currentGroup.taskIds
      .map((id) => nodeMap.get(id))
      .filter(Boolean) as TaskGraphNode[];
    const nextTasks = nextGroup.taskIds
      .map((id) => nodeMap.get(id))
      .filter(Boolean) as TaskGraphNode[];

    if (currentTasks.length === 0 || nextTasks.length === 0) continue;

    // Pick leaf from current group (prefer outDegree === 0, else highest tier)
    const leafCandidates = currentTasks.filter((t) => t.outDegree === 0);
    const leafNode = leafCandidates.length > 0
      ? leafCandidates.reduce((a, b) => (a.tier > b.tier ? a : b))
      : currentTasks.reduce((a, b) => (a.tier > b.tier ? a : b));

    // Pick root from next group (prefer inDegree === 0, else lowest tier)
    const rootCandidates = nextTasks.filter((t) => t.inDegree === 0);
    const rootNode = rootCandidates.length > 0
      ? rootCandidates.reduce((a, b) => (a.tier < b.tier ? a : b))
      : nextTasks.reduce((a, b) => (a.tier < b.tier ? a : b));

    interGroupEdges.push({
      layoutSource: leafNode.taskId,
      layoutTarget: rootNode.taskId,
      sourceGroupId: currentGroup.id,
      targetGroupId: nextGroup.id,
    });
  }

  return interGroupEdges;
}

/**
 * Generate synthetic edges between consecutive tier groups within the same plan.
 * These are render-only edges for visual grouping (no dagre layout influence).
 */
function generateInterTierEdges(
  tierGroups: TierGroupInfo[],
  collapsedPlanIds: Set<string>
): InterTierEdgeInfo[] {
  const tiersByPlan = new Map<string, TierGroupInfo[]>();
  for (const tierGroup of tierGroups) {
    if (collapsedPlanIds.has(tierGroup.planArtifactId)) continue;
    const existing = tiersByPlan.get(tierGroup.planArtifactId);
    if (existing) {
      existing.push(tierGroup);
    } else {
      tiersByPlan.set(tierGroup.planArtifactId, [tierGroup]);
    }
  }

  const edges: InterTierEdgeInfo[] = [];
  for (const [, tiers] of tiersByPlan) {
    const ordered = [...tiers].sort((a, b) => a.tier - b.tier);
    if (ordered.length < 2) continue;
    for (let i = 0; i < ordered.length - 1; i++) {
      const sourceTier = ordered[i];
      const targetTier = ordered[i + 1];
      if (!sourceTier || !targetTier) continue;
      edges.push({
        sourceTierId: sourceTier.id,
        targetTierId: targetTier.id,
        sourceLabel: `Tier ${sourceTier.tier}`,
        targetLabel: `Tier ${targetTier.tier}`,
      });
    }
  }

  return edges;
}

// ============================================================================
// Layout Computation (with caching)
// ============================================================================

/**
 * Build set of task IDs that belong to collapsed groups.
 * Used for lazy loading - we skip these tasks in layout computation.
 */
function buildCollapsedTaskIds(
  graphNodes: TaskGraphNode[],
  planGroups: PlanGroupInfo[],
  tierGroups: TierGroupInfo[],
  collapsedPlanIds: Set<string>,
  collapsedTierIds: Set<string>,
  includeUncategorized: boolean
): Set<string> {
  const hiddenIds = new Set<string>();

  // Add tasks from collapsed plan groups
  for (const pg of planGroups) {
    if (collapsedPlanIds.has(pg.planArtifactId)) {
      for (const taskId of pg.taskIds) {
        hiddenIds.add(taskId);
      }
    }
  }

  // Check for ungrouped tasks if the ungrouped group is collapsed
  if (includeUncategorized && collapsedPlanIds.has(UNGROUPED_PLAN_ID)) {
    const groupedTaskIds = new Set<string>();
    for (const pg of planGroups) {
      for (const taskId of pg.taskIds) {
        groupedTaskIds.add(taskId);
      }
    }
    for (const node of graphNodes) {
      if (!groupedTaskIds.has(node.taskId)) {
        hiddenIds.add(node.taskId);
      }
    }
  }

  // Add tasks from collapsed tier groups (unless their plan is already collapsed)
  for (const tg of tierGroups) {
    if (collapsedTierIds.has(tg.id) && !collapsedPlanIds.has(tg.planArtifactId)) {
      for (const taskId of tg.taskIds) {
        hiddenIds.add(taskId);
      }
    }
  }

  return hiddenIds;
}

/**
 * Compute layout using cached positions if structure unchanged, otherwise recompute.
 *
 * LAZY LOADING: Tasks in collapsed groups are excluded from layout computation entirely.
 * This improves performance by avoiding dagre calculations for hidden nodes.
 * When a group expands, the cache is invalidated (hash changes) and layout is recomputed.
 */
function computeLayoutWithCache(
  graphNodes: TaskGraphNode[],
  graphEdges: TaskGraphEdge[],
  criticalPath: string[],
  planGroups: PlanGroupInfo[],
  grouping: GroupingConfig,
  config: LayoutConfig,
  collapsedPlanIds: Set<string>,
  collapsedTierIds: Set<string>,
  onToggleCollapse: ((planArtifactId: string) => void) | undefined,
  onToggleTierCollapse: ((tierGroupId: string) => void) | undefined,
  onToggleAllTiers: ((planArtifactId: string, action: "expand" | "collapse") => void) | undefined,
  cache: React.MutableRefObject<CachedLayout | null>,
  scaledDims: { nodeWidth: number; nodeHeight: number; compactNodeWidth: number; compactNodeHeight: number },
  projectId?: string,
  onNavigateToTask?: (taskId: string) => void,
  onCancelAll?: (sessionId: string) => void,
  planBranchMap?: Map<string, PlanBranchNodeContext>
): LayoutResult {
  // Helper to get per-node dimensions from the mode lookup
  const getNodeDimensions = (nodeId: string) => {
    const mode = config.nodeModeLookup?.get(nodeId) ?? "standard";
    return {
      width: mode === "compact" ? scaledDims.compactNodeWidth : scaledDims.nodeWidth,
      height: mode === "compact" ? scaledDims.compactNodeHeight : scaledDims.nodeHeight,
    };
  };
  // Default dimensions for group bounding box calculations (use standard as safe fallback)
  const defaultNodeWidth = scaledDims.nodeWidth;
  const defaultNodeHeight = scaledDims.nodeHeight;

  const planGroupingEnabled = grouping.byPlan;
  const includeUncategorized = grouping.showUncategorized || !planGroupingEnabled;
  const activePlanGroups = planGroupingEnabled ? planGroups : [];
  const tierGroups = buildTierGroups(graphNodes, activePlanGroups, {
    enabled: grouping.byTier,
    includeUngrouped: includeUncategorized,
  });

  // Build set of collapsed task IDs for lazy loading
  const collapsedTaskIds = buildCollapsedTaskIds(
    graphNodes,
    activePlanGroups,
    tierGroups,
    collapsedPlanIds,
    collapsedTierIds,
    includeUncategorized
  );

  // Generate inter-group connector edges to stack groups vertically
  const interGroupEdges = generateInterGroupEdges(
    graphNodes,
    activePlanGroups,
    includeUncategorized
  );
  const interTierEdges = generateInterTierEdges(tierGroups, collapsedPlanIds);

  // Filter nodes and edges to exclude collapsed groups (lazy loading)
  // This saves rendering for tasks that won't be shown
  const visibleNodes = graphNodes.filter((n) => !collapsedTaskIds.has(n.taskId));
  const visibleEdges = graphEdges.filter(
    (e) => !collapsedTaskIds.has(e.source) && !collapsedTaskIds.has(e.target)
  );

  // Inter-group edges connect group nodes directly, so they're always visible
  // (they don't depend on individual task visibility)

  // Compute layout for ALL nodes (needed for group bounding boxes)
  // We need positions for collapsed group tasks too, so groups remain visible
  const allNodeIds = graphNodes.map((n) => n.taskId);
  // Include inter-group edges for layout computation (so dagre stacks groups)
  // Use task-level edges (layoutSource/layoutTarget) for dagre, not group IDs
  const allEdgePairs = [
    ...graphEdges.map((e) => ({ source: e.source, target: e.target })),
    ...interGroupEdges.map((e) => ({ source: e.layoutSource, target: e.layoutTarget })),
  ];

  // Convert PlanGroupInfo to LayoutPlanGroup for layout computation
  const layoutPlanGroups: LayoutPlanGroup[] = activePlanGroups.map((pg) => ({
    planArtifactId: pg.planArtifactId,
    taskIds: pg.taskIds,
  }));
  const layoutTierGroups: LayoutTierGroup[] = tierGroups.map((tg) => ({
    tierGroupId: tg.id,
    planArtifactId: tg.planArtifactId,
    taskIds: tg.taskIds,
  }));

  const hash = computeGraphHash(
    allNodeIds,
    allEdgePairs,
    config,
    layoutPlanGroups,
    layoutTierGroups,
    collapsedPlanIds,
    collapsedTierIds,
    includeUncategorized,
    config.nodeModeLookup
  );

  // Check if we can use cached positions
  let positions: Map<string, { x: number; y: number }>;
  if (cache.current && cache.current.hash === hash) {
    // Cache hit: reuse positions
    positions = cache.current.positions;
  } else {
    // Cache miss: compute new layout using compound graph and cache it
    // Collapsed groups use minimal placeholder nodes
    positions = computePositions(
      allNodeIds,
      allEdgePairs,
      config,
      scaledDims,
      layoutPlanGroups,
      layoutTierGroups,
      collapsedPlanIds,
      collapsedTierIds,
      includeUncategorized,
      config.nodeModeLookup
    );
    cache.current = { hash, positions };
  }

  // Create a set of critical path nodes for quick lookup
  const criticalPathSet = new Set(criticalPath);

  // Determine handle positions based on layout direction
  const sourcePosition = config.direction === "TB" ? Position.Bottom : Position.Right;
  const targetPosition = config.direction === "TB" ? Position.Top : Position.Left;

  // Build maps of task ID -> status and task ID -> title for edge data
  const nodeStatusMap = new Map<string, string>();
  const nodeTitleMap = new Map<string, string>();
  graphNodes.forEach((node) => {
    nodeStatusMap.set(node.taskId, node.internalStatus);
    nodeTitleMap.set(node.taskId, node.title);
  });

  // Build a map of task ID -> plan artifact ID for cross-plan edge detection
  const taskToPlanMap = new Map<string, string>();
  for (const pg of planGroups) {
    for (const taskId of pg.taskIds) {
      taskToPlanMap.set(taskId, pg.planArtifactId);
    }
  }

  // Transform to React Flow nodes using cached/computed positions
  // Only process visible nodes (lazy loading - collapsed group tasks excluded)
  const nodes: Node[] = visibleNodes.map((graphNode) => {
    const pos = positions.get(graphNode.taskId) ?? { x: 0, y: 0 };
    const dims = getNodeDimensions(graphNode.taskId);

    const planId = graphNode.category === "plan_merge"
      ? graphNode.planArtifactId ?? taskToPlanMap.get(graphNode.taskId)
      : undefined;
    const planBranchContext = graphNode.category === "plan_merge" && planBranchMap
      ? planBranchMap.get(graphNode.taskId) ?? (planId ? planBranchMap.get(planId) : undefined)
      : undefined;

    return {
      id: graphNode.taskId,
      type: "task", // Use custom TaskNode component
      position: pos,
      width: dims.width,
      height: dims.height,
      data: {
        label: graphNode.title, // Full title - TaskNode handles truncation
        taskId: graphNode.taskId,
        description: graphNode.description,
        category: graphNode.category,
        internalStatus: graphNode.internalStatus,
        priority: graphNode.priority,
        isCriticalPath: criticalPathSet.has(graphNode.taskId),
        ...(planBranchContext?.mergeTarget !== undefined && { mergeTarget: planBranchContext.mergeTarget }),
        ...(planBranchContext?.prNumber != null && { prNumber: planBranchContext.prNumber }),
        ...(planBranchContext?.prStatus !== undefined && { prStatus: planBranchContext.prStatus }),
        ...(planBranchContext?.status !== undefined && { planBranchStatus: planBranchContext.status }),
      } satisfies TaskNodeData,
      sourcePosition,
      targetPosition,
    } as Node;
  });

  // Transform graph edges to React Flow edges
  // Only process visible edges (lazy loading - edges to/from collapsed tasks excluded)
  // Cross-plan edges (source and target in different plan groups) get higher z-index
  // to render on top of plan group regions
  const CROSS_PLAN_EDGE_ZINDEX = 10;

  const edges: Edge[] = visibleEdges.map((graphEdge) => {
    const sourceStatus = nodeStatusMap.get(graphEdge.source);
    const sourcePlan = taskToPlanMap.get(graphEdge.source);
    const targetPlan = taskToPlanMap.get(graphEdge.target);

    // Edge is cross-plan if source and target are in different plan groups
    // (or one is grouped and the other is ungrouped)
    const isCrossPlan = sourcePlan !== targetPlan;

    // Get titles for tooltip
    const sourceLabel = nodeTitleMap.get(graphEdge.source);
    const targetLabel = nodeTitleMap.get(graphEdge.target);

    const edgeData: DependencyEdgeData = {
      isCriticalPath: graphEdge.isCriticalPath,
      isCrossPlan,
    };
    // Only add optional fields if they exist
    if (sourceStatus !== undefined) {
      edgeData.sourceStatus = sourceStatus;
    }
    if (sourceLabel !== undefined) {
      edgeData.sourceLabel = sourceLabel;
    }
    if (targetLabel !== undefined) {
      edgeData.targetLabel = targetLabel;
    }

    const edge: Edge = {
      id: `${graphEdge.source}-${graphEdge.target}`,
      type: "dependency", // Use custom DependencyEdge component
      source: graphEdge.source,
      target: graphEdge.target,
      data: edgeData,
    };

    // Set higher z-index for cross-plan edges to ensure they render on top of groups
    if (isCrossPlan) {
      edge.zIndex = CROSS_PLAN_EDGE_ZINDEX;
    }

    return edge;
  });

  // Build a map of group ID -> session title for edge labels
  const groupTitleMap = new Map<string, string>();
  for (const pg of activePlanGroups) {
    groupTitleMap.set(pg.planArtifactId, pg.sessionTitle ?? "Plan");
  }
  if (includeUncategorized) {
    groupTitleMap.set(UNGROUPED_PLAN_ID, "Uncategorized");
  }

  // Add inter-group connector edges (connect group nodes directly for rendering)
  const GROUP_CONNECTOR_ZINDEX = 15; // Higher than regular cross-plan edges
  for (const interEdge of interGroupEdges) {
    const sourceLabel = groupTitleMap.get(interEdge.sourceGroupId) ?? interEdge.sourceGroupId;
    const targetLabel = groupTitleMap.get(interEdge.targetGroupId) ?? interEdge.targetGroupId;

    const edgeData: DependencyEdgeData = {
      isCriticalPath: false,
      isCrossPlan: true,
      isGroupConnector: true,
      sourceLabel,
      targetLabel,
    };

    const sourceNodeId = getPlanGroupNodeId(interEdge.sourceGroupId);
    const targetNodeId = getPlanGroupNodeId(interEdge.targetGroupId);

    edges.push({
      id: `group-connector-${interEdge.sourceGroupId}-${interEdge.targetGroupId}`,
      type: "dependency",
      source: sourceNodeId,
      target: targetNodeId,
      data: edgeData,
      zIndex: GROUP_CONNECTOR_ZINDEX,
    });
  }

  // Add inter-tier connector edges (connect tier group nodes directly)
  const TIER_CONNECTOR_ZINDEX = 12;
  for (const interEdge of interTierEdges) {
    const edgeData: DependencyEdgeData = {
      isCriticalPath: false,
      isCrossPlan: false,
      isGroupConnector: true,
      sourceLabel: interEdge.sourceLabel,
      targetLabel: interEdge.targetLabel,
    };

    edges.push({
      id: `tier-connector-${interEdge.sourceTierId}-${interEdge.targetTierId}`,
      type: "dependency",
      source: getTierGroupNodeId(interEdge.sourceTierId),
      target: getTierGroupNodeId(interEdge.targetTierId),
      data: edgeData,
      zIndex: TIER_CONNECTOR_ZINDEX,
    });
  }

  // Create ALL positioned nodes for group bounding box calculation
  // (includes collapsed group tasks that won't be rendered)
  const allPositionedNodes: Node[] = graphNodes.map((graphNode) => {
    const pos = positions.get(graphNode.taskId) ?? { x: 0, y: 0 };
    const dims = getNodeDimensions(graphNode.taskId);
    return {
      id: graphNode.taskId,
      type: "task",
      position: pos,
      width: dims.width,
      height: dims.height,
      data: {},
    } as Node;
  });

  // Create group nodes for plan groups using ALL positioned nodes
  const planGroupNodes = createGroupNodes(
    allPositionedNodes,
    activePlanGroups,
    collapsedPlanIds,
    tierGroups,
    collapsedTierIds,
    positions,
    graphNodes,
    defaultNodeWidth,
    defaultNodeHeight,
    onToggleCollapse,
    onToggleAllTiers,
    includeUncategorized,
    projectId,
    onNavigateToTask,
    onCancelAll
  );

  const planGroupBounds = new Map<string, { position: { x: number; y: number }; width: number }>();
  for (const groupNode of planGroupNodes) {
    if (!groupNode.data.isCollapsed) {
      planGroupBounds.set(groupNode.data.planArtifactId, {
        position: groupNode.position,
        width: groupNode.data.width,
      });
    }
  }

  // Center plan_merge tasks within their plan group (non-tiered mode)
  // For tiered mode, centering is handled in the tier recomputation below
  for (const pg of activePlanGroups) {
    const planTiers = tierGroups.filter((tg) => tg.planArtifactId === pg.planArtifactId);
    if (planTiers.length > 0) continue; // Skip — handled in tiered recomputation
    const bounds = planGroupBounds.get(pg.planArtifactId);
    if (!bounds) continue;
    const groupCenterX = bounds.position.x + bounds.width / 2;
    for (const taskId of pg.taskIds) {
      const graphNode = graphNodes.find((n) => n.taskId === taskId);
      if (graphNode?.category !== "plan_merge") continue;
      const { width: mergeNodeWidth } = getNodeDimensions(taskId);
      const centeredX = groupCenterX - mergeNodeWidth / 2;
      const visibleNode = nodes.find((n) => n.id === taskId);
      if (visibleNode) {
        visibleNode.position = { ...visibleNode.position, x: centeredX };
      }
      const allNode = allPositionedNodes.find((n) => n.id === taskId);
      if (allNode) {
        allNode.position = { ...allNode.position, x: centeredX };
      }
    }
  }

  const tierGroupNodes = createTierGroupNodes(
    allPositionedNodes,
    tierGroups,
    collapsedTierIds,
    collapsedPlanIds,
    planGroupBounds,
    positions,
    defaultNodeWidth,
    defaultNodeHeight,
    onToggleTierCollapse
  );

  // Compress vertical spacing for collapsed tiers to avoid huge gaps
  const tierGroupsByPlan = new Map<string, TierGroupNode[]>();
  const tierOriginalY = new Map<string, number>();
  for (const tierNode of tierGroupNodes) {
    tierOriginalY.set(tierNode.id, tierNode.position.y);
    const existing = tierGroupsByPlan.get(tierNode.data.planArtifactId);
    if (existing) {
      existing.push(tierNode);
    } else {
      tierGroupsByPlan.set(tierNode.data.planArtifactId, [tierNode]);
    }
  }

  for (const [, tierNodes] of tierGroupsByPlan) {
    const ordered = [...tierNodes].sort((a, b) => a.data.tier - b.data.tier);
    const minY = Math.min(...ordered.map((node) => node.position.y));
    let cursorY = Number.isFinite(minY) ? minY : 0;

    for (const tierNode of ordered) {
      const height = tierNode.data.isCollapsed
        ? COLLAPSED_TIER_HEIGHT
        : tierNode.data.height;
      tierNode.position = { ...tierNode.position, y: cursorY };
      cursorY = tierNode.position.y + height + TIER_STACK_GAP;
    }
  }

  const taskToTierGroupId = new Map<string, string>();
  for (const tierGroup of tierGroups) {
    for (const taskId of tierGroup.taskIds) {
      taskToTierGroupId.set(taskId, getTierGroupNodeId(tierGroup.id));
    }
  }
  const tierDelta = new Map<string, number>();
  for (const tierNode of tierGroupNodes) {
    const originalY = tierOriginalY.get(tierNode.id);
    if (originalY === undefined) continue;
    const delta = tierNode.position.y - originalY;
    if (delta !== 0) {
      tierDelta.set(tierNode.id, delta);
    }
  }

  if (tierDelta.size > 0) {
    for (const node of nodes) {
      const tierId = taskToTierGroupId.get(node.id);
      if (!tierId) continue;
      const delta = tierDelta.get(tierId);
      if (!delta) continue;
      node.position = { ...node.position, y: node.position.y + delta };
    }
    for (const node of allPositionedNodes) {
      const tierId = taskToTierGroupId.get(node.id);
      if (!tierId) continue;
      const delta = tierDelta.get(tierId);
      if (!delta) continue;
      node.position = { ...node.position, y: node.position.y + delta };
    }
  }

  // Center task nodes within their tier group lane
  const tierNodeMap = new Map<string, TierGroupNode>();
  for (const tierNode of tierGroupNodes) {
    tierNodeMap.set(tierNode.id, tierNode);
  }
  const tasksByTier = new Map<string, Node[]>();
  for (const node of allPositionedNodes) {
    const tierId = taskToTierGroupId.get(node.id);
    if (!tierId) continue;
    const bucket = tasksByTier.get(tierId);
    if (bucket) {
      bucket.push(node);
    } else {
      tasksByTier.set(tierId, [node]);
    }
  }

  for (const [tierId, tierTasks] of tasksByTier) {
    const tierNode = tierNodeMap.get(tierId);
    if (!tierNode || tierNode.data.isCollapsed) continue;
    const bbox = calculateBoundingBox(tierTasks, defaultNodeWidth, defaultNodeHeight);
    if (!bbox) continue;
    const desiredLeft = tierNode.position.x + (tierNode.data.width - bbox.width) / 2;
    const deltaX = desiredLeft - bbox.minX;
    if (deltaX === 0) continue;

    for (const node of tierTasks) {
      node.position = { ...node.position, x: node.position.x + deltaX };
    }
    for (const node of nodes) {
      if (taskToTierGroupId.get(node.id) !== tierId) continue;
      node.position = { ...node.position, x: node.position.x + deltaX };
    }
  }

  // If we have tiers, recompute plan group bounds using tier group nodes
  // AND include plan_merge tasks (which are excluded from tier groups)
  if (tierGroups.length > 0) {
    // Build a lookup of planArtifactId -> merge task nodes
    const planTaskMap = new Map<string, string[]>();
    for (const pg of activePlanGroups) {
      planTaskMap.set(pg.planArtifactId, pg.taskIds);
    }

    for (const planGroupNode of planGroupNodes) {
      if (planGroupNode.data.isCollapsed) continue;
      const tierNodes = tierGroupNodes.filter(
        (node) => node.data.planArtifactId === planGroupNode.data.planArtifactId
      );
      if (tierNodes.length === 0) continue;

      // Find plan_merge tasks for this plan (excluded from tier groups)
      const planTaskIds = planTaskMap.get(planGroupNode.data.planArtifactId) ?? [];
      const mergeTaskNodes: SizedNode[] = [];
      for (const taskId of planTaskIds) {
        const graphNode = graphNodes.find((n) => n.taskId === taskId);
        if (graphNode?.category !== "plan_merge") continue;
        const posNode = allPositionedNodes.find((n) => n.id === taskId);
        if (!posNode) continue;
        const mergeDims = getNodeDimensions(taskId);
        mergeTaskNodes.push({
          position: posNode.position,
          width: mergeDims.width,
          height: mergeDims.height,
        });
      }

      // Position merge tasks below the last tier, centered
      if (mergeTaskNodes.length > 0) {
        const lastTier = [...tierNodes].sort((a, b) => a.data.tier - b.data.tier).pop();
        if (lastTier) {
          const belowY = lastTier.position.y + lastTier.data.height + TIER_STACK_GAP;
          const tierCenterX = lastTier.position.x + lastTier.data.width / 2;
          for (const mergeNode of mergeTaskNodes) {
            mergeNode.position = {
              x: tierCenterX - mergeNode.width / 2,
              y: belowY,
            };
          }
          // Also update the actual task node positions
          for (const taskId of planTaskIds) {
            const graphNode = graphNodes.find((n) => n.taskId === taskId);
            if (graphNode?.category !== "plan_merge") continue;
            const mergePos = mergeTaskNodes[0]?.position;
            if (!mergePos) continue;
            // Update visible nodes
            const visibleNode = nodes.find((n) => n.id === taskId);
            if (visibleNode) {
              visibleNode.position = { ...mergePos };
            }
            // Update allPositionedNodes
            const allNode = allPositionedNodes.find((n) => n.id === taskId);
            if (allNode) {
              allNode.position = { ...mergePos };
            }
          }
        }
      }

      const sizedNodes: SizedNode[] = tierNodes.map((node) => ({
        position: node.position,
        width: node.data.width,
        height: node.data.height,
      }));
      // Include merge task nodes in bounding box calculation
      sizedNodes.push(...mergeTaskNodes);

      const bbox = calculateSizedBoundingBox(sizedNodes);
      if (!bbox) continue;
      const expanded = expandBoundingBox(bbox, GROUP_PADDING, HEADER_HEIGHT);
      const groupDims = boundingBoxToGroupNode(expanded);
      // Keep existing plan width so full-width tier lanes don't recursively expand the plan container.
      // We only refresh vertical geometry from the tier stack.
      const preservedWidth = planGroupNode.data.width;
      planGroupNode.position = {
        x: planGroupNode.position.x,
        y: groupDims.position.y,
      };
      planGroupNode.data.width = preservedWidth;
      planGroupNode.data.height = groupDims.height;
      planGroupNode.style = {
        ...planGroupNode.style,
        width: preservedWidth,
        height: groupDims.height,
      };
    }
  }

  const groupNodes = [...planGroupNodes, ...tierGroupNodes];

  return { nodes, edges, groupNodes };
}


// ============================================================================
// Hook
// ============================================================================

/**
 * Hook to compute dagre-based hierarchical layout for task graph
 *
 * Uses layout caching to avoid expensive dagre recomputation when only node data
 * (status, title, priority) changes but graph structure (nodes, edges) remains the same.
 *
 * @param nodes - Task graph nodes from API
 * @param edges - Task graph edges from API
 * @param criticalPath - Array of task IDs on the critical path
 * @param planGroups - Plan group info for visual grouping
 * @param config - Optional layout configuration overrides
 * @param collapsedPlanIds - Set of plan artifact IDs that are collapsed
 * @param onToggleCollapse - Callback when collapse is toggled
 * @returns React Flow nodes, edges, and group nodes with computed positions
 *
 * @example
 * ```tsx
 * const [collapsedPlanIds, setCollapsedPlanIds] = useState<Set<string>>(new Set());
 * const handleToggleCollapse = (planArtifactId: string) => {
 *   setCollapsedPlanIds(prev => {
 *     const next = new Set(prev);
 *     if (next.has(planArtifactId)) next.delete(planArtifactId);
 *     else next.add(planArtifactId);
 *     return next;
 *   });
 * };
 *
 * const { nodes, edges, groupNodes } = useTaskGraphLayout(
 *   graphData.nodes,
 *   graphData.edges,
 *   graphData.criticalPath,
 *   graphData.planGroups,
 *   { direction: "LR" },
 *   collapsedPlanIds,
 *   handleToggleCollapse
 * );
 *
 * // Combine task nodes and group nodes for React Flow
 * const allNodes = [...groupNodes, ...nodes];
 * return <ReactFlow nodes={allNodes} edges={edges} />;
 * ```
 */
export function useTaskGraphLayout(
  graphNodes: TaskGraphNode[],
  graphEdges: TaskGraphEdge[],
  criticalPath: string[],
  planGroups: PlanGroupInfo[] = [],
  grouping: GroupingConfig = { byPlan: true, byTier: true, showUncategorized: true },
  config: Partial<LayoutConfig> = {},
  collapsedPlanIds: Set<string> = new Set(),
  collapsedTierIds: Set<string> = new Set(),
  onToggleCollapse?: (planArtifactId: string) => void,
  onToggleTierCollapse?: (tierGroupId: string) => void,
  onToggleAllTiers?: (planArtifactId: string, action: "expand" | "collapse") => void,
  projectId?: string,
  onNavigateToTask?: (taskId: string) => void,
  onCancelAll?: (sessionId: string) => void,
  planBranchMap?: Map<string, PlanBranchNodeContext>
): LayoutResult {
  // Merge with default config
  const fullConfig = useMemo(
    () => ({ ...DEFAULT_CONFIG, ...config }),
    [config]
  );

  // Layout cache - persists across renders, reused when graph structure unchanged
  const layoutCache = useRef<CachedLayout | null>(null);

  // Scale node dimensions with the active font-scale toggle so 110%/125%
  // text doesn't overflow the fixed-pixel React Flow nodes.
  const fontScale = useThemeStore((s) => s.fontScale);
  const scaledDims = useMemo(() => getScaledNodeDimensions(fontScale), [fontScale]);
  const lastFontScale = useRef(fontScale);

  // Compute layout using cache when structure is unchanged
  /* eslint-disable react-hooks/refs -- layoutCache is intentionally read during render to reuse structural dagre output and avoid graph relayout churn */
  const layout = useMemo(() => {
    if (graphNodes.length === 0) {
      return { nodes: [], edges: [], groupNodes: [] };
    }
    // Invalidate the structural cache when font scale changes so positions
    // get recomputed against the new node dimensions.
    if (lastFontScale.current !== fontScale) {
      layoutCache.current = null;
      lastFontScale.current = fontScale;
    }
    return computeLayoutWithCache(
      graphNodes,
      graphEdges,
      criticalPath,
      planGroups,
      grouping,
      fullConfig,
      collapsedPlanIds,
      collapsedTierIds,
      onToggleCollapse,
      onToggleTierCollapse,
      onToggleAllTiers,
      layoutCache,
      scaledDims,
      projectId,
      onNavigateToTask,
      onCancelAll,
      planBranchMap
    );
  }, [
    graphNodes,
    graphEdges,
    criticalPath,
    planGroups,
    grouping,
    fullConfig,
    collapsedPlanIds,
    collapsedTierIds,
    onToggleCollapse,
    onToggleTierCollapse,
    onToggleAllTiers,
    scaledDims,
    fontScale,
    projectId,
    onNavigateToTask,
    onCancelAll,
    planBranchMap,
  ]);
  /* eslint-enable react-hooks/refs */

  return layout;
}

// ============================================================================
// Layout Cache
// ============================================================================

/**
 * Cached layout positions from dagre computation.
 * Only stores structural layout info - node data is applied fresh each time.
 */
interface CachedLayout {
  hash: string;
  positions: Map<string, { x: number; y: number }>;
}

/**
 * Plan group info for compound graph layout
 */
interface LayoutPlanGroup {
  planArtifactId: string;
  taskIds: string[];
}

interface LayoutTierGroup {
  tierGroupId: string;
  planArtifactId: string;
  taskIds: string[];
}

/**
 * Compute a structural hash of the graph for cache key.
 * Hash includes: node IDs (sorted), edge pairs (sorted), layout spacing config,
 * compact mode, plan groups, and collapsed state.
 * Does NOT include node data (status, title, priority) since those don't affect layout.
 */
function computeGraphHash(
  nodeIds: string[],
  edges: { source: string; target: string }[],
  config: Pick<LayoutConfig, "direction" | "nodesep" | "ranksep" | "marginx" | "marginy">,
  planGroups: LayoutPlanGroup[] = [],
  tierGroups: LayoutTierGroup[] = [],
  collapsedPlanIds: Set<string> = new Set(),
  collapsedTierIds: Set<string> = new Set(),
  includeUncategorized: boolean = true,
  nodeModeLookup?: Map<string, NodeMode>
): string {
  // Sort for consistent ordering
  const sortedNodes = [...nodeIds].sort().join(",");
  const sortedEdges = [...edges]
    .map((e) => `${e.source}>${e.target}`)
    .sort()
    .join(",");
  // Include plan group assignments in hash (affects compound layout)
  const sortedGroups = [...planGroups]
    .map((g) => `${g.planArtifactId}:[${[...g.taskIds].sort().join(",")}]`)
    .sort()
    .join(";");
  const sortedTierGroups = [...tierGroups]
    .map((g) => `${g.tierGroupId}:${g.planArtifactId}:[${[...g.taskIds].sort().join(",")}]`)
    .sort()
    .join(";");
  // Include collapsed state so layout recalculates when groups collapse/expand
  const sortedCollapsed = [...collapsedPlanIds].sort().join(",");
  const sortedCollapsedTiers = [...collapsedTierIds].sort().join(",");
  // Include per-node modes so layout recalculates when mode changes
  const modeHash = nodeModeLookup
    ? [...nodeModeLookup.entries()].filter(([, m]) => m === "compact").map(([id]) => id).sort().join(",")
    : "standard";
  const spacingHash = `${config.direction}:${config.nodesep}:${config.ranksep}:${config.marginx}:${config.marginy}`;
  const layoutVersion = "layout-v3";
  return `${layoutVersion}:${spacingHash}:${modeHash}:${sortedNodes}|${sortedEdges}|${sortedGroups}|${sortedTierGroups}|${sortedCollapsed}|${sortedCollapsedTiers}|${includeUncategorized ? "uncategorized" : "no-uncategorized"}`;
}

/**
 * Compute layout positions using dagre with compound graph support.
 * When planGroups are provided, uses compound graph to keep grouped nodes together
 * and prevent group overlap.
 *
 * Collapsed groups use a single placeholder node for minimal space.
 *
 * Returns position maps for both task nodes and group parent nodes.
 */
function computePositions(
  nodeIds: string[],
  edges: { source: string; target: string }[],
  config: LayoutConfig,
  scaledDims: { nodeWidth: number; nodeHeight: number; compactNodeWidth: number; compactNodeHeight: number },
  planGroups: LayoutPlanGroup[] = [],
  tierGroups: LayoutTierGroup[] = [],
  collapsedPlanIds: Set<string> = new Set(),
  collapsedTierIds: Set<string> = new Set(),
  includeUncategorized: boolean = true,
  nodeModeLookup?: Map<string, NodeMode>
): Map<string, { x: number; y: number }> {
  // Per-node dimensions helper
  const getNodeDims = (id: string) => {
    const mode = nodeModeLookup?.get(id) ?? "standard";
    return {
      width: mode === "compact" ? scaledDims.compactNodeWidth : scaledDims.nodeWidth,
      height: mode === "compact" ? scaledDims.compactNodeHeight : scaledDims.nodeHeight,
    };
  };

  // Use compound graph when we have plan groups or tier groups to prevent overlap
  const useCompound = planGroups.length > 0 || tierGroups.length > 0;
  const g = new dagre.graphlib.Graph({ compound: useCompound });
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({
    rankdir: config.direction,
    nodesep: config.nodesep,
    ranksep: config.ranksep,
    marginx: config.marginx,
    marginy: config.marginy,
  });

  // Build task/group mappings and identify collapsed tasks
  const taskToPlanMap = new Map<string, string>();
  const taskToTierMap = new Map<string, string>();
  const collapsedTaskIds = new Set<string>();

  if (useCompound) {
    for (const group of planGroups) {
      const isCollapsed = collapsedPlanIds.has(group.planArtifactId);
      for (const taskId of group.taskIds) {
        taskToPlanMap.set(taskId, group.planArtifactId);
        if (isCollapsed) {
          collapsedTaskIds.add(taskId);
        }
      }
    }

    // Add parent nodes for each plan group
    for (const group of planGroups) {
      g.setNode(group.planArtifactId, { label: group.planArtifactId });
    }

    // Find ungrouped tasks and create a pseudo-group for them
    const groupedTaskIds = new Set<string>();
    for (const group of planGroups) {
      for (const taskId of group.taskIds) {
        groupedTaskIds.add(taskId);
      }
    }
    const ungroupedTaskIds = nodeIds.filter((id) => !groupedTaskIds.has(id));

    const isUngroupedCollapsed = collapsedPlanIds.has(UNGROUPED_PLAN_ID);
    if (includeUncategorized && ungroupedTaskIds.length > 0) {
      g.setNode(UNGROUPED_PLAN_ID, { label: UNGROUPED_PLAN_ID });
      for (const taskId of ungroupedTaskIds) {
        taskToPlanMap.set(taskId, UNGROUPED_PLAN_ID);
        if (isUngroupedCollapsed) {
          collapsedTaskIds.add(taskId);
        }
      }
    }

    // Add tier group nodes and map tasks to tiers
    for (const tierGroup of tierGroups) {
      if (collapsedPlanIds.has(tierGroup.planArtifactId)) {
        continue;
      }
      g.setNode(tierGroup.tierGroupId, { label: tierGroup.tierGroupId });
      g.setParent(tierGroup.tierGroupId, tierGroup.planArtifactId);

      const isTierCollapsed = collapsedTierIds.has(tierGroup.tierGroupId);
      const isPlanCollapsed = collapsedPlanIds.has(tierGroup.planArtifactId);
      for (const taskId of tierGroup.taskIds) {
        taskToTierMap.set(taskId, tierGroup.tierGroupId);
        if (isTierCollapsed && !isPlanCollapsed) {
          collapsedTaskIds.add(taskId);
        }
      }
    }
  }

  // Add task nodes - skip collapsed group tasks, add placeholder instead
  const collapsedPlanPlaceholders = new Set<string>();
  const collapsedTierPlaceholders = new Set<string>();
  for (const id of nodeIds) {
    if (collapsedTaskIds.has(id)) {
      const planId = taskToPlanMap.get(id);
      const tierId = taskToTierMap.get(id);

      if (planId && collapsedPlanIds.has(planId)) {
        if (!collapsedPlanPlaceholders.has(planId)) {
          const placeholderId = `__collapsed_placeholder_${planId}__`;
          g.setNode(placeholderId, { width: COLLAPSED_GROUP_WIDTH, height: 40 });
          g.setParent(placeholderId, planId);
          collapsedPlanPlaceholders.add(planId);
        }
        continue;
      }

      if (tierId && collapsedTierIds.has(tierId)) {
        if (!collapsedTierPlaceholders.has(tierId)) {
          const placeholderId = `__collapsed_tier_placeholder_${tierId}__`;
          g.setNode(placeholderId, { width: COLLAPSED_TIER_WIDTH, height: 32 });
          g.setParent(placeholderId, tierId);
          collapsedTierPlaceholders.add(tierId);
        }
        continue;
      }
    }

    const dims = getNodeDims(id);
    g.setNode(id, { width: dims.width, height: dims.height });

    // Set parent relationship for compound graph
    if (useCompound) {
      const parentTierId = taskToTierMap.get(id);
      if (parentTierId) {
        g.setParent(id, parentTierId);
      } else {
        const parentPlanId = taskToPlanMap.get(id);
        if (parentPlanId) {
          g.setParent(id, parentPlanId);
        }
      }
    }
  }

  // Add edges - for collapsed tasks, redirect to placeholder nodes
  for (const edge of edges) {
    const source = edge.source;
    const target = edge.target;

    const resolvePlaceholder = (taskId: string): string | null => {
      if (!collapsedTaskIds.has(taskId)) return taskId;

      const planId = taskToPlanMap.get(taskId);
      const tierId = taskToTierMap.get(taskId);

      if (planId && collapsedPlanPlaceholders.has(planId)) {
        return `__collapsed_placeholder_${planId}__`;
      }
      if (tierId && collapsedTierPlaceholders.has(tierId)) {
        return `__collapsed_tier_placeholder_${tierId}__`;
      }
      return null;
    };

    const resolvedSource = resolvePlaceholder(source);
    const resolvedTarget = resolvePlaceholder(target);
    if (!resolvedSource || !resolvedTarget) continue;
    if (resolvedSource === resolvedTarget) continue;

    g.setEdge(resolvedSource, resolvedTarget);
  }

  // Run dagre layout
  dagre.layout(g);

  // Extract positions for task nodes
  const positions = new Map<string, { x: number; y: number }>();
  for (const id of nodeIds) {
    const dagreNode = g.node(id);
    if (dagreNode) {
      // Dagre gives center position, React Flow needs top-left
      const dims = getNodeDims(id);
      positions.set(id, {
        x: dagreNode.x - dims.width / 2,
        y: dagreNode.y - dims.height / 2,
      });
    }
  }

  // Store placeholder positions for collapsed groups (used for group bounding boxes)
  for (const groupId of collapsedPlanPlaceholders) {
    const placeholderId = `__collapsed_placeholder_${groupId}__`;
    const placeholderNode = g.node(placeholderId);
    if (placeholderNode) {
      positions.set(`__group_position_${groupId}__`, {
        x: placeholderNode.x - COLLAPSED_GROUP_WIDTH / 2,
        y: placeholderNode.y - 20,
      });
    }
  }

  for (const tierId of collapsedTierPlaceholders) {
    const placeholderId = `__collapsed_tier_placeholder_${tierId}__`;
    const placeholderNode = g.node(placeholderId);
    if (placeholderNode) {
      positions.set(`__tier_group_position_${tierId}__`, {
        x: placeholderNode.x - COLLAPSED_TIER_WIDTH / 2,
        y: placeholderNode.y - 16,
      });
    }
  }

  return positions;
}

// Export default config for use in controls
export { DEFAULT_CONFIG };
