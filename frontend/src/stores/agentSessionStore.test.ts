import { beforeEach, describe, expect, it } from "vitest";

import {
  migrateAgentSessionStore,
  selectArtifactState,
  selectHasStoredArtifactState,
  useAgentSessionStore,
} from "./agentSessionStore";

describe("agentSessionStore", () => {
  it("defaults the Agents sidebar to all projects", () => {
    expect(useAgentSessionStore.getInitialState().showAllProjects).toBe(true);
  });

  it("migrates older persisted sidebar filter state to all projects", () => {
    expect(
      migrateAgentSessionStore(
        {
          showAllProjects: false,
          projectSort: "latest",
        },
        0,
      ),
    ).toMatchObject({
      showAllProjects: true,
    });
  });

  it("preserves current persisted sidebar filter state", () => {
    expect(
      migrateAgentSessionStore(
        {
          showAllProjects: false,
          projectSort: "latest",
        },
        1,
      ),
    ).toMatchObject({
      showAllProjects: false,
    });
  });

  it("migrates remembered runtimes to include model-specific effort", () => {
    expect(
      migrateAgentSessionStore(
        {
          runtimeByConversationId: {
            "conversation-1": {
              provider: "codex",
              modelId: "gpt-5.4-mini",
            },
          },
          lastRuntimeByProjectId: {
            "project-1": {
              provider: "claude",
              modelId: "opus",
            },
          },
        },
        1,
      ),
    ).toMatchObject({
      runtimeByConversationId: {
        "conversation-1": {
          provider: "codex",
          modelId: "gpt-5.4-mini",
          effort: "medium",
        },
      },
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "claude",
          modelId: "opus",
          effort: "xhigh",
        },
      },
    });
  });

  it("preserves valid remembered runtime efforts during migration", () => {
    expect(
      migrateAgentSessionStore(
        {
          lastRuntimeByProjectId: {
            "project-1": {
              provider: "claude",
              modelId: "opus",
              effort: "high",
            },
          },
        },
        1,
      ),
    ).toMatchObject({
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "claude",
          modelId: "opus",
          effort: "high",
        },
      },
    });
  });

  it("returns persistedState unchanged when version is at-or-above current", () => {
    expect(migrateAgentSessionStore({ showAllProjects: false }, 99)).toEqual({
      showAllProjects: false,
    });
  });

  it("passes through non-object persistedState", () => {
    expect(migrateAgentSessionStore(null, 0)).toBeNull();
    expect(migrateAgentSessionStore("nope", 0)).toBe("nope");
  });

  describe("actions", () => {
    beforeEach(() => {
      useAgentSessionStore.setState(useAgentSessionStore.getInitialState(), true);
    });

    it("setFocusedProject expands only the focused project and restores last conversation", () => {
      const { setFocusedProject, selectConversation, setProjectExpanded } =
        useAgentSessionStore.getState();

      // Seed two expanded projects + a remembered conversation for "p1"
      setProjectExpanded("p1", true);
      setProjectExpanded("p2", true);
      selectConversation("p1", "conv-1");

      // Focus a different project; p1 should collapse, last conversation should not change
      setFocusedProject("p2");
      const after = useAgentSessionStore.getState();
      expect(after.focusedProjectId).toBe("p2");
      expect(after.expandedProjectIds).toEqual({ p1: false, p2: true });

      // Focus back to p1 — its last conversation is restored as the selected one.
      setFocusedProject("p1");
      const restored = useAgentSessionStore.getState();
      expect(restored.selectedProjectId).toBe("p1");
      expect(restored.selectedConversationId).toBe("conv-1");

      // Clearing focus is a no-op for expansion.
      setFocusedProject(null);
      expect(useAgentSessionStore.getState().focusedProjectId).toBeNull();
    });

    it("selectConversation pins focus + remembers per-project last conversation", () => {
      const { selectConversation, clearSelection } = useAgentSessionStore.getState();

      selectConversation("p1", "conv-1");
      let s = useAgentSessionStore.getState();
      expect(s.selectedProjectId).toBe("p1");
      expect(s.selectedConversationId).toBe("conv-1");
      expect(s.lastSelectedConversationByProjectId.p1).toBe("conv-1");
      expect(s.expandedProjectIds.p1).toBe(true);

      clearSelection();
      s = useAgentSessionStore.getState();
      expect(s.selectedProjectId).toBeNull();
      expect(s.selectedConversationId).toBeNull();
    });

    it("setProjectExpanded false leaves other expansions intact", () => {
      const { setProjectExpanded } = useAgentSessionStore.getState();
      setProjectExpanded("p1", true);
      setProjectExpanded("p2", false);
      expect(useAgentSessionStore.getState().expandedProjectIds).toEqual({
        p1: true,
        p2: false,
      });
    });

    it("toggleProjectExpanded flips expansion both ways", () => {
      const { toggleProjectExpanded } = useAgentSessionStore.getState();
      toggleProjectExpanded("p1");
      expect(useAgentSessionStore.getState().expandedProjectIds.p1).toBe(true);
      toggleProjectExpanded("p1");
      expect(useAgentSessionStore.getState().expandedProjectIds.p1).toBe(false);
    });

    it("setShowAllProjects + setProjectSort persist their values", () => {
      const { setShowAllProjects, setProjectSort } = useAgentSessionStore.getState();
      setShowAllProjects(false);
      setProjectSort("za");
      const s = useAgentSessionStore.getState();
      expect(s.showAllProjects).toBe(false);
      expect(s.projectSort).toBe("za");
    });

    it("artifact actions: open/tab/state/taskMode flow", () => {
      const {
        setArtifactOpen,
        setArtifactTab,
        setArtifactState,
        setTaskArtifactMode,
      } = useAgentSessionStore.getState();

      setArtifactOpen("c1", true);
      expect(selectArtifactState("c1")(useAgentSessionStore.getState()).isOpen).toBe(true);

      setArtifactTab("c1", "verification");
      const after = selectArtifactState("c1")(useAgentSessionStore.getState());
      expect(after.activeTab).toBe("verification");
      expect(after.isOpen).toBe(true);

      setArtifactState("c1", { isOpen: false, activeTab: "plan", taskMode: "kanban" });
      expect(selectArtifactState("c1")(useAgentSessionStore.getState())).toEqual({
        isOpen: false,
        activeTab: "plan",
        taskMode: "kanban",
      });

      setTaskArtifactMode("c1", "graph");
      expect(selectArtifactState("c1")(useAgentSessionStore.getState()).taskMode).toBe("graph");
      expect(selectHasStoredArtifactState("c1")(useAgentSessionStore.getState())).toBe(true);
      expect(selectHasStoredArtifactState("missing")(useAgentSessionStore.getState())).toBe(false);
      expect(selectHasStoredArtifactState(null)(useAgentSessionStore.getState())).toBe(false);
    });

    it("setRuntimeForConversation + setLastRuntimeForProject normalize via lib/agent-models", () => {
      const { setRuntimeForConversation, setLastRuntimeForProject } =
        useAgentSessionStore.getState();
      setRuntimeForConversation("c1", "p1", {
        provider: "claude",
        modelId: "opus",
        effort: "xhigh",
      });
      const s1 = useAgentSessionStore.getState();
      expect(s1.runtimeByConversationId.c1.modelId).toBe("opus");
      expect(s1.lastRuntimeByProjectId.p1.modelId).toBe("opus");

      setLastRuntimeForProject("p2", { provider: "codex", modelId: "gpt-5.4-mini" });
      expect(useAgentSessionStore.getState().lastRuntimeByProjectId.p2.modelId).toBe(
        "gpt-5.4-mini",
      );
    });
  });

  describe("selectArtifactState", () => {
    it("returns the default state when no entry is stored", () => {
      const s = useAgentSessionStore.getInitialState();
      expect(selectArtifactState(null)(s).isOpen).toBe(false);
      expect(selectArtifactState("missing")(s).activeTab).toBe("plan");
    });
  });
});
