import { beforeEach, describe, expect, it } from "vitest";

import {
  DEFAULT_GITHUB_DASHBOARD_STATE,
  DEFAULT_GRANOLA_DASHBOARD_STATE,
  useIntegrationDashboardStore,
} from "./integrationDashboardStore";

describe("useIntegrationDashboardStore", () => {
  beforeEach(() => {
    useIntegrationDashboardStore.getState().reset();
  });

  it("keeps GitHub dashboard state per project and resets filters without closing details", () => {
    useIntegrationDashboardStore.getState().setGitHubState("project-1", {
      associationFilter: "tickets",
      statusFilter: "merged",
      searchQuery: "WISE-27",
      selectedBranchName: "agent/work",
    });
    useIntegrationDashboardStore.getState().setGitHubState("project-2", {
      associationFilter: "rx",
      searchQuery: "planning",
    });

    useIntegrationDashboardStore.getState().resetGitHubFilters("project-1");

    expect(useIntegrationDashboardStore.getState().githubByProject["project-1"]).toEqual({
      ...DEFAULT_GITHUB_DASHBOARD_STATE,
      selectedBranchName: "agent/work",
    });
    expect(useIntegrationDashboardStore.getState().githubByProject["project-2"]).toMatchObject({
      associationFilter: "rx",
      searchQuery: "planning",
    });
  });

  it("keeps Granola dashboard state per project and resets filters without changing selection", () => {
    useIntegrationDashboardStore.getState().setGranolaState("project-1", {
      query: "sync",
      noteFilter: "with_tickets",
      selectedNoteId: "note-1",
    });
    useIntegrationDashboardStore.getState().setGranolaState("project-2", {
      query: "roadmap",
      selectedNoteId: "note-2",
    });

    useIntegrationDashboardStore.getState().resetGranolaFilters("project-1");

    expect(useIntegrationDashboardStore.getState().granolaByProject["project-1"]).toEqual({
      ...DEFAULT_GRANOLA_DASHBOARD_STATE,
      selectedNoteId: "note-1",
    });
    expect(useIntegrationDashboardStore.getState().granolaByProject["project-2"]).toMatchObject({
      query: "roadmap",
      selectedNoteId: "note-2",
    });
  });
});
