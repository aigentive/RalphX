import { describe, expect, it } from "vitest";

import {
  getAutomationConversationTabPolicy,
  type AutomationConversationTabAvailability,
} from "./automationConversationTabPolicy";

const baseAvailability: AutomationConversationTabAvailability = {
  hasPlanArtifact: true,
  hasPullRequest: true,
  hasPublishWorkspace: true,
  hasIssues: true,
  hasVerification: true,
  hasTasks: true,
  hasReview: true,
  hasJira: true,
  hasLinear: true,
  hasGranola: true,
  canStartPlan: true,
};

function tabIds(availability: AutomationConversationTabAvailability = baseAvailability) {
  return getAutomationConversationTabPolicy({
    surface: "run",
    runStatus: "running",
    judgeState: "none",
    workspaceMode: "edit",
    availability,
  }).tabs.map((tab) => tab.id);
}

describe("getAutomationConversationTabPolicy", () => {
  it("shows publish but hides unrelated integration tabs for eligible run workspaces", () => {
    expect(tabIds()).toEqual(["automation", "plan", "pr", "publish"]);
  });

  it("hides publish when the run has no publishable workspace", () => {
    expect(tabIds({ ...baseAvailability, hasPublishWorkspace: false })).toEqual([
      "automation",
      "plan",
      "pr",
    ]);
  });

  it("keeps the plan tab visible but disabled until the run plan exists", () => {
    const policy = getAutomationConversationTabPolicy({
      surface: "run",
      runStatus: "running",
      judgeState: "none",
      workspaceMode: "edit",
      availability: { ...baseAvailability, hasPlanArtifact: false },
    });

    expect(policy.tabs).toContainEqual({
      id: "plan",
      enabled: false,
      disabledReason: "No run plan has been authored yet.",
    });
  });

  it("defaults parked and plan-phase running runs to the plan tab", () => {
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "awaiting_plan_approval",
        judgeState: "none",
        workspaceMode: "plan",
        availability: baseAvailability,
      }).defaultTab,
    ).toBe("plan");
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "running",
        judgeState: "none",
        workspaceMode: "plan",
        availability: baseAvailability,
      }).defaultTab,
    ).toBe("plan");
  });

  it("defaults published and judge-settling PR runs to the PR tab", () => {
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "published",
        judgeState: "none",
        workspaceMode: "edit",
        availability: baseAvailability,
      }).defaultTab,
    ).toBe("pr");
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "merged",
        judgeState: "in_progress",
        workspaceMode: "edit",
        availability: baseAvailability,
      }).defaultTab,
    ).toBe("pr");
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "completed",
        judgeState: "none",
        workspaceMode: "edit",
        availability: baseAvailability,
      }).defaultTab,
    ).toBe("pr");
  });

  it("defaults implementing, failed-judge, and terminal runs to automation", () => {
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "running",
        judgeState: "none",
        workspaceMode: "edit",
        availability: baseAvailability,
      }).defaultTab,
    ).toBe("automation");
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "merged",
        judgeState: "failed",
        workspaceMode: "edit",
        availability: baseAvailability,
      }).defaultTab,
    ).toBe("automation");
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "agent_failed",
        judgeState: "none",
        workspaceMode: "edit",
        availability: baseAvailability,
      }).defaultTab,
    ).toBe("automation");
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "cancelled",
        judgeState: "none",
        workspaceMode: "edit",
        availability: baseAvailability,
      }).defaultTab,
    ).toBe("automation");
  });

  it("gives an explicit caller tab hint precedence over synthesized run state", () => {
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: null,
        judgeState: null,
        workspaceMode: null,
        availability: { ...baseAvailability, hasPlanArtifact: false, hasPullRequest: false },
        tabHint: "plan",
      }).defaultTab,
    ).toBe("plan");
    expect(
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: "awaiting_plan_approval",
        judgeState: "none",
        workspaceMode: "plan",
        availability: baseAvailability,
        tabHint: "automation",
      }).defaultTab,
    ).toBe("automation");
  });

  it("keeps terminal runs with authored plans plan-tab enabled", () => {
    const policy = getAutomationConversationTabPolicy({
      surface: "run",
      runStatus: "cancelled",
      judgeState: "none",
      workspaceMode: "edit",
      availability: baseAvailability,
    });

    expect(policy.tabs).toContainEqual({ id: "plan", enabled: true });
  });

  it("keeps setup conversations on the broader tab set", () => {
    const policy = getAutomationConversationTabPolicy({
      surface: "setup",
      runStatus: null,
      judgeState: null,
      workspaceMode: "automation",
      availability: baseAvailability,
    });

    expect(policy.tabs.map((tab) => tab.id)).toContain("publish");
    expect(policy.tabs.map((tab) => tab.id)).toContain("jira");
    expect(policy.tabs.map((tab) => tab.id)).toContain("automation");
  });

  it("hides setup publishing when the setup workspace is not publishable", () => {
    const policy = getAutomationConversationTabPolicy({
      surface: "setup",
      runStatus: null,
      judgeState: null,
      workspaceMode: "automation",
      availability: { ...baseAvailability, hasPublishWorkspace: false },
    });

    expect(policy.tabs.map((tab) => tab.id)).not.toContain("publish");
  });
});
