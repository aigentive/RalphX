import type { InternalStatus } from "@/types/status";

export type LayoutDirection = "TB" | "LR";

export type GroupingState = {
  byPlan: boolean;
  byTier: boolean;
  showUncategorized: boolean;
};

export interface GraphFilters {
  /** Selected status values (empty = show all). */
  statuses: InternalStatus[];
  /** Whether to include archived tasks (fetched from backend). */
  showArchived: boolean;
}

export type NodeMode = "standard" | "compact";

export const DEFAULT_GRAPH_FILTERS: GraphFilters = {
  statuses: [],
  showArchived: false,
};

export const DEFAULT_LAYOUT_DIRECTION: LayoutDirection = "TB";

export const DEFAULT_GROUPING: GroupingState = {
  byPlan: true,
  byTier: true,
  showUncategorized: true,
};

export const DEFAULT_NODE_MODE: NodeMode = "standard";

/** Threshold for auto-switching to compact nodes. */
export const COMPACT_MODE_THRESHOLD = 8;
