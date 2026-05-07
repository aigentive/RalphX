/**
 * useColumnCollapse — auto-collapse/expand logic for kanban columns
 *
 * Combines uiStore collapse state with stable v29a board behavior:
 * - Empty columns collapse into rails by default
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
  // Track previous session ID for detecting plan changes
  const prevSessionRef = useRef<string | null | undefined>(undefined);
  // Track whether initial v29a layout reset has been performed
  const initializedRef = useRef(false);

  // Empty columns collapse into compact rails by default. Plan changes reset
  // stale manual expand/collapse state so the new plan reflects its counts.
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

  // Auto-collapse empty columns and auto-expand columns with tasks.
  useEffect(() => {
    if (!initializedRef.current) return;

    const nextCollapsed = new Set(collapsedColumns);

    for (const col of columns) {
      const currentCount = taskCounts.get(col.id) ?? 0;

      if (currentCount > 0) {
        // Columns with tasks should never stay collapsed.
        userExpandedRef.current.delete(col.id);
        userCollapsedRef.current.delete(col.id);
        nextCollapsed.delete(col.id);
      } else if (!userExpandedRef.current.has(col.id)) {
        nextCollapsed.add(col.id);
      }
    }

    const changed =
      nextCollapsed.size !== collapsedColumns.size ||
      Array.from(nextCollapsed).some((columnId) => !collapsedColumns.has(columnId));

    if (changed) {
      setCollapsedColumns(nextCollapsed);
    }

  }, [taskCounts, columns, collapsedColumns, setCollapsedColumns]);

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
