import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ProjectPrInsights } from "@/types/project-stats";
import { PrPerformanceInsightsCard } from "./PrPerformanceInsightsCard";

const prInsights: ProjectPrInsights = {
  summary: {
    totalPrs: 8,
    directWorkspacePrs: 5,
    taskPipelinePrs: 3,
    executionOwnedWorkspaceRefs: 2,
    mergedPrs: 6,
    openPrs: 1,
    draftPrs: 0,
    changesRequestedPrs: 1,
    closedPrs: 1,
    needsAgentPrs: 1,
    unpushedWorkspacePrs: 0,
    totalWorkspaces: 10,
    directWorkspaces: 8,
    directWorkspacesWithPrs: 5,
    directWorkspacePrConversionRate: 0.625,
    terminalMergeRate: 6 / 7,
    avgWorkspacePrCycleHours: 20,
    avgPlanPrWaitHours: 14,
    requestedChangesEvents: 2,
    autofixNeededEvents: 1,
    agentFixCompletedEvents: 1,
    supervisionEnabledWorkspaces: 4,
    autoMergeDesiredWorkspaces: 3,
    autoMergeActiveWorkspaces: 2,
  },
  origins: [
    {
      origin: "agent_workspace_direct",
      label: "Agent workspace",
      countedInTotals: true,
      totalPrs: 5,
      mergedPrs: 4,
      openPrs: 1,
      draftPrs: 0,
      changesRequestedPrs: 0,
      closedPrs: 0,
      needsAgentPrs: 0,
      unpushedWorkspacePrs: 0,
    },
    {
      origin: "task_pipeline_pr_mode",
      label: "Task pipeline PR mode",
      countedInTotals: true,
      totalPrs: 3,
      mergedPrs: 2,
      openPrs: 0,
      draftPrs: 0,
      changesRequestedPrs: 1,
      closedPrs: 1,
      needsAgentPrs: 1,
      unpushedWorkspacePrs: 0,
    },
    {
      origin: "agent_workspace_execution_owned",
      label: "Execution-owned workspace",
      countedInTotals: false,
      totalPrs: 2,
      mergedPrs: 2,
      openPrs: 0,
      draftPrs: 0,
      changesRequestedPrs: 0,
      closedPrs: 0,
      needsAgentPrs: 0,
      unpushedWorkspacePrs: 0,
    },
  ],
  weeklyThroughput: [
    { weekStart: "2026-05-03", opened: 2, merged: 1, sampleSize: 3 },
    { weekStart: "2026-05-10", opened: 3, merged: 4, sampleSize: 7 },
  ],
  workspaceDwellTimes: [],
  latestPrs: [
    {
      origin: "agent_workspace_direct",
      label: "Agent workspace",
      countedInTotals: true,
      status: "merged",
      prNumber: 42,
      prUrl: "https://github.test/org/repo/pull/42",
      branchName: "rx/direct",
      baseRef: "main",
      conversationId: "conversation-1",
      taskId: null,
      planBranchId: null,
      createdAt: "2026-05-10T12:00:00Z",
      updatedAt: "2026-05-11T12:00:00Z",
      mergedAt: "2026-05-11T12:00:00Z",
    },
  ],
};

describe("PrPerformanceInsightsCard", () => {
  it("renders PR velocity, outcomes, rework, and conversion without cost copy", () => {
    render(<PrPerformanceInsightsCard insights={prInsights} />);

    expect(screen.getByText("PR Performance")).toBeInTheDocument();
    expect(screen.getByText("3 opened · 4 merged week of May 10")).toBeInTheDocument();
    expect(screen.getByText("8")).toBeInTheDocument();
    expect(screen.getByText("6")).toBeInTheDocument();
    expect(screen.getByText("86% terminal merge rate")).toBeInTheDocument();
    expect(screen.getByText("63%")).toBeInTheDocument();
    expect(screen.getByText("Agent workspace: 5 PRs")).toBeInTheDocument();
    expect(screen.getByText("Task pipeline PR mode: 3 PRs")).toBeInTheDocument();
    expect(screen.getByText("2 execution-owned workspace refs deduped")).toBeInTheDocument();
    expect(screen.getByText("Requested changes: 2")).toBeInTheDocument();
    expect(screen.getByText("Autofix routed: 1")).toBeInTheDocument();
    expect(screen.queryByText(/cost/i)).not.toBeInTheDocument();
  });
});
