/**
 * useColumnCollapse — auto-collapse/expand logic for kanban columns
 *
 * Combines uiStore collapse state with stable v29a board behavior:
 * - Columns stay expanded by default so the five-column board does not jump
 * - Columns auto-expand when task count transitions from 0 to N
 * - Manual collapse is only allowed for empty columns
 */

import { useEffect, useRef, useCallback } from "react";
import { useUiStore } from "@/stores/uiStore";
import type { WorkflowColumn } from "@/types/workflow";

export interface UseColumnCollapseReturn {
  /** Check if a column is collapsed */
  isCollapsed: (columnId: string) => boolean;
  /** Toggle collapse state for a column */
  toggleCollapse: (columnId: string) => void;
  /** Expand a specific column */
  expandColumn: (columnId: string) => void;
}

/**
 * Hook that manages column collapse state with auto-collapse/expand logic.
 *
 * @param columns - Workflow columns
 * @param taskCounts - Map from column ID to task count
 * @param ideationSessionId - Current plan/session ID (triggers re-collapse on change)
 */
export function useColumnCollapse(
  columns: WorkflowColumn[],
  taskCounts: Map<string, number>,
  ideationSessionId?: string | null,
): UseColumnCollapseReturn {
  const collapsedColumns = useUiStore((s) => s.collapsedColumns);
  const setColumnCollapsed = useUiStore((s) => s.setColumnCollapsed);
  const storeExpandColumn = useUiStore((s) => s.expandColumn);
  const setCollapsedColumns = useUiStore((s) => s.setCollapsedColumns);

  // Track columns the user has manually expanded
  const userExpandedRef = useRef<Set<string>>(new Set());
  // Track columns the user has manually collapsed (won't auto-expand)
  const userCollapsedRef = useRef<Set<string>>(new Set());
  // Track previous counts for detecting 0→N transitions
  const prevCountsRef = useRef<Map<string, number>>(new Map());
  // Track previous session ID for detecting plan changes
  const prevSessionRef = useRef<string | null | undefined>(undefined);
  // Track whether initial v29a layout reset has been performed
  const initializedRef = useRef(false);

  // Keep the v29a board stable: empty columns render empty states instead of
  // auto-collapsed rails. Plan changes reset stale manual collapse state.
  useEffect(() => {
    const sessionChanged =
      prevSessionRef.current !== undefined &&
      prevSessionRef.current !== ideationSessionId;

    if (sessionChanged) {
      // Plan changed — reset user-expanded/collapsed tracking
      userExpandedRef.current = new Set();
      userCollapsedRef.current = new Set();
    }

    if (!initializedRef.current || sessionChanged) {
      setCollapsedColumns(new Set());
      initializedRef.current = true;
    }

    prevSessionRef.current = ideationSessionId;
  }, [ideationSessionId, setCollapsedColumns]);

  // Auto-expand: detect 0→N count transitions
  useEffect(() => {
    if (!initializedRef.current) return;

    const prevCounts = prevCountsRef.current;

    for (const col of columns) {
      const prevCount = prevCounts.get(col.id) ?? 0;
      const currentCount = taskCounts.get(col.id) ?? 0;

      if (currentCount > 0 && collapsedColumns.has(col.id)) {
        // Columns with tasks should never stay collapsed.
        userCollapsedRef.current.delete(col.id);
        storeExpandColumn(col.id);
      } else if (prevCount === 0 && currentCount > 0) {
        storeExpandColumn(col.id);
      }
    }

    // Update previous counts
    prevCountsRef.current = new Map(taskCounts);
  }, [taskCounts, columns, collapsedColumns, storeExpandColumn]);

  const isCollapsed = useCallback(
    (columnId: string): boolean => collapsedColumns.has(columnId),
    [collapsedColumns],
  );

  const toggleCollapse = useCallback(
    (columnId: string): void => {
      const currentlyCollapsed = collapsedColumns.has(columnId);
      if (currentlyCollapsed) {
        // Expanding — track as user-expanded
        userExpandedRef.current.add(columnId);
        userCollapsedRef.current.delete(columnId);
        storeExpandColumn(columnId);
      } else {
        if ((taskCounts.get(columnId) ?? 0) > 0) {
          return;
        }
        // Collapsing — track as user-collapsed
        userCollapsedRef.current.add(columnId);
        userExpandedRef.current.delete(columnId);
        setColumnCollapsed(columnId, true);
      }
    },
    [collapsedColumns, taskCounts, storeExpandColumn, setColumnCollapsed],
  );

  const expandColumn = useCallback(
    (columnId: string): void => {
      userExpandedRef.current.add(columnId);
      userCollapsedRef.current.delete(columnId);
      storeExpandColumn(columnId);
    },
    [storeExpandColumn],
  );

  return { isCollapsed, toggleCollapse, expandColumn };
}
