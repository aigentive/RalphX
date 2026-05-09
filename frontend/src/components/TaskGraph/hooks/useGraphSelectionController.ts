import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useStore, type Edge, type Node } from "@xyflow/react";
import type { GraphSelection } from "@/stores/uiStore";
import { useUiStore } from "@/stores/uiStore";
import type { PlanGroupInfo } from "@/api/task-graph.types";
import type { GroupingState } from "../controls/GraphControls";
import {
  getPlanGroupNodeId,
  getTierGroupNodeId,
  parseGroupNodeId,
} from "../groups/groupTypes";
import { PLAN_GROUP_NODE_TYPE, type PlanGroupData } from "../groups/PlanGroup";
import { TIER_GROUP_NODE_TYPE, type TierGroupData } from "../groups/TierGroup";
import type { TierGroupInfo } from "../groups/tierGroupUtils";

/** Duration in ms before clearing the highlighted node */
const HIGHLIGHT_TIMEOUT_MS = 3000;
const DOUBLE_CLICK_DELAY_MS = 140;

/** Dependencies required to manage graph selection + navigation behavior. */
interface GraphSelectionControllerParams {
  nodes: Node[];
  edges: Edge[];
  layoutNodes: Node[];
  groupNodes: Node[];
  planGroups: PlanGroupInfo[];
  tierGroups: TierGroupInfo[];
  grouping: GroupingState;
  collapsedPlanIds: Set<string>;
  collapsedTierIds: Set<string>;
  onToggleCollapse: (planArtifactId: string) => void;
  onToggleTierCollapse: (tierGroupId: string) => void;
  onToggleAllTiers: (planArtifactId: string, action: "expand" | "collapse") => void;
  onTaskSelect?: (taskId: string) => void;
  centerOnPlanGroup: (planArtifactId: string, duration?: number, zoom?: number) => boolean;
  centerOnNode: (
    nodeId: string,
    options?: { duration?: number; zoom?: number; fallbackWidth?: number; fallbackHeight?: number }
  ) => boolean;
  centerOnNodeObject: (
    node: Node,
    options?: { duration?: number; zoom?: number; fallbackWidth?: number; fallbackHeight?: number }
  ) => void;
  fitNode?: (
    node: Node,
    options?: { duration?: number; padding?: number; maxZoom?: number }
  ) => void;
  fitViewDefault: (options?: { duration?: number; padding?: number }) => void;
  zoomBy: (delta: number, options?: { duration?: number; minZoom?: number; maxZoom?: number }) => boolean;
  graphReady: boolean;
  graphError: Error | null;
  isLoading: boolean;
  /** Disable keyboard navigation/shortcuts (used while battle overlay is active) */
  keyboardNavigationEnabled?: boolean;
}

/** Public controller surface returned to TaskGraphView. */
interface GraphSelectionControllerResult {
  focusedNodeId: string | null;
  highlightedTaskId: string | null;
  graphSelection: GraphSelection | null;
  /** Set selection + focus viewport in one call. Stable ref — safe in effects.
   *  Pass `{ skipFocus: true }` to set selection without viewport animation. */
  select: (selection: GraphSelection, options?: { skipFocus?: boolean }) => void;
  focusSelectionInView: (selection: GraphSelection) => void;
  containerRef: React.RefObject<HTMLDivElement | null>;
  onNodeClick: (event: React.MouseEvent, node: Node) => void;
  onNodeDoubleClick: (event: React.MouseEvent, node: Node) => void;
  onPaneClick: () => void;
  onTimelineTaskClick: (taskId: string) => void;
  onKeyDown: (event: React.KeyboardEvent<HTMLDivElement>) => void;
}

function findNextNode(
  direction: "up" | "down" | "left" | "right",
  currentNodeId: string,
  nodes: Node[],
  edges: Edge[]
): string | null {
  const taskNodes = nodes.filter((n) => n.type === "task" || n.type === undefined);
  const currentNode = taskNodes.find((n) => n.id === currentNodeId);
  if (!currentNode) return null;

  if (direction === "up") {
    const sourceIds = edges.filter((e) => e.target === currentNodeId).map((e) => e.source);
    if (sourceIds.length === 0) return null;
    const sourceNodes = taskNodes.filter((n) => sourceIds.includes(n.id));
    if (sourceNodes.length === 0) return null;
    return sourceNodes.reduce((closest, node) =>
      Math.abs(node.position.x - currentNode.position.x) <
      Math.abs(closest.position.x - currentNode.position.x)
        ? node
        : closest
    ).id;
  }

  if (direction === "down") {
    const targetIds = edges.filter((e) => e.source === currentNodeId).map((e) => e.target);
    if (targetIds.length === 0) return null;
    const targetNodes = taskNodes.filter((n) => targetIds.includes(n.id));
    if (targetNodes.length === 0) return null;
    return targetNodes.reduce((closest, node) =>
      Math.abs(node.position.x - currentNode.position.x) <
      Math.abs(closest.position.x - currentNode.position.x)
        ? node
        : closest
    ).id;
  }

  const tolerance = 40;
  const siblingNodes = taskNodes.filter(
    (n) => n.id !== currentNodeId && Math.abs(n.position.y - currentNode.position.y) < tolerance
  );
  if (siblingNodes.length === 0) return null;

  if (direction === "left") {
    const leftNodes = siblingNodes.filter((n) => n.position.x < currentNode.position.x);
    if (leftNodes.length === 0) return null;
    return leftNodes.reduce((nearest, node) =>
      node.position.x > nearest.position.x ? node : nearest
    ).id;
  }

  if (direction === "right") {
    const rightNodes = siblingNodes.filter((n) => n.position.x > currentNode.position.x);
    if (rightNodes.length === 0) return null;
    return rightNodes.reduce((nearest, node) =>
      node.position.x < nearest.position.x ? node : nearest
    ).id;
  }

  return null;
}

function findNextGroupNode(
  direction: "up" | "down" | "left" | "right",
  currentNodeId: string,
  groupNodes: Node[]
): string | null {
  const currentNode = groupNodes.find((n) => n.id === currentNodeId);
  if (!currentNode) return null;

  const candidates = groupNodes.filter((n) => {
    if (n.id === currentNodeId) return false;
    if (direction === "up") return n.position.y < currentNode.position.y;
    if (direction === "down") return n.position.y > currentNode.position.y;
    if (direction === "left") return n.position.x < currentNode.position.x;
    return n.position.x > currentNode.position.x;
  });
  if (candidates.length === 0) return null;

  return candidates.reduce((closest, node) => {
    const primaryDelta =
      direction === "up" || direction === "down"
        ? Math.abs(node.position.y - currentNode.position.y)
        : Math.abs(node.position.x - currentNode.position.x);
    const closestPrimary =
      direction === "up" || direction === "down"
        ? Math.abs(closest.position.y - currentNode.position.y)
        : Math.abs(closest.position.x - currentNode.position.x);

    if (primaryDelta !== closestPrimary) {
      return primaryDelta < closestPrimary ? node : closest;
    }

    const secondaryDelta =
      direction === "up" || direction === "down"
        ? Math.abs(node.position.x - currentNode.position.x)
        : Math.abs(node.position.y - currentNode.position.y);
    const closestSecondary =
      direction === "up" || direction === "down"
        ? Math.abs(closest.position.x - currentNode.position.x)
        : Math.abs(closest.position.y - currentNode.position.y);

    return secondaryDelta < closestSecondary ? node : closest;
  }).id;
}

function sortNodesByPosition(a: Node, b: Node): number {
  if (a.position.y === b.position.y) {
    return a.position.x - b.position.x;
  }
  return a.position.y - b.position.y;
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tagName = target.tagName.toLowerCase();
  return tagName === "input" || tagName === "textarea" || target.isContentEditable;
}

function hasHandledGraphKey(event: { nativeEvent?: KeyboardEvent } | KeyboardEvent): boolean {
  const nativeEvent = "nativeEvent" in event ? event.nativeEvent : event;
  if (!nativeEvent) return false;
  const marker = nativeEvent as KeyboardEvent & { __graphHandled?: boolean };
  if (marker.__graphHandled) return true;
  marker.__graphHandled = true;
  return false;
}

function isNavigableGraphSelection(
  selection: GraphSelection | null
): selection is { kind: "task" | "planGroup" | "tierGroup"; id: string } {
  return Boolean(
    selection &&
      (selection.kind === "task" ||
        selection.kind === "planGroup" ||
        selection.kind === "tierGroup")
  );
}

/**
 * Orchestrates graph selection, keyboard navigation, and viewport focus.
 * Keeps global `graphSelection` in the UI store and local focus/highlight state here.
 */
export function useGraphSelectionController({
  nodes,
  edges,
  layoutNodes,
  groupNodes,
  planGroups,
  tierGroups,
  grouping,
  collapsedPlanIds,
  collapsedTierIds,
  onToggleCollapse,
  onToggleTierCollapse,
  onToggleAllTiers,
  onTaskSelect,
  centerOnPlanGroup,
  centerOnNode,
  centerOnNodeObject,
  fitNode,
  fitViewDefault,
  zoomBy,
  graphReady,
  graphError,
  isLoading,
  keyboardNavigationEnabled = true,
}: GraphSelectionControllerParams): GraphSelectionControllerResult {
  const selectedTaskId = useUiStore((s) => s.selectedTaskId);
  const setSelectedTaskId = useUiStore((s) => s.setSelectedTaskId);
  const graphSelection = useUiStore((s) => s.graphSelection);
  const setGraphSelection = useUiStore((s) => s.setGraphSelection);
  const clearGraphSelection = useUiStore((s) => s.clearGraphSelection);
  const reactFlowDomNode = useStore((state) => state.domNode);

  const [highlightedTaskId, setHighlightedTaskId] = useState<string | null>(null);
  const [focusedNodeId, setFocusedNodeId] = useState<string | null>(null);

  const highlightTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const groupClickTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  const openTaskDetails = useCallback(
    (taskId: string) => {
      if (onTaskSelect) {
        onTaskSelect(taskId);
        return;
      }
      setSelectedTaskId(taskId);
    },
    [onTaskSelect, setSelectedTaskId]
  );

  const planGroupsById = useMemo(() => {
    const map = new Map<string, PlanGroupInfo>();
    for (const planGroup of planGroups) {
      map.set(planGroup.planArtifactId, planGroup);
    }
    return map;
  }, [planGroups]);

  const taskToPlanMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const planGroup of planGroups) {
      for (const taskId of planGroup.taskIds) {
        map.set(taskId, planGroup.planArtifactId);
      }
    }
    return map;
  }, [planGroups]);

  const taskToTierMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const tierGroup of tierGroups) {
      for (const taskId of tierGroup.taskIds) {
        map.set(taskId, tierGroup.id);
      }
    }
    return map;
  }, [tierGroups]);

  const tierGroupsById = useMemo(() => {
    const map = new Map<string, TierGroupInfo>();
    for (const tierGroup of tierGroups) {
      map.set(tierGroup.id, tierGroup);
    }
    return map;
  }, [tierGroups]);

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

  const taskNodesById = useMemo(() => {
    const map = new Map<string, Node>();
    for (const node of layoutNodes) {
      map.set(node.id, node);
    }
    return map;
  }, [layoutNodes]);

  const graphNodesById = useMemo(() => {
    const map = new Map<string, Node>();
    for (const node of groupNodes) {
      map.set(node.id, node);
    }
    for (const node of layoutNodes) {
      map.set(node.id, node);
    }
    return map;
  }, [groupNodes, layoutNodes]);

  const planGroupNodes = useMemo(
    () => groupNodes.filter((node) => node.type === PLAN_GROUP_NODE_TYPE),
    [groupNodes]
  );
  const tierGroupNodes = useMemo(
    () => groupNodes.filter((node) => node.type === TIER_GROUP_NODE_TYPE),
    [groupNodes]
  );
  const planGroupNodesSorted = useMemo(
    () => [...planGroupNodes].sort(sortNodesByPosition),
    [planGroupNodes]
  );

  const getFirstTaskNode = useCallback(
    (taskIds: string[]): Node | null => {
      const candidates = taskIds
        .map((taskId) => taskNodesById.get(taskId))
        .filter((node): node is Node => Boolean(node));
      if (candidates.length === 0) return null;
      return [...candidates].sort(sortNodesByPosition)[0] ?? null;
    },
    [taskNodesById]
  );

  const getFirstTaskInPlan = useCallback(
    (planArtifactId: string): Node | null => {
      const planGroup = planGroupsById.get(planArtifactId);
      if (!planGroup) return null;
      return getFirstTaskNode(planGroup.taskIds);
    },
    [getFirstTaskNode, planGroupsById]
  );

  const getFirstTaskInTier = useCallback(
    (tierGroupId: string): Node | null => {
      const tierGroup = tierGroupsById.get(tierGroupId);
      if (!tierGroup) return null;
      return getFirstTaskNode(tierGroup.taskIds);
    },
    [getFirstTaskNode, tierGroupsById]
  );

  const getTierGroupNodesForPlan = useCallback(
    (planArtifactId: string): Node[] =>
      tierGroupNodes.filter(
        (node) => (node.data as TierGroupData | undefined)?.planArtifactId === planArtifactId
      ),
    [tierGroupNodes]
  );

  const focusSelectionInView = useCallback(
    (selection: GraphSelection): void => {
      if (selection.kind === "customGroup") return;
      const nodeId =
        selection.kind === "planGroup"
          ? getPlanGroupNodeId(selection.id)
          : selection.kind === "tierGroup"
            ? getTierGroupNodeId(selection.id)
            : selection.id;

      const runFocus = () => {
        const node = graphNodesById.get(nodeId);
        if (node) {
          if (selection.kind === "planGroup") {
            // Keep the activation zoom effect, but cap zoom using fit-to-view bounds.
            if (fitNode) {
              fitNode(node, { duration: 200, padding: 0.18, maxZoom: 0.9 });
            } else {
              centerOnNodeObject(node, { duration: 200, zoom: 0.9, fallbackWidth: 320, fallbackHeight: 120 });
            }
          } else if (selection.kind === "tierGroup") {
            // Tier groups can be tall/wide; cap zoom to fit viewport.
            if (fitNode) {
              fitNode(node, { duration: 200, padding: 0.16, maxZoom: 0.95 });
            } else {
              centerOnNodeObject(node, { duration: 200, zoom: 0.95, fallbackWidth: 180, fallbackHeight: 60 });
            }
          } else {
            centerOnNodeObject(node, { duration: 200, zoom: 0.95, fallbackWidth: 180, fallbackHeight: 60 });
          }
          return;
        }
        if (selection.kind === "planGroup") {
          if (centerOnPlanGroup(selection.id, 200, 0.9)) return;
        }
        centerOnNode(nodeId, { duration: 200, zoom: 0.95, fallbackWidth: 180, fallbackHeight: 60 });
      };

      requestAnimationFrame(() => {
        requestAnimationFrame(runFocus);
      });
    },
    [
      centerOnNode,
      centerOnNodeObject,
      fitNode,
      centerOnPlanGroup,
      graphNodesById,
    ]
  );

  // Ref-stabilize focusSelectionInView so `select` has a stable identity.
  // focusSelectionInView changes when graphNodesById changes (every layout),
  // but select() must be safe to use in effects without causing re-fires.
  const focusSelectionRef = useRef(focusSelectionInView);
  useEffect(() => {
    focusSelectionRef.current = focusSelectionInView;
  }, [focusSelectionInView]);

  /** Set selection state AND focus viewport — stable callback, safe for effects.
   *  Pass `skipFocus: true` to set selection without viewport animation. */
  const select = useCallback(
    (selection: GraphSelection, options?: { skipFocus?: boolean }): void => {
      setGraphSelection(selection);
      if (!options?.skipFocus) {
        focusSelectionRef.current(selection);
      }
    },
    [setGraphSelection]
  );

  const handleTimelineTaskClick = useCallback(
    (taskId: string) => {
      if (highlightTimeoutRef.current) {
        clearTimeout(highlightTimeoutRef.current);
        highlightTimeoutRef.current = null;
      }

      setHighlightedTaskId(taskId);
      setGraphSelection({ kind: "task", id: taskId });
      focusSelectionInView({ kind: "task", id: taskId });

      highlightTimeoutRef.current = setTimeout(() => {
        setHighlightedTaskId(null);
        highlightTimeoutRef.current = null;
      }, HIGHLIGHT_TIMEOUT_MS);
    },
    [focusSelectionInView, setGraphSelection]
  );

  const handleNodeClick = useCallback(
    (event: React.MouseEvent, node: Node) => {
      if (node.type === PLAN_GROUP_NODE_TYPE) {
        if (event.detail > 1) {
          return;
        }
        const parsed = parseGroupNodeId(node.id);
        const planArtifactId =
          (node.data as PlanGroupData | undefined)?.planArtifactId ??
          (parsed?.kind === "plan" ? parsed.id : undefined);
        if (planArtifactId) {
          setGraphSelection({ kind: "planGroup", id: planArtifactId });
          focusSelectionInView({ kind: "planGroup", id: planArtifactId });
        }
        if (groupClickTimeoutRef.current) {
          clearTimeout(groupClickTimeoutRef.current);
        }
        groupClickTimeoutRef.current = setTimeout(() => {
          groupClickTimeoutRef.current = null;
        }, DOUBLE_CLICK_DELAY_MS);
        return;
      }
      if (node.type === TIER_GROUP_NODE_TYPE) {
        if (event.detail > 1) {
          return;
        }
        const parsed = parseGroupNodeId(node.id);
        const tierGroupId =
          (node.data as TierGroupData | undefined)?.tierGroupId ??
          (parsed?.kind === "tier" ? parsed.id : undefined);
        if (tierGroupId) {
          setGraphSelection({ kind: "tierGroup", id: tierGroupId });
          focusSelectionInView({ kind: "tierGroup", id: tierGroupId });
        }
        if (groupClickTimeoutRef.current) {
          clearTimeout(groupClickTimeoutRef.current);
        }
        groupClickTimeoutRef.current = setTimeout(() => {
          groupClickTimeoutRef.current = null;
        }, DOUBLE_CLICK_DELAY_MS);
        return;
      }

      if (highlightTimeoutRef.current) {
        clearTimeout(highlightTimeoutRef.current);
        highlightTimeoutRef.current = null;
      }

      setGraphSelection({ kind: "task", id: node.id });
      setHighlightedTaskId(node.id);
      setFocusedNodeId(node.id);
      focusSelectionInView({ kind: "task", id: node.id });

      highlightTimeoutRef.current = setTimeout(() => {
        setHighlightedTaskId(null);
        highlightTimeoutRef.current = null;
      }, HIGHLIGHT_TIMEOUT_MS);
    },
    [focusSelectionInView, setGraphSelection]
  );

  const handleNodeDoubleClick = useCallback(
    (_: React.MouseEvent, node: Node) => {
      if (highlightTimeoutRef.current) {
        clearTimeout(highlightTimeoutRef.current);
        highlightTimeoutRef.current = null;
      }
      if (groupClickTimeoutRef.current) {
        clearTimeout(groupClickTimeoutRef.current);
        groupClickTimeoutRef.current = null;
      }
      setHighlightedTaskId(null);

      if (node.type === PLAN_GROUP_NODE_TYPE) {
        const parsed = parseGroupNodeId(node.id);
        const planArtifactId =
          (node.data as PlanGroupData | undefined)?.planArtifactId ??
          (parsed?.kind === "plan" ? parsed.id : undefined);
        if (planArtifactId) {
          onToggleCollapse(planArtifactId);
        }
        return;
      }
      if (node.type === TIER_GROUP_NODE_TYPE) {
        const parsed = parseGroupNodeId(node.id);
        const tierGroupId =
          (node.data as TierGroupData | undefined)?.tierGroupId ??
          (parsed?.kind === "tier" ? parsed.id : node.id);
        onToggleTierCollapse(tierGroupId);
        return;
      }

      openTaskDetails(node.id);
    },
    [onToggleCollapse, onToggleTierCollapse, openTaskDetails]
  );

  const handlePaneClick = useCallback(() => {
    if (highlightTimeoutRef.current) {
      clearTimeout(highlightTimeoutRef.current);
      highlightTimeoutRef.current = null;
    }
    setHighlightedTaskId(null);
    setFocusedNodeId(null);
    clearGraphSelection();
    (reactFlowDomNode ?? containerRef.current)?.focus();
  }, [clearGraphSelection, reactFlowDomNode]);

  const handleKeyDownEvent = useCallback(
    (event: {
      key: string;
      metaKey: boolean;
      altKey: boolean;
      shiftKey: boolean;
      preventDefault: () => void;
      nativeEvent?: KeyboardEvent;
    }) => {
      if (!keyboardNavigationEnabled) return;
      if (hasHandledGraphKey(event)) return;
      const key = event.key;
      const lowerKey = key.toLowerCase();

      if (event.metaKey && ["+", "=", "-", "0", "1"].includes(key)) {
        event.preventDefault();
        if (key === "+" || key === "=") {
          zoomBy(0.1);
          return;
        }
        if (key === "-") {
          zoomBy(-0.1);
          return;
        }
        if (key === "0") {
          fitViewDefault({ padding: 0.2, duration: 200 });
          return;
        }
        if (key === "1") {
          const activeSelection =
            (isNavigableGraphSelection(graphSelection) ? graphSelection : null) ??
            (focusedNodeId
              ? { kind: "task" as const, id: focusedNodeId }
              : selectedTaskId
                ? { kind: "task" as const, id: selectedTaskId }
                : null);
          if (activeSelection) {
            focusSelectionInView(activeSelection);
          } else {
            fitViewDefault({ padding: 0.2, duration: 200 });
          }
          return;
        }
      }

      if (event.altKey && lowerKey === "a") {
        const selection = graphSelection;
        if (!selection) return;
        event.preventDefault();
        if (selection.kind === "planGroup") {
          if (event.shiftKey) {
            const tierIds = tierGroupsByPlan.get(selection.id) ?? [];
            if (tierIds.length === 0) return;
            const shouldExpand = tierIds.some((tierId) => collapsedTierIds.has(tierId));
            onToggleAllTiers(selection.id, shouldExpand ? "expand" : "collapse");
            return;
          }
          onToggleCollapse(selection.id);
          return;
        }
        if (selection.kind === "tierGroup") {
          if (event.shiftKey) {
            const planArtifactId = tierGroupsById.get(selection.id)?.planArtifactId;
            if (!planArtifactId) return;
            const tierIds = tierGroupsByPlan.get(planArtifactId) ?? [];
            if (tierIds.length === 0) return;
            const shouldExpand = tierIds.some((tierId) => collapsedTierIds.has(tierId));
            onToggleAllTiers(planArtifactId, shouldExpand ? "expand" : "collapse");
            return;
          }
          onToggleTierCollapse(selection.id);
        }
        return;
      }

      if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter", "Escape", "Backspace"].includes(key)) {
        return;
      }

      event.preventDefault();

      if (key === "Escape") {
        const selectionForEscape =
          (isNavigableGraphSelection(graphSelection) ? graphSelection : null) ??
          (focusedNodeId
            ? { kind: "task" as const, id: focusedNodeId }
            : selectedTaskId
              ? { kind: "task" as const, id: selectedTaskId }
              : null);

        let recenterPlanId: string | null = null;
        if (selectionForEscape?.kind === "planGroup") {
          recenterPlanId = selectionForEscape.id;
        } else if (selectionForEscape?.kind === "tierGroup") {
          recenterPlanId = tierGroupsById.get(selectionForEscape.id)?.planArtifactId ?? null;
        } else if (selectionForEscape?.kind === "task") {
          recenterPlanId = taskToPlanMap.get(selectionForEscape.id) ?? null;
        }

        setSelectedTaskId(null);
        setFocusedNodeId(null);
        setHighlightedTaskId(null);
        clearGraphSelection();
        if (highlightTimeoutRef.current) {
          clearTimeout(highlightTimeoutRef.current);
          highlightTimeoutRef.current = null;
        }
        if (recenterPlanId) {
          requestAnimationFrame(() => {
            requestAnimationFrame(() => {
              focusSelectionInView({ kind: "planGroup", id: recenterPlanId });
            });
          });
        }
        return;
      }

      const activeSelection =
        (isNavigableGraphSelection(graphSelection) ? graphSelection : null) ??
        (focusedNodeId
          ? { kind: "task" as const, id: focusedNodeId }
          : selectedTaskId
            ? { kind: "task" as const, id: selectedTaskId }
            : null);

      if (key === "Backspace") {
        if (!activeSelection) return;
        if (activeSelection.kind === "task") {
          const tierGroupId = taskToTierMap.get(activeSelection.id);
          if (tierGroupId) {
            setFocusedNodeId(null);
            setGraphSelection({ kind: "tierGroup", id: tierGroupId });
            focusSelectionInView({ kind: "tierGroup", id: tierGroupId });
            return;
          }
          const planArtifactId = taskToPlanMap.get(activeSelection.id);
          if (planArtifactId) {
            setFocusedNodeId(null);
            setGraphSelection({ kind: "planGroup", id: planArtifactId });
            focusSelectionInView({ kind: "planGroup", id: planArtifactId });
            return;
          }
          // Uncategorized task (no tier group, no plan group) — no action
          return;
        }
        if (activeSelection.kind === "tierGroup") {
          const planArtifactId = tierGroupsById.get(activeSelection.id)?.planArtifactId;
          if (planArtifactId) {
            setFocusedNodeId(null);
            setGraphSelection({ kind: "planGroup", id: planArtifactId });
            focusSelectionInView({ kind: "planGroup", id: planArtifactId });
            return;
          }
        }
        if (activeSelection.kind === "planGroup") {
          setFocusedNodeId(null);
          clearGraphSelection();
        }
        return;
      }

      if (key === "Enter") {
        if (!activeSelection) return;
        if (activeSelection.kind === "planGroup") {
          if (collapsedPlanIds.has(activeSelection.id)) {
            onToggleCollapse(activeSelection.id);
            return;
          }

          if (grouping.byTier) {
            const tierNodes = getTierGroupNodesForPlan(activeSelection.id).sort(sortNodesByPosition);
            if (tierNodes.length > 0) {
              const tierIds = tierGroupsByPlan.get(activeSelection.id) ?? [];
              if (tierIds.some((tierId) => collapsedTierIds.has(tierId))) {
                onToggleAllTiers(activeSelection.id, "expand");
              }
              return;
            }
          }

          const firstTask = getFirstTaskInPlan(activeSelection.id);
          if (firstTask) {
            setGraphSelection({ kind: "task", id: firstTask.id });
            setFocusedNodeId(firstTask.id);
            focusSelectionInView({ kind: "task", id: firstTask.id });
          }
          return;
        }

        if (activeSelection.kind === "tierGroup") {
          if (collapsedTierIds.has(activeSelection.id)) {
            onToggleTierCollapse(activeSelection.id);
            return;
          }

          const firstTask = getFirstTaskInTier(activeSelection.id);
          if (firstTask) {
            setGraphSelection({ kind: "task", id: firstTask.id });
            setFocusedNodeId(firstTask.id);
            focusSelectionInView({ kind: "task", id: firstTask.id });
          }
          return;
        }

        if (activeSelection.kind === "task") {
          openTaskDetails(activeSelection.id);
        }
        return;
      }

      const direction =
        key === "ArrowUp"
          ? "up"
          : key === "ArrowDown"
            ? "down"
            : key === "ArrowLeft"
              ? "left"
              : "right";

      if (!activeSelection) {
        if (planGroupNodesSorted.length > 0) {
          const node =
            direction === "up" ? planGroupNodesSorted[planGroupNodesSorted.length - 1] : planGroupNodesSorted[0];
          const planArtifactId =
            (node?.data as PlanGroupData | undefined)?.planArtifactId ?? null;
          if (planArtifactId) {
            setGraphSelection({ kind: "planGroup", id: planArtifactId });
            focusSelectionInView({ kind: "planGroup", id: planArtifactId });
          }
          return;
        }

        const firstTask = layoutNodes[0];
        if (firstTask) {
          setGraphSelection({ kind: "task", id: firstTask.id });
          setFocusedNodeId(firstTask.id);
          focusSelectionInView({ kind: "task", id: firstTask.id });
        }
        return;
      }

      if (activeSelection.kind === "planGroup") {
        if (direction === "left") {
          if (!collapsedPlanIds.has(activeSelection.id)) {
            onToggleCollapse(activeSelection.id);
            return;
          }
        }
        if (direction === "right") {
          if (collapsedPlanIds.has(activeSelection.id)) {
            onToggleCollapse(activeSelection.id);
            return;
          }
          if (grouping.byTier && !collapsedPlanIds.has(activeSelection.id)) {
            const tierNodes = getTierGroupNodesForPlan(activeSelection.id).sort(sortNodesByPosition);
            const firstTier = tierNodes[0];
            const tierGroupId = firstTier
              ? (firstTier.data as TierGroupData | undefined)?.tierGroupId
              : null;
            if (tierGroupId) {
              setGraphSelection({ kind: "tierGroup", id: tierGroupId });
              focusSelectionInView({ kind: "tierGroup", id: tierGroupId });
              return;
            }
          }
          const firstTask = getFirstTaskInPlan(activeSelection.id);
          if (firstTask) {
            setGraphSelection({ kind: "task", id: firstTask.id });
            setFocusedNodeId(firstTask.id);
            focusSelectionInView({ kind: "task", id: firstTask.id });
          }
          return;
        }
        const currentNodeId = getPlanGroupNodeId(activeSelection.id);
        const nextNodeId = findNextGroupNode(direction, currentNodeId, planGroupNodes);
        if (nextNodeId) {
          const nextNode = planGroupNodes.find((node) => node.id === nextNodeId);
          const planArtifactId =
            (nextNode?.data as PlanGroupData | undefined)?.planArtifactId ?? null;
          if (planArtifactId) {
            setGraphSelection({ kind: "planGroup", id: planArtifactId });
            focusSelectionInView({ kind: "planGroup", id: planArtifactId });
          }
        }
        return;
      }

      if (activeSelection.kind === "tierGroup") {
        const planArtifactId = tierGroupsById.get(activeSelection.id)?.planArtifactId;
        if (direction === "left") {
          if (!collapsedTierIds.has(activeSelection.id)) {
            onToggleTierCollapse(activeSelection.id);
            return;
          }
          if (!planArtifactId) {
            return;
          }
          setGraphSelection({ kind: "planGroup", id: planArtifactId });
          focusSelectionInView({ kind: "planGroup", id: planArtifactId });
          return;
        }
        if (direction === "right") {
          if (collapsedTierIds.has(activeSelection.id)) {
            onToggleTierCollapse(activeSelection.id);
            return;
          }
          const firstTask = getFirstTaskInTier(activeSelection.id);
          if (firstTask) {
            setGraphSelection({ kind: "task", id: firstTask.id });
            setFocusedNodeId(firstTask.id);
            focusSelectionInView({ kind: "task", id: firstTask.id });
          }
          return;
        }
        const tierNodes = planArtifactId ? getTierGroupNodesForPlan(planArtifactId) : tierGroupNodes;
        const currentNodeId = getTierGroupNodeId(activeSelection.id);
        const nextNodeId = findNextGroupNode(direction, currentNodeId, tierNodes);
        if (nextNodeId) {
          const nextNode = tierNodes.find((node) => node.id === nextNodeId);
          const tierGroupId =
            (nextNode?.data as TierGroupData | undefined)?.tierGroupId ?? null;
          if (tierGroupId) {
            setGraphSelection({ kind: "tierGroup", id: tierGroupId });
            focusSelectionInView({ kind: "tierGroup", id: tierGroupId });
          }
        }
        return;
      }

      const nextNodeId = findNextNode(direction, activeSelection.id, nodes, edges);
      if (nextNodeId) {
        setFocusedNodeId(nextNodeId);
        setGraphSelection({ kind: "task", id: nextNodeId });
        focusSelectionInView({ kind: "task", id: nextNodeId });
      } else if (direction === "left") {
        const tierGroupId = taskToTierMap.get(activeSelection.id);
        if (tierGroupId) {
          setFocusedNodeId(null);
          setGraphSelection({ kind: "tierGroup", id: tierGroupId });
          focusSelectionInView({ kind: "tierGroup", id: tierGroupId });
          return;
        }
        const planArtifactId = taskToPlanMap.get(activeSelection.id);
        if (planArtifactId) {
          setFocusedNodeId(null);
          setGraphSelection({ kind: "planGroup", id: planArtifactId });
          focusSelectionInView({ kind: "planGroup", id: planArtifactId });
        }
      }
    },
    [
      clearGraphSelection,
      collapsedPlanIds,
      collapsedTierIds,
      edges,
      fitViewDefault,
      focusSelectionInView,
      focusedNodeId,
      getFirstTaskInPlan,
      getFirstTaskInTier,
      getTierGroupNodesForPlan,
      graphSelection,
      grouping.byTier,
      layoutNodes,
      nodes,
      onToggleAllTiers,
      onToggleCollapse,
      onToggleTierCollapse,
      openTaskDetails,
      planGroupNodes,
      planGroupNodesSorted,
      selectedTaskId,
      setGraphSelection,
      setSelectedTaskId,
      taskToPlanMap,
      taskToTierMap,
      tierGroupNodes,
      tierGroupsById,
      tierGroupsByPlan,
      zoomBy,
      keyboardNavigationEnabled,
    ]
  );

  const onKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      handleKeyDownEvent(event);
    },
    [handleKeyDownEvent]
  );

  useEffect(() => {
    if (!keyboardNavigationEnabled) return;
    if (!reactFlowDomNode) return;
    reactFlowDomNode.setAttribute("tabindex", "0");
    const handleDomKeyDown = (event: KeyboardEvent) => {
      if (isEditableTarget(event.target)) return;
      handleKeyDownEvent(event);
    };
    reactFlowDomNode.addEventListener("keydown", handleDomKeyDown, { capture: true });
    return () => {
      reactFlowDomNode.removeEventListener("keydown", handleDomKeyDown, { capture: true });
    };
  }, [handleKeyDownEvent, keyboardNavigationEnabled, reactFlowDomNode]);

  useEffect(() => {
    if (!keyboardNavigationEnabled) return;
    if (!graphReady || isLoading || graphError) return;
    const container = containerRef.current;
    if (!container) return;
    const focusTarget = reactFlowDomNode ?? container;
    const active = document.activeElement;
    if (active && active !== document.body && active !== focusTarget) return;
    focusTarget.focus();
  }, [graphError, graphReady, isLoading, keyboardNavigationEnabled, reactFlowDomNode]);

  useEffect(() => {
    return () => {
      if (highlightTimeoutRef.current) {
        clearTimeout(highlightTimeoutRef.current);
      }
    };
  }, []);

  return {
    focusedNodeId,
    highlightedTaskId,
    graphSelection,
    select,
    focusSelectionInView,
    containerRef,
    onNodeClick: handleNodeClick,
    onNodeDoubleClick: handleNodeDoubleClick,
    onPaneClick: handlePaneClick,
    onTimelineTaskClick: handleTimelineTaskClick,
    onKeyDown,
  };
}
