import { beforeEach, describe, expect, it } from "vitest";

import {
  getShownArtifactTabs,
  migrateAgentSessionStore,
  mergeAgentSessionStore,
  selectArtifactState,
  selectHasStoredArtifactState,
  useAgentSessionStore,
} from "./agentSessionStore";

describe("agentSessionStore", () => {
  it("drops structurally corrupt current-version runtime preferences", () => {
    const merged = mergeAgentSessionStore(
      {
        serviceTierByConversationId: { good: "standard", bad: "turbo" },
        roleRuntimeOverridesByConversationId: {
          conversation: {
            workspace_reviewer: {
              provider: "claude",
              model: "sonnet",
              effort: "high",
              serviceTier: "fast",
              coordinationMode: "solo",
              personaId: null,
            },
            workspace_repair: {
              provider: "codex",
              model: [],
              effort: "high",
              serviceTier: "fast",
              coordinationMode: "solo",
              personaId: null,
            },
          },
        },
      },
      useAgentSessionStore.getState(),
    );

    expect(merged.serviceTierByConversationId).toEqual({ good: "standard" });
    expect(merged.roleRuntimeOverridesByConversationId).toEqual({
      conversation: {
        workspace_reviewer: expect.objectContaining({
          provider: "claude",
          serviceTier: "fast",
        }),
      },
    });
  });

  it("defaults the Agents sidebar to all projects", () => {
    expect(useAgentSessionStore.getInitialState().showAllProjects).toBe(true);
    expect(useAgentSessionStore.getInitialState().showEmptyProjectGroups).toBe(true);
    expect(useAgentSessionStore.getInitialState().sidebarGroupBy).toBe("inbox");
    expect(useAgentSessionStore.getInitialState().sidebarPublicationStateFilters).toEqual([
      "active",
      "draft",
      "merged",
      "closed",
      "uncommitted",
      "unpushed",
    ]);
    expect(useAgentSessionStore.getInitialState().defaultStartMode).toBe("edit");
  });

  it("migrates invalid new-run mode preferences to Agent", () => {
    expect(
      migrateAgentSessionStore(
        {
          defaultStartMode: "persona_builder",
        },
        8,
      ),
    ).toMatchObject({ defaultStartMode: "edit" });

    expect(
      migrateAgentSessionStore(
        {
          defaultStartMode: "plan",
        },
        8,
      ),
    ).toMatchObject({ defaultStartMode: "plan" });
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

  it("migrates existing sidebar preferences to show empty project groups", () => {
    expect(
      migrateAgentSessionStore(
        {
          showAllProjects: false,
          projectSort: "za",
          sidebarGroupBy: "project",
          sidebarProjectFilterIds: ["project-1", "project-2"],
          sidebarPublicationStateFilters: ["active", "draft"],
        },
        9,
      ),
    ).toMatchObject({
      showAllProjects: false,
      showEmptyProjectGroups: true,
      projectSort: "za",
      sidebarGroupBy: "inbox",
      sidebarProjectFilterIds: ["project-1", "project-2"],
      sidebarPublicationStateFilters: ["active", "draft"],
    });
  });

  it("preserves a current persisted empty-project-group preference", () => {
    expect(
      migrateAgentSessionStore(
        {
          showEmptyProjectGroups: false,
          sidebarProjectFilterIds: ["project-2"],
        },
        10,
      ),
    ).toEqual({
      showEmptyProjectGroups: false,
      sidebarProjectFilterIds: ["project-2"],
      sidebarGroupBy: "inbox",
      sidebarInboxActiveLane: "needs",
    });
  });

  it("promotes stores persisted before the inbox existed to the inbox default", () => {
    const migrated = migrateAgentSessionStore(
      { sidebarGroupBy: "publication" },
      10,
    ) as { sidebarGroupBy?: unknown };

    expect(migrated.sidebarGroupBy).toBe("inbox");
  });

  it("preserves a grouping chosen after the inbox shipped", () => {
    const migrated = migrateAgentSessionStore(
      { sidebarGroupBy: "publication" },
      11,
    ) as { sidebarGroupBy?: unknown };

    expect(migrated.sidebarGroupBy).toBe("publication");
  });

  it("defaults the inbox active lane for stores persisted before lanes existed", () => {
    const migrated = migrateAgentSessionStore(
      { sidebarGroupBy: "project" },
      10,
    ) as { sidebarInboxActiveLane?: unknown };

    expect(migrated.sidebarInboxActiveLane).toBe("needs");
  });

  it("drops the retired collapsed-lane set instead of projecting it onto a selection", () => {
    const migrated = migrateAgentSessionStore(
      { sidebarInboxCollapsedLanes: ["done", "stale"] },
      11,
    ) as Record<string, unknown>;

    expect("sidebarInboxCollapsedLanes" in migrated).toBe(false);
    expect(migrated.sidebarInboxActiveLane).toBe("needs");
  });

  it("preserves an inbox active lane persisted after the lane switcher shipped", () => {
    const migrated = migrateAgentSessionStore(
      { sidebarInboxActiveLane: "done" },
      12,
    ) as { sidebarInboxActiveLane?: unknown };

    expect(migrated.sidebarInboxActiveLane).toBe("done");
  });

  it("falls back to the needs lane when the persisted active lane is unknown", () => {
    const merged = mergeAgentSessionStore(
      { sidebarInboxActiveLane: "archived-lane" },
      useAgentSessionStore.getState(),
    );

    expect(merged.sidebarInboxActiveLane).toBe("needs");
  });

  it("keeps a known persisted active lane through merge", () => {
    const merged = mergeAgentSessionStore(
      { sidebarInboxActiveLane: "stale" },
      useAgentSessionStore.getState(),
    );

    expect(merged.sidebarInboxActiveLane).toBe("stale");
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

  it("normalizes remembered Ultra preferences to ordinary Max", () => {
    expect(
      migrateAgentSessionStore(
        {
          runtimeByConversationId: {
            "conversation-1": {
              provider: "codex",
              modelId: "gpt-5.6-terra",
              effort: "ultra",
            },
          },
          lastRuntimeByProjectId: {
            "project-1": {
              provider: "codex",
              modelId: "gpt-5.6-luna",
              effort: "ultra",
            },
          },
        },
        1,
      ),
    ).toMatchObject({
      runtimeByConversationId: {
        "conversation-1": {
          provider: "codex",
          modelId: "gpt-5.6-terra",
          effort: "max",
        },
      },
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "codex",
          modelId: "gpt-5.6-luna",
          effort: "max",
        },
      },
    });
  });

  it("returns persistedState unchanged when version is at-or-above current", () => {
    expect(migrateAgentSessionStore({ showAllProjects: false }, 99)).toEqual({
      showAllProjects: false,
    });
  });

  it("migrates legacy standalone proposal artifact tabs to Plan", () => {
    expect(
      migrateAgentSessionStore(
        {
          artifactByConversationId: {
            "conversation-1": {
              isOpen: true,
              activeTab: "proposal",
              taskMode: "kanban",
            },
            "conversation-2": {
              isOpen: false,
              activeTab: "tasks",
              taskMode: "graph",
            },
          },
        },
        6,
      ),
    ).toMatchObject({
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "plan",
          taskMode: "kanban",
        },
        "conversation-2": {
          isOpen: false,
          activeTab: "tasks",
          taskMode: "graph",
        },
      },
    });
  });

  it("migrates older persisted sidebar metadata filters and pin state", () => {
    expect(
      migrateAgentSessionStore(
        {
          showAllProjects: true,
          projectSort: "latest",
        },
        3,
      ),
    ).toMatchObject({
      sidebarGroupBy: "inbox",
      sidebarProjectFilterIds: [],
      sidebarPublicationStateFilters: [
        "active",
        "draft",
        "merged",
        "closed",
        "uncommitted",
        "unpushed",
      ],
      pinnedConversationIds: {},
    });
  });

  it("migrates stale persisted Proposals artifact tabs to Plan", () => {
    expect(
      migrateAgentSessionStore(
        {
          artifactByConversationId: {
            "conversation-1": {
              isOpen: true,
              activeTab: "proposal",
              taskMode: "graph",
            },
            "conversation-2": {
              isOpen: true,
              activeTab: "verification",
              taskMode: "kanban",
            },
          },
        },
        6,
      ),
    ).toMatchObject({
      artifactByConversationId: {
        "conversation-1": {
          isOpen: true,
          activeTab: "plan",
          taskMode: "graph",
        },
        "conversation-2": {
          isOpen: true,
          activeTab: "verification",
          taskMode: "kanban",
        },
      },
    });
  });

  it("migrates v7 artifact visibility preferences and drops unknown tabs", () => {
    expect(
      migrateAgentSessionStore(
        {
          artifactByConversationId: {
            "conversation-1": {
              isOpen: true,
              activeTab: "plan",
              taskMode: "graph",
              hiddenTabs: ["plan", "removed-tab", "plan", "jira"],
            },
            "conversation-2": {
              isOpen: false,
              activeTab: "tasks",
              taskMode: "kanban",
            },
          },
        },
        7,
      ),
    ).toMatchObject({
      artifactByConversationId: {
        "conversation-1": {
          hiddenTabs: ["plan", "jira"],
        },
        "conversation-2": {
          hiddenTabs: [],
        },
      },
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

    it("defaults the inbox lane switcher to the needs lane", () => {
      expect(useAgentSessionStore.getState().sidebarInboxActiveLane).toBe("needs");
    });

    it("persists the selected inbox lane", () => {
      useAgentSessionStore.getState().setSidebarInboxActiveLane("done");

      expect(useAgentSessionStore.getState().sidebarInboxActiveLane).toBe("done");
      expect(localStorage.getItem("ralphx-agent-session-store")).toContain(
        '"sidebarInboxActiveLane":"done"',
      );
    });

    it("persists the preferred ordinary new-run mode", () => {
      useAgentSessionStore.getState().setDefaultStartMode("plan");

      expect(useAgentSessionStore.getState().defaultStartMode).toBe("plan");
      expect(localStorage.getItem("ralphx-agent-session-store")).toContain(
        '"defaultStartMode":"plan"',
      );
    });

    it("setFocusedProject expands only the focused project without selecting a conversation", () => {
      const { clearSelection, setFocusedProject, selectConversation, setProjectExpanded } =
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

      clearSelection();

      // Focus back to p1 — its last conversation remains remembered, but focus
      // does not select it. Only selectConversation may select a conversation.
      setFocusedProject("p1");
      const restored = useAgentSessionStore.getState();
      expect(restored.selectedProjectId).toBeNull();
      expect(restored.selectedConversationId).toBeNull();
      expect(restored.lastSelectedConversationByProjectId.p1).toBe("conv-1");

      // Clearing focus is a no-op for expansion.
      setFocusedProject(null);
      expect(useAgentSessionStore.getState().focusedProjectId).toBeNull();
    });

    it("stores and consumes a pending start conversation draft once", () => {
      const { consumeStartConversationDraft, setStartConversationDraft } =
        useAgentSessionStore.getState();

      const composerProjectReferences = [
        {
          projectId: "project-1",
          name: "ralphx",
          path: "/work/ralphx",
        },
      ];
      const composerArtifactReferences = [
        {
          kind: "plan" as const,
          artifactId: "plan-1",
          title: "Fix flow",
          sessionId: "session-1",
          version: 2,
          status: "approved",
        },
      ];
      const composerIntegrationReferences = [
        {
          provider: "clickup" as const,
          kind: "clickup" as const,
          id: "TASK-123",
          key: "TASK-123",
          title: "Demo task",
        },
      ];

      setStartConversationDraft({
        projectId: "project-1",
        content: "Fix the failing publish flow",
        mode: "edit",
        composerProjectReferences,
        composerArtifactReferences,
        composerIntegrationReferences,
      });

      expect(useAgentSessionStore.getState().startConversationDraft).toEqual({
        projectId: "project-1",
        content: "Fix the failing publish flow",
        mode: "edit",
        composerProjectReferences,
        composerArtifactReferences,
        composerIntegrationReferences,
      });
      const consumed = consumeStartConversationDraft();
      expect(consumed).toEqual({
        projectId: "project-1",
        content: "Fix the failing publish flow",
        mode: "edit",
        composerProjectReferences,
        composerArtifactReferences,
        composerIntegrationReferences,
      });
      expect(consumed?.composerProjectReferences?.[0]).not.toBe(composerProjectReferences[0]);
      expect(consumed?.composerArtifactReferences?.[0]).not.toBe(composerArtifactReferences[0]);
      expect(consumed?.composerIntegrationReferences?.[0]).not.toBe(
        composerIntegrationReferences[0],
      );
      expect(useAgentSessionStore.getState().startConversationDraft).toBeNull();
      expect(consumeStartConversationDraft()).toBeNull();
    });

    it("round-trips a standalone start draft without inventing a project id", () => {
      const { consumeStartConversationDraft, setStartConversationDraft } =
        useAgentSessionStore.getState();
      setStartConversationDraft({
        projectId: null,
        content: "Explore this privately",
        mode: "chat",
      });

      expect(consumeStartConversationDraft()).toEqual({
        projectId: null,
        content: "Explore this privately",
        mode: "chat",
      });
    });

    it("preserves persona-builder scope lock and refine provenance in the consumed copy", () => {
      const { consumeStartConversationDraft, setStartConversationDraft } =
        useAgentSessionStore.getState();
      setStartConversationDraft({
        projectId: null,
        projectLocked: true,
        content: "",
        mode: "persona_builder",
        sourcePersonaId: "persona-1",
        sourcePersonaName: "Reviewer Voice",
      });

      expect(consumeStartConversationDraft()).toEqual({
        projectId: null,
        projectLocked: true,
        content: "",
        mode: "persona_builder",
        sourcePersonaId: "persona-1",
        sourcePersonaName: "Reviewer Voice",
      });
      expect(useAgentSessionStore.getState().startConversationDraft).toBeNull();
    });

    it("selects a standalone conversation without project focus bookkeeping", () => {
      useAgentSessionStore.getState().selectConversation(null, "standalone-1");

      const state = useAgentSessionStore.getState();
      expect(state.selectedProjectId).toBeNull();
      expect(state.focusedProjectId).toBeNull();
      expect(state.selectedConversationId).toBe("standalone-1");
      expect(state.lastSelectedConversationByProjectId).toEqual({});
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

    it("persists project scope, empty-group visibility, and sort preferences", () => {
      const {
        setProjectSort,
        setShowAllProjects,
        setShowEmptyProjectGroups,
      } = useAgentSessionStore.getState();
      setShowAllProjects(false);
      setShowEmptyProjectGroups(false);
      setProjectSort("za");
      const s = useAgentSessionStore.getState();
      expect(s.showAllProjects).toBe(false);
      expect(s.showEmptyProjectGroups).toBe(false);
      expect(s.projectSort).toBe("za");
      expect(localStorage.getItem("ralphx-agent-session-store")).toContain(
        '"showEmptyProjectGroups":false',
      );
    });

    it("persists sidebar filters and pinned conversation ids", () => {
      const {
        setSidebarGroupBy,
        setSidebarProjectFilterIds,
        setSidebarPublicationStateFilters,
        togglePinnedConversation,
      } = useAgentSessionStore.getState();

      setSidebarGroupBy("automation");
      setSidebarProjectFilterIds(["project-2"]);
      setSidebarPublicationStateFilters(["merged", "closed"]);
      togglePinnedConversation("conversation-1");

      let s = useAgentSessionStore.getState();
      expect(s.sidebarGroupBy).toBe("automation");
      expect(s.sidebarProjectFilterIds).toEqual(["project-2"]);
      expect(s.sidebarPublicationStateFilters).toEqual(["merged", "closed"]);
      expect(s.pinnedConversationIds["conversation-1"]).toBe(true);

      togglePinnedConversation("conversation-1");
      s = useAgentSessionStore.getState();
      expect(s.pinnedConversationIds["conversation-1"]).toBeUndefined();
    });

    it("toggles individual sidebar project and publication-state filters", () => {
      const {
        setSidebarProjectFilterIds,
        setSidebarPublicationStateFilters,
        toggleSidebarProjectFilter,
        toggleSidebarPublicationStateFilter,
      } = useAgentSessionStore.getState();

      setSidebarProjectFilterIds(["project-1"]);
      toggleSidebarProjectFilter("project-2");
      expect(useAgentSessionStore.getState()).toMatchObject({
        showAllProjects: false,
        sidebarProjectFilterIds: ["project-1", "project-2"],
      });
      toggleSidebarProjectFilter("project-1");
      expect(useAgentSessionStore.getState().sidebarProjectFilterIds).toEqual([
        "project-2",
      ]);

      setSidebarPublicationStateFilters(["merged"]);
      toggleSidebarPublicationStateFilter("closed");
      expect(useAgentSessionStore.getState().sidebarPublicationStateFilters).toEqual([
        "merged",
        "closed",
      ]);
      toggleSidebarPublicationStateFilter("merged");
      expect(useAgentSessionStore.getState().sidebarPublicationStateFilters).toEqual([
        "closed",
      ]);
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

      setArtifactState("c1", {
        isOpen: false,
        activeTab: "plan",
        taskMode: "kanban",
        hiddenTabs: [],
      });
      expect(selectArtifactState("c1")(useAgentSessionStore.getState())).toEqual({
        isOpen: false,
        activeTab: "plan",
        taskMode: "kanban",
        hiddenTabs: [],
      });

      setTaskArtifactMode("c1", "graph");
      expect(selectArtifactState("c1")(useAgentSessionStore.getState()).taskMode).toBe("graph");
      expect(selectHasStoredArtifactState("c1")(useAgentSessionStore.getState())).toBe(true);
      expect(selectHasStoredArtifactState("missing")(useAgentSessionStore.getState())).toBe(false);
      expect(selectHasStoredArtifactState(null)(useAgentSessionStore.getState())).toBe(false);
    });

    it("hides, shows, and reveals tabs without changing pane-open semantics", () => {
      const {
        hideArtifactTab,
        revealArtifactTab,
        setArtifactOpen,
        setArtifactTab,
        showArtifactTab,
      } = useAgentSessionStore.getState();

      setArtifactTab("c1", "verification");
      hideArtifactTab("c1", "verification", ["plan", "verification", "tasks"]);
      expect(selectArtifactState("c1")(useAgentSessionStore.getState())).toEqual({
        isOpen: true,
        activeTab: "plan",
        taskMode: "graph",
        hiddenTabs: ["verification"],
      });

      showArtifactTab("c1", "verification");
      expect(selectArtifactState("c1")(useAgentSessionStore.getState())).toMatchObject({
        activeTab: "plan",
        hiddenTabs: [],
      });

      hideArtifactTab("c1", "verification", ["plan", "verification", "tasks"]);
      setArtifactOpen("c1", false);
      revealArtifactTab("c1", "verification");
      expect(selectArtifactState("c1")(useAgentSessionStore.getState())).toEqual({
        isOpen: false,
        activeTab: "verification",
        taskMode: "graph",
        hiddenTabs: [],
      });
    });

    it("uses canonical visible filtering without mutating hidden preferences", () => {
      const availableTabs = ["plan", "verification", "tasks"] as const;
      const hiddenTabs = ["verification"] as const;

      expect(getShownArtifactTabs(availableTabs, hiddenTabs)).toEqual([
        "plan",
        "tasks",
      ]);
      expect(hiddenTabs).toEqual(["verification"]);
    });

    it("focusTaskArtifact opens the Tasks tab and increments focus requests", () => {
      const { focusTaskArtifact, setArtifactState } = useAgentSessionStore.getState();

      setArtifactState("c1", {
        isOpen: false,
        activeTab: "plan",
        taskMode: "graph",
        hiddenTabs: ["tasks"],
      });

      focusTaskArtifact("c1", "task-1");
      let state = useAgentSessionStore.getState();
      expect(selectArtifactState("c1")(state)).toMatchObject({
        isOpen: true,
        activeTab: "tasks",
        hiddenTabs: [],
      });
      expect(state.taskArtifactFocusRequestByConversationId.c1).toEqual({
        taskId: "task-1",
        requestId: 1,
      });

      focusTaskArtifact("c1", "task-2");
      state = useAgentSessionStore.getState();
      expect(state.taskArtifactFocusRequestByConversationId.c1).toEqual({
        taskId: "task-2",
        requestId: 2,
      });
    });

    it("stores and clears automation run focus requests without persisting them", () => {
      const {
        clearAutomationRunFocusRequest,
        requestAutomationRunFocus,
      } = useAgentSessionStore.getState();

      requestAutomationRunFocus("setup-conversation-1", {
        projectId: "project-1",
        automationId: "automation-1",
        runId: "run-1",
        conversationId: "run-conversation-1",
        runStatus: "awaiting_plan_approval",
        judgeState: "none",
        workspaceMode: "plan",
        hasPlanArtifact: true,
        hasPullRequest: false,
        seededTab: "plan",
      });
      let state = useAgentSessionStore.getState();
      expect(state.automationRunFocusRequestByConversationId["setup-conversation-1"]).toEqual({
        projectId: "project-1",
        automationId: "automation-1",
        runId: "run-1",
        conversationId: "run-conversation-1",
        runStatus: "awaiting_plan_approval",
        judgeState: "none",
        workspaceMode: "plan",
        hasPlanArtifact: true,
        hasPullRequest: false,
        seededTab: "plan",
        requestId: 1,
      });

      requestAutomationRunFocus("setup-conversation-1", {
        projectId: "project-1",
        automationId: "automation-1",
        runId: "run-2",
        conversationId: "run-conversation-2",
        runStatus: "published",
        judgeState: "none",
        workspaceMode: null,
        hasPlanArtifact: false,
        hasPullRequest: true,
        seededTab: "pr",
      });
      state = useAgentSessionStore.getState();
      expect(state.automationRunFocusRequestByConversationId["setup-conversation-1"]).toMatchObject({
        runId: "run-2",
        conversationId: "run-conversation-2",
        requestId: 2,
      });

      clearAutomationRunFocusRequest("setup-conversation-1", 1);
      expect(
        useAgentSessionStore.getState().automationRunFocusRequestByConversationId[
          "setup-conversation-1"
        ],
      ).toMatchObject({ runId: "run-2", requestId: 2 });

      clearAutomationRunFocusRequest("setup-conversation-1", 2);
      expect(
        useAgentSessionStore.getState().automationRunFocusRequestByConversationId[
          "setup-conversation-1"
        ],
      ).toBeUndefined();
    });

    it("keeps the exact visible Agent scope transient", () => {
      useAgentSessionStore.getState().setVisibleAgentScope({
        workspaceConversationId: "setup-conversation-1",
        visibleConversationId: "run-conversation-1",
        automationRunId: "run-1",
        automationConversationId: "run-conversation-1",
      });

      expect(useAgentSessionStore.getState().visibleAgentScope).toEqual({
        workspaceConversationId: "setup-conversation-1",
        visibleConversationId: "run-conversation-1",
        automationRunId: "run-1",
        automationConversationId: "run-conversation-1",
      });
      const partialize = useAgentSessionStore.persist.getOptions().partialize;
      const persisted = partialize?.(useAgentSessionStore.getState()) as Record<
        string,
        unknown
      >;
      expect(persisted).not.toHaveProperty("visibleAgentScope");
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
      expect(s1.lastModelEffortByProvider.claude).toEqual({
        modelId: "opus",
        effort: "xhigh",
      });

      setLastRuntimeForProject("p2", { provider: "codex", modelId: "gpt-5.4-mini" });
      expect(useAgentSessionStore.getState().lastRuntimeByProjectId.p2.modelId).toBe(
        "gpt-5.4-mini",
      );
      expect(useAgentSessionStore.getState().lastModelEffortByProvider.codex).toEqual({
        modelId: "gpt-5.4-mini",
        effort: "medium",
      });

      setRuntimeForConversation("c2", "p3", {
        provider: "codex",
        modelId: "gpt-5.6-terra",
        effort: "ultra",
      });
      expect(useAgentSessionStore.getState().runtimeByConversationId.c2).toEqual({
        provider: "codex",
        modelId: "gpt-5.6-terra",
        effort: "max",
      });
      expect(useAgentSessionStore.getState().lastRuntimeByProjectId.p3).toEqual({
        provider: "codex",
        modelId: "gpt-5.6-terra",
        effort: "max",
      });
      expect(useAgentSessionStore.getState().lastModelEffortByProvider.codex).toEqual({
        modelId: "gpt-5.6-terra",
        effort: "max",
      });
    });

    it("clears a project runtime override without changing remembered provider choices", () => {
      const state = useAgentSessionStore.getState();
      state.setRuntimeForConversation("c-reset", "p-reset", {
        provider: "codex",
        modelId: "gpt-5.6",
        effort: "xhigh",
      });

      state.setRoleDefaultRuntimeForConversation("c-reset", "p-reset", {
        provider: "claude",
        modelId: "sonnet",
        effort: "high",
      });

      const cleared = useAgentSessionStore.getState();
      expect(cleared.runtimeByConversationId["c-reset"]).toEqual({
        provider: "claude",
        modelId: "sonnet",
        effort: "high",
      });
      expect(cleared.lastRuntimeByProjectId["p-reset"]).toBeUndefined();
      expect(cleared.lastModelEffortByProvider.claude).toEqual({
        modelId: "sonnet",
        effort: "high",
      });
    });

    it("isolates complete launch overrides by conversation and role and preserves three-state speed", () => {
      const state = useAgentSessionStore.getState();
      const reviewer = {
        provider: "codex",
        model: "gpt-5.5",
        effort: "high",
        serviceTier: "provider_default" as const,
        coordinationMode: "solo" as const,
        personaId: null,
      };
      const repair = {
        ...reviewer,
        serviceTier: "standard" as const,
      };

      state.setRoleRuntimeOverride("conversation-a", "workspace_reviewer", reviewer);
      state.setRoleRuntimeOverride("conversation-a", "workspace_repair", repair);
      state.setRoleRuntimeOverride("conversation-b", "workspace_reviewer", {
        ...reviewer,
        serviceTier: "fast",
      });
      state.setServiceTierForConversation("conversation-a", "provider_default");

      expect(
        useAgentSessionStore.getState().roleRuntimeOverridesByConversationId,
      ).toEqual({
        "conversation-a": {
          workspace_reviewer: reviewer,
          workspace_repair: repair,
        },
        "conversation-b": {
          workspace_reviewer: { ...reviewer, serviceTier: "fast" },
        },
      });
      expect(
        useAgentSessionStore.getState().serviceTierByConversationId["conversation-a"],
      ).toBe("provider_default");

      state.clearRoleRuntimeOverride("conversation-a", "workspace_reviewer");
      state.clearRoleRuntimeOverride("conversation-a", "workspace_repair");
      state.clearServiceTierForConversation("conversation-a");
      expect(
        useAgentSessionStore.getState().roleRuntimeOverridesByConversationId[
          "conversation-a"
        ],
      ).toBeUndefined();
      expect(
        useAgentSessionStore.getState().serviceTierByConversationId["conversation-a"],
      ).toBeUndefined();
    });

    it("remembers branch base cache and selected branch per project", () => {
      const {
        setBranchBaseCacheForProject,
        setLastBranchBaseSelectionForProject,
      } = useAgentSessionStore.getState();

      setBranchBaseCacheForProject(
        "p1",
        [
          {
            key: "project_default:main",
            label: "Project default (main)",
            detail: "Configured project base branch",
            source: "project",
            selection: {
              kind: "project_default",
              ref: "main",
              displayName: "Project default (main)",
            },
          },
          {
            key: "local_branch:feature/cached",
            label: "feature/cached",
            detail: "Local branch",
            source: "local",
            selection: {
              kind: "local_branch",
              ref: "feature/cached",
              displayName: "feature/cached",
            },
          },
        ],
        "project_default:main",
      );
      setLastBranchBaseSelectionForProject("p1", "local_branch:feature/cached");

      const state = useAgentSessionStore.getState();
      expect(state.lastBranchBaseSelectionByProjectId.p1).toBe(
        "local_branch:feature/cached",
      );
      expect(state.branchBaseCacheByProjectId.p1?.selectedKey).toBe(
        "local_branch:feature/cached",
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
