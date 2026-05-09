/* eslint-disable react-refresh/only-export-components */
/**
 * TierGroup.tsx - Visual region component for dependency tiers inside plan groups
 */

import { memo } from "react";
import { Handle, Position, type NodeProps, type Node } from "@xyflow/react";
import { cn } from "@/lib/utils";
import { TierGroupHeader } from "./TierGroupHeader";
import { getTierGroupNodeId } from "./groupTypes";

export const TIER_HEADER_HEIGHT = 36;

export interface TierGroupData extends Record<string, unknown> {
  tierGroupId: string;
  planArtifactId: string;
  tier: number;
  taskIds: string[];
  isCollapsed: boolean;
  width: number;
  height: number;
  onToggleCollapse?: (tierGroupId: string) => void;
  /** Selection state driven by graph selection */
  isSelected?: boolean;
}

export type TierGroupNode = Node<TierGroupData, "tierGroup">;

export interface TierGroupProps extends NodeProps<TierGroupNode> {
  onToggleCollapse?: (tierGroupId: string) => void;
}

export const TierGroup = memo(function TierGroup({ data, selected }: TierGroupProps) {
  const {
    tierGroupId,
    tier,
    taskIds,
    isCollapsed,
    width,
    height,
    onToggleCollapse,
    isSelected,
  } = data;

  const displayHeight = isCollapsed ? TIER_HEADER_HEIGHT + 8 : height;
  const isGroupSelected = isSelected ?? selected;

  return (
    <div
      className={cn(
        "rounded-md overflow-hidden",
        "bg-[var(--bg-elevated)]",
        "transition-all duration-200"
      )}
      style={{
        width,
        height: displayHeight,
        ...(isGroupSelected && {
          outline: "2px solid var(--accent-primary)",
          outlineOffset: "-2px",
        }),
      }}
      data-testid={`tier-group-${tierGroupId}`}
    >
      <TierGroupHeader
        tier={tier}
        taskCount={taskIds.length}
        isCollapsed={isCollapsed}
        onToggleCollapse={() => onToggleCollapse?.(tierGroupId)}
      />
      {!isCollapsed && (
        <div
          className="relative"
          style={{
            height: displayHeight - TIER_HEADER_HEIGHT,
          }}
        />
      )}

      {/* Invisible handles for tier-connector edges */}
      <Handle
        type="target"
        position={Position.Top}
        className="!bg-transparent !border-0 !w-4 !h-1"
        style={{ top: 0, left: "50%", visibility: "hidden" }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        className="!bg-transparent !border-0 !w-4 !h-1"
        style={{ bottom: 0, left: "50%", visibility: "hidden" }}
      />
    </div>
  );
});

export function createTierGroupNode(
  tierGroupId: string,
  planArtifactId: string,
  tier: number,
  taskIds: string[],
  position: { x: number; y: number },
  width: number,
  height: number,
  isCollapsed = false,
  onToggleCollapse?: (tierGroupId: string) => void
): TierGroupNode {
  return {
    id: getTierGroupNodeId(tierGroupId),
    type: "tierGroup",
    position,
    data: {
      tierGroupId,
      planArtifactId,
      tier,
      taskIds,
      isCollapsed,
      width,
      height,
      ...(onToggleCollapse && { onToggleCollapse }),
    },
    style: {
      width,
      height: isCollapsed ? TIER_HEADER_HEIGHT + 8 : height,
    },
    draggable: false,
    selectable: true,
    zIndex: -1,
  };
}

export const TIER_GROUP_NODE_TYPE = "tierGroup";
