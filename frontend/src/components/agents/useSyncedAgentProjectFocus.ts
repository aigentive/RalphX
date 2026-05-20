import { useEffect, useRef } from "react";

import { useAgentSessionStore } from "@/stores/agentSessionStore";

export function useSyncedAgentProjectFocus(
  projectId: string,
  setFocusedProject: (projectId: string | null) => void,
) {
  const syncedProjectIdRef = useRef<string | null>(null);
  const selectedProjectId = useAgentSessionStore((s) => s.selectedProjectId);
  const isInitialRef = useRef(true);

  useEffect(() => {
    if (!projectId || syncedProjectIdRef.current === projectId) {
      return;
    }
    syncedProjectIdRef.current = projectId;

    if (isInitialRef.current && selectedProjectId && selectedProjectId !== projectId) {
      isInitialRef.current = false;
      setFocusedProject(selectedProjectId);
      return;
    }
    isInitialRef.current = false;
    setFocusedProject(projectId);
  }, [projectId, selectedProjectId, setFocusedProject]);
}
