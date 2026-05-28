import { useEffect, useRef } from "react";

import {
  type AgentWorkspaceOperationToast,
  agentWorkspaceOperationToastId,
  publishPipelineToastLabel,
  startAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";

export function AgentsPublishProgressToast({
  active,
  conversationTitle,
  conversationId,
  startedAtMs,
  status,
}: {
  active: boolean;
  conversationTitle?: string | null;
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
        conversationTitle,
        detail,
        id: toastId,
        startedAtMs: startedAtMs ?? Date.now(),
        title: "Publishing workspace",
      });
      return;
    }

    progressToastRef.current.update({
      conversationTitle,
      detail,
      id: toastId,
      title: "Publishing workspace",
    });
  }, [active, conversationTitle, detail, startedAtMs, toastId]);

  useEffect(
    () => () => {
      progressToastRef.current?.dispose();
      progressToastRef.current = null;
    },
    [],
  );

  return null;
}
