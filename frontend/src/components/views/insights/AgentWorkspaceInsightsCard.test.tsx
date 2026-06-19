import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ProjectPrInsights } from "@/types/project-stats";
import { AgentWorkspaceInsightsCard } from "./AgentWorkspaceInsightsCard";

const insights: ProjectPrInsights = {
  summary: {
    totalPrs: 4,
    directWorkspacePrs: 3,
    taskPipelinePrs: 1,
    executionOwnedWorkspaceRefs: 1,
    mergedPrs: 3,
    openPrs: 1,
    draftPrs: 0,
    changesRequestedPrs: 0,
    closedPrs: 0,
    needsAgentPrs: 1,
    unpushedWorkspacePrs: 0,
    totalWorkspaces: 5,
    directWorkspaces: 4,
    directWorkspacesWithPrs: 3,
    directWorkspacePrConversionRate: 0.75,
    terminalMergeRate: 1,
    avgWorkspacePrCycleHours: 18,
    avgPlanPrWaitHours: 9,
    requestedChangesEvents: 1,
    autofixNeededEvents: 1,
    agentFixCompletedEvents: 1,
    supervisionEnabledWorkspaces: 2,
    autoMergeDesiredWorkspaces: 1,
    autoMergeActiveWorkspaces: 1,
  },
  origins: [
    {
      origin: "agent_workspace_direct",
      label: "Agent workspace",
      countedInTotals: true,
      totalPrs: 3,
      mergedPrs: 2,
      openPrs: 1,
      draftPrs: 0,
      changesRequestedPrs: 0,
      closedPrs: 0,
      needsAgentPrs: 1,
      unpushedWorkspacePrs: 0,
    },
  ],
  weeklyThroughput: [],
  workspaceDwellTimes: [
    {
      stateFamily: "publication_push_status",
      state: "pushed",
      label: "Publication: Pushed",
      avgMinutes: 180,
      sampleSize: 2,
    },
  ],
  latestPrs: [],
};

describe("AgentWorkspaceInsightsCard", () => {
  it("renders workspace conversion, merge output, and dwell time without cost copy", () => {
    render(<AgentWorkspaceInsightsCard insights={insights} />);

    expect(screen.getByText("Agent Workspaces")).toBeInTheDocument();
    expect(screen.getByText("75%")).toBeInTheDocument();
    expect(screen.getByText("Publication: Pushed")).toBeInTheDocument();
    expect(screen.getByText("3h")).toBeInTheDocument();
    expect(screen.queryByText(/cost/i)).not.toBeInTheDocument();
  });
});
