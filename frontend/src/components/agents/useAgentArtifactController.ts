import { useCallback, useEffect, useRef } from "react";

import {
  useAgentSessionStore,
  type AgentArtifactState,
  type AgentArtifactTab,
  type AgentTaskArtifactMode,
} from "@/stores/agentSessionStore";

import { getAgentArtifactStateSnapshot } from "./agentArtifactState";
import { useAgentArtifactUiStore, persistTaskMode } from "./agentArtifactUiStore";
import { preloadAgentsArtifactPane } from "./agentArtifactPanePreload";
import type { DeferredFrameJob } from "./agentDeferredFrame";

interface UseAgentArtifactControllerArgs {
  hasAutoOpenArtifacts: boolean;
  selectedConversationId: string | null;
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
    const job = artifactPanePreloadJobRef.current;
    if (!job) {
      return;
    }
    if (job.frame !== null) {
      window.cancelAnimationFrame(job.frame);
    }
    if (job.timer !== null) {
      window.clearTimeout(job.timer);
    }
    artifactPanePreloadJobRef.current = null;
  }, []);

  const scheduleArtifactPanePreload = useCallback(() => {
    if (artifactPanePreloadJobRef.current) {
      return;
    }
    const job: DeferredFrameJob = {
      frame: null,
      timer: null,
    };
    job.frame = window.requestAnimationFrame(() => {
      job.frame = null;
      job.timer = window.setTimeout(() => {
        job.timer = null;
        artifactPanePreloadJobRef.current = null;
        void preloadAgentsArtifactPane().catch(() => undefined);
      }, 0);
    });
    artifactPanePreloadJobRef.current = job;
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
      }));
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
        "3": "proposal",
        "4": "tasks",
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
    openArtifactTab,
    scheduleArtifactPanePreload,
    setArtifactPaneVisibility,
    setArtifactTaskMode,
    toggleArtifactPaneVisibility,
  };
}
