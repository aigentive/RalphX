import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { useEventBus } from "@/providers/EventProvider";

interface AgentWorkflowProgressEvent {
  runId: string;
  emittedAt: string;
}

export function useAgentWorkflowEvents(): void {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(
    () =>
      bus.subscribe<AgentWorkflowProgressEvent>("agent:workflow_progress", (payload) => {
        if (!payload.runId) return;
        void queryClient.invalidateQueries({
          queryKey: ["agent-workflow-progress", payload.runId],
        });
        void queryClient.invalidateQueries({
          queryKey: ["agent-workflow-latest-run"],
        });
      }),
    [bus, queryClient],
  );
}
