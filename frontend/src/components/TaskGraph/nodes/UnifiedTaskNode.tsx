/**
 * UnifiedTaskNode - Single node component that delegates to TaskNode or TaskNodeCompact
 * based on per-node `data.nodeMode`. Replaces the global nodeTypes switching pattern.
 */

import { memo } from "react";
import type { NodeProps, Node } from "@xyflow/react";
import { TaskNode, type TaskNodeData, type TaskNodeType } from "./TaskNode";
import { TaskNodeCompact, type TaskNodeCompactType } from "./TaskNodeCompact";
import type { NodeMode } from "../controls/graph-controls";

export type UnifiedTaskNodeData = TaskNodeData & {
  nodeMode?: NodeMode;
};

type UnifiedTaskNodeType = Node<UnifiedTaskNodeData, "task">;

/**
 * Delegates to TaskNode or TaskNodeCompact based on data.nodeMode.
 * Both accept NodeProps<Node<TaskNodeData, ...>> — the type literal
 * differs but runtime shape is identical, so the cast is safe.
 */
const UnifiedTaskNodeComponent = (props: NodeProps<UnifiedTaskNodeType>) => {
  if (props.data.nodeMode === "compact") {
    return <TaskNodeCompact {...(props as unknown as NodeProps<TaskNodeCompactType>)} />;
  }
  return <TaskNode {...(props as unknown as NodeProps<TaskNodeType>)} />;
};

export const UnifiedTaskNode = memo(UnifiedTaskNodeComponent);
