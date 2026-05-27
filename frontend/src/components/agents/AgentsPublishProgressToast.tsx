import { useEffect, useRef } from "react";

import {
  type AgentWorkspaceOperationToast,
  agentWorkspaceOperationToastId,
  publishPipelineToastLabel,
  startAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";

export function AgentsPublishProgressToast({
  active,
  conversationId,
  startedAtMs,
  status,
}: {
  active: boolean;
  conversationId: string | null;
  startedAtMs: number | null;
  status: string | null;
}) {
  const progressToastRef = useRef<AgentWorkspaceOperationToast | null>(null);
  const toastId = conversationId
    ? agentWorkspaceOperationToastId(conversationId, "publish")
    : null;
  const detail = publishPipelineToastLabel(status);

  useEffect(() => {
    if (!active || !toastId) {
      progressToastRef.current?.dispose();
      progressToastRef.current = null;
      return;
    }

    if (!progressToastRef.current) {
      progressToastRef.current = startAgentWorkspaceOperationToast({
        detail,
        id: toastId,
        startedAtMs: startedAtMs ?? Date.now(),
        title: "Publishing workspace",
      });
      return;
    }

    progressToastRef.current.update({
      detail,
      id: toastId,
      title: "Publishing workspace",
    });
  }, [active, detail, startedAtMs, toastId]);

  useEffect(
    () => () => {
      progressToastRef.current?.dispose();
      progressToastRef.current = null;
    },
    [],
  );

  return null;
}
