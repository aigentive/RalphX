import { useCallback, useEffect, useRef } from "react";

import {
  getArtifactTabFallback,
  getSeededArtifactTab,
  useAgentSessionStore,
  type AgentArtifactState,
  type AgentArtifactTab,
  type AgentTaskArtifactMode,
} from "@/stores/agentSessionStore";

import { getAgentArtifactStateSnapshot } from "./agentArtifactState";
import { useAgentArtifactUiStore, persistTaskMode } from "./agentArtifactUiStore";
import { preloadAgentsArtifactPane } from "./agentArtifactPanePreload";
import {
  cancelDeferredFrameJob,
  scheduleDeferredFrameJob,
  type DeferredFrameJob,
} from "./agentDeferredFrame";
import { useAgentTerminalStore } from "./agentTerminalStore";

interface UseAgentArtifactControllerArgs {
  hasAutoOpenArtifacts: boolean;
  selectedConversationId: string | null;
}

function movePanelTerminalPlacementToChat() {
  const terminalState = useAgentTerminalStore.getState();
  if (terminalState.placement === "panel") {
    terminalState.setPlacement("chat");
  }
}

export function useAgentArtifactController({
  hasAutoOpenArtifacts,
  selectedConversationId,
}: UseAgentArtifactControllerArgs) {
  const setArtifactState = useAgentSessionStore((s) => s.setArtifactState);
  const artifactPersistenceJobsRef = useRef<
    Map<string, { frame: number | null; timer: number | null; state: AgentArtifactState }>
  >(new Map());
  const artifactPanePreloadJobRef = useRef<DeferredFrameJob | null>(null);
  const cancelArtifactPersistenceJob = useCallback((conversationId: string) => {
    const job = artifactPersistenceJobsRef.current.get(conversationId);
    if (!job) {
      return;
    }
    if (job.frame !== null) {
      window.cancelAnimationFrame(job.frame);
    }
    if (job.timer !== null) {
      window.clearTimeout(job.timer);
    }
    artifactPersistenceJobsRef.current.delete(conversationId);
  }, []);

  const flushArtifactPersistenceJobs = useCallback(() => {
    for (const [conversationId, job] of Array.from(artifactPersistenceJobsRef.current)) {
      if (job.frame !== null) {
        window.cancelAnimationFrame(job.frame);
      }
      if (job.timer !== null) {
        window.clearTimeout(job.timer);
      }
      artifactPersistenceJobsRef.current.delete(conversationId);
      setArtifactState(conversationId, job.state);
    }
  }, [setArtifactState]);

  const cancelArtifactPanePreloadJob = useCallback(() => {
    cancelDeferredFrameJob(artifactPanePreloadJobRef.current);
    artifactPanePreloadJobRef.current = null;
  }, []);

  const scheduleArtifactPanePreload = useCallback(() => {
    if (artifactPanePreloadJobRef.current) {
      return;
    }
    artifactPanePreloadJobRef.current = scheduleDeferredFrameJob(() => {
      artifactPanePreloadJobRef.current = null;
      void preloadAgentsArtifactPane().catch(() => undefined);
    });
  }, []);

  const scheduleArtifactStatePersistence = useCallback(
    (conversationId: string, nextState: AgentArtifactState) => {
      cancelArtifactPersistenceJob(conversationId);
      const job: { frame: number | null; timer: number | null; state: AgentArtifactState } = {
        frame: null,
        timer: null,
        state: nextState,
      };
      job.frame = window.requestAnimationFrame(() => {
        job.frame = null;
        job.timer = window.setTimeout(() => {
          job.timer = null;
          artifactPersistenceJobsRef.current.delete(conversationId);
          setArtifactState(conversationId, nextState);
        }, 0);
      });
      artifactPersistenceJobsRef.current.set(conversationId, job);
    },
    [cancelArtifactPersistenceJob, setArtifactState],
  );

  useEffect(
    () => () => flushArtifactPersistenceJobs(),
    [flushArtifactPersistenceJobs],
  );

  useEffect(
    () => () => cancelArtifactPanePreloadJob(),
    [cancelArtifactPanePreloadJob],
  );

  const updateArtifactState = useCallback(
    (
      conversationId: string,
      updater: (current: AgentArtifactState) => AgentArtifactState,
    ) => {
      const currentState = getAgentArtifactStateSnapshot(conversationId, hasAutoOpenArtifacts);
      const nextState = updater(currentState);
      useAgentArtifactUiStore.getState().setArtifactState(conversationId, nextState);
      scheduleArtifactStatePersistence(conversationId, nextState);
    },
    [hasAutoOpenArtifacts, scheduleArtifactStatePersistence],
  );

  const setArtifactPaneVisibility = useCallback(
    (conversationId: string, isOpen: boolean) => {
      updateArtifactState(conversationId, (current) => ({
        ...current,
        isOpen,
      }));
      if (!isOpen) {
        movePanelTerminalPlacementToChat();
      }
    },
    [updateArtifactState],
  );

  const toggleArtifactPaneVisibility = useCallback(
    (conversationId: string) => {
      const currentState = getAgentArtifactStateSnapshot(
        conversationId,
        hasAutoOpenArtifacts,
      );
      setArtifactPaneVisibility(conversationId, !currentState.isOpen);
    },
    [hasAutoOpenArtifacts, setArtifactPaneVisibility],
  );

  const openArtifactTab = useCallback(
    (conversationId: string, tab: AgentArtifactTab) => {
      updateArtifactState(conversationId, (current) => ({
        ...current,
        activeTab: tab,
        isOpen: true,
        hiddenTabs: current.hiddenTabs.filter((hiddenTab) => hiddenTab !== tab),
      }));
    },
    [updateArtifactState],
  );

  const hideArtifactTab = useCallback(
    (
      conversationId: string,
      tab: AgentArtifactTab,
      availableTabs: readonly AgentArtifactTab[],
    ) => {
      updateArtifactState(conversationId, (current) => {
        const hiddenTabs = current.hiddenTabs.includes(tab)
          ? current.hiddenTabs
          : [...current.hiddenTabs, tab];
        if (current.activeTab !== tab) {
          return { ...current, hiddenTabs };
        }
        const nextActiveTab = getArtifactTabFallback(
          availableTabs,
          hiddenTabs,
          tab,
        );
        return {
          ...current,
          ...(nextActiveTab ? { activeTab: nextActiveTab } : {}),
          hiddenTabs,
        };
      });
    },
    [updateArtifactState],
  );

  const showArtifactTab = useCallback(
    (conversationId: string, tab: AgentArtifactTab) => {
      updateArtifactState(conversationId, (current) => ({
        ...current,
        hiddenTabs: current.hiddenTabs.filter((hiddenTab) => hiddenTab !== tab),
      }));
    },
    [updateArtifactState],
  );

  const seedArtifactTab = useCallback(
    (conversationId: string, tab: AgentArtifactTab) => {
      updateArtifactState(conversationId, (current) => {
        const activeTab = getSeededArtifactTab(
          current.activeTab,
          tab,
          current.hiddenTabs,
        );
        return {
          ...current,
          activeTab,
          isOpen: true,
        };
      });
    },
    [updateArtifactState],
  );

  const setArtifactTaskMode = useCallback(
    (conversationId: string, mode: AgentTaskArtifactMode) => {
      persistTaskMode(mode);
      updateArtifactState(conversationId, (current) => ({
        ...current,
        taskMode: mode,
      }));
    },
    [updateArtifactState],
  );
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || !selectedConversationId) {
        return;
      }
      const activeElement = document.activeElement;
      if (
        activeElement instanceof HTMLInputElement ||
        activeElement instanceof HTMLTextAreaElement
      ) {
        return;
      }

      if (event.key === "\\") {
        event.preventDefault();
        toggleArtifactPaneVisibility(selectedConversationId);
        return;
      }

      const tabByKey: Record<string, AgentArtifactTab> = {
        "1": "plan",
        "2": "verification",
        "4": "tasks",
        "5": "issues",
      };
      const tab = tabByKey[event.key];
      if (tab) {
        event.preventDefault();
        openArtifactTab(selectedConversationId, tab);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    openArtifactTab,
    selectedConversationId,
    toggleArtifactPaneVisibility,
  ]);

  return {
    hideArtifactTab,
    openArtifactTab,
    scheduleArtifactPanePreload,
    seedArtifactTab,
    setArtifactPaneVisibility,
    setArtifactTaskMode,
    showArtifactTab,
    toggleArtifactPaneVisibility,
  };
}
