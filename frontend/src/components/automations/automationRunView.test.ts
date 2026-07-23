import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import type {
  Automation,
  AutomationRun,
  AutomationJudgeState,
  AutomationRunStatus,
} from "@/api/automations";
import {
  describeAutomationRunPrState,
  getAutomationJudgeRecovery,
  getAutomationRunJudgeLabel,
  getAutomationRunStatusTone,
  getAutomationRunView,
  isAutomationRunDeletable,
  isAutomationRunResumable,
  isAutomationRunComposerReadOnly,
  isIdleAfterCancelledRun,
  latestRunHoldsGoalAuthority,
} from "./automationRunView";

function automation(overrides: Partial<Automation> = {}): Automation {
  const now = "2026-07-05T00:00:00Z";
  return {
    id: "automation-1",
    projectId: "project-1",
    name: "Ship migration loop",
    status: "active",
    pausedReasonCode: null,
    pausedReasonDetail: null,
    goalPrompt: "Keep landing migration tasks.",
    setupConversationId: "conversation-1",
    specArtifactId: null,
    providerHarness: "codex",
    modelId: "gpt-5.4",
    logicalEffort: "high",
    runMode: "edit",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "main",
    baseSourcePullRequestJson: null,
    goalItemsJson: null,
    chainMode: "merged_base",
    completionSignal: "pr_merged",
    planApprovalMode: "manual",
    prMergeMode: "manual",
    planDeepVerification: false,
    maxRuns: 25,
    maxConsecutiveFailures: 3,
    firstRunPrompt: null,
    setupAnalysisSummary: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function run(overrides: Partial<AutomationRun> = {}): AutomationRun {
  const now = "2026-07-05T00:00:00Z";
  return {
    id: "run-1",
    automationId: "automation-1",
    runIndex: 1,
    status: "running",
    judgeState: "none",
    judgeLeaseExpiresAt: null,
    planJudgeState: "none",
    planRevisionRound: 0,
    planRevisionPending: false,
    planPhase: false,
    planArtifactId: null,
    planApprovedBy: null,
    planApprovedArtifactVersion: null,
    planApprovedAt: null,
    conversationId: "conversation-1",
    runPrompt: "Continue the automation.",
    promptAuthor: "judge",
    baseRefKind: "project_default",
    baseRefUsed: "main",
    baseFromRunId: null,
    goalItemId: null,
    branchName: "ralphx/test",
    prNumber: null,
    prUrl: null,
    prTitle: null,
    prHeadRefName: null,
    prBaseRefName: "main",
    prMergedAt: null,
    mergeCommitSha: null,
    diffStatsJson: null,
    agentSummary: null,
    judgeVerdictJson: null,
    judgeModelId: null,
    errorCode: null,
    errorDetail: null,
    signalCheckFailures: 0,
    startedAt: now,
    finishedAt: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function collectSourceFiles(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      collectSourceFiles(path, files);
      continue;
    }
    if (/\.(ts|tsx)$/.test(entry) && !entry.includes(".test.")) {
      files.push(path);
    }
  }
  return files;
}

describe("automationRunView", () => {
  const openStatuses: AutomationRunStatus[] = [
    "pending",
    "provisioning",
    "running",
    "awaiting_plan_approval",
    "published",
  ];
  const signalTerminalStatuses: AutomationRunStatus[] = [
    "merged",
    "pr_closed",
    "agent_failed",
    "completed",
  ];
  const judgePendingStates: AutomationJudgeState[] = [
    "none",
    "in_progress",
    "failed",
  ];

  it.each([
    ["running", "accent"],
    ["pending", "neutral"],
    ["provisioning", "neutral"],
    ["published", "success"],
    ["merged", "success"],
    ["completed", "success"],
    ["awaiting_plan_approval", "warning"],
    ["agent_failed", "error"],
    ["pr_closed", "error"],
    ["cancelled", "neutral"],
  ] as const)("maps %s status to the %s tone", (status, tone) => {
    expect(getAutomationRunStatusTone(run({ status }))).toBe(tone);
  });

  it.each(["running", "agent_failed", "cancelled"] as const)(
    "allows deleting a %s run",
    (status) => {
      expect(isAutomationRunDeletable(run({ status }))).toBe(true);
    },
  );

  it.each([
    "completed",
    "published",
    "merged",
    "pending",
    "provisioning",
    "awaiting_plan_approval",
  ] as const)("rejects deleting a %s run", (status) => {
    expect(isAutomationRunDeletable(run({ status }))).toBe(false);
  });

  it("rejects deletion when there is no run", () => {
    expect(isAutomationRunDeletable(null)).toBe(false);
  });

  it("allows resuming an agent-failed run", () => {
    expect(isAutomationRunResumable(run({ status: "agent_failed" }))).toBe(true);
  });

  it.each([
    "running",
    "completed",
    "published",
    "merged",
    "cancelled",
    "pending",
  ] as const)("rejects resuming a %s run", (status) => {
    expect(isAutomationRunResumable(run({ status }))).toBe(false);
  });

  it("rejects resume when there is no run", () => {
    expect(isAutomationRunResumable(null)).toBe(false);
  });

  it("mirrors goal authority independently from open-run semantics", () => {
    expect(
      latestRunHoldsGoalAuthority(run({ status: "merged", judgeState: "done" })),
    ).toBe(true);
    expect(
      latestRunHoldsGoalAuthority(run({ status: "agent_failed", judgeState: "failed" })),
    ).toBe(false);
    expect(
      latestRunHoldsGoalAuthority(run({ status: "cancelled", judgeState: "none" })),
    ).toBe(false);
  });

  it("keeps parked plan approval runs composer-editable while authority is held", () => {
    const parked = run({ status: "awaiting_plan_approval" });

    expect(latestRunHoldsGoalAuthority(parked)).toBe(true);
    expect(isAutomationRunComposerReadOnly(parked)).toBe(false);
    expect(
      isAutomationRunComposerReadOnly(run({ status: "running", judgeState: "none" })),
    ).toBe(true);
    expect(
      isAutomationRunComposerReadOnly(run({ status: "merged", judgeState: "skipped" })),
    ).toBe(false);
  });

  it("identifies active automations idling after a cancelled latest run", () => {
    expect(isIdleAfterCancelledRun(automation(), run({ status: "cancelled" }))).toBe(true);
    expect(
      isIdleAfterCancelledRun(automation({ status: "paused" }), run({ status: "cancelled" })),
    ).toBe(false);
    expect(
      isIdleAfterCancelledRun(automation({ status: "stopped" }), run({ status: "cancelled" })),
    ).toBe(false);
    expect(
      isIdleAfterCancelledRun(automation({ status: "completed" }), run({ status: "cancelled" })),
    ).toBe(false);
    expect(isIdleAfterCancelledRun(automation(), run({ status: "running" }))).toBe(false);
    expect(isIdleAfterCancelledRun(automation(), null)).toBe(false);
  });

  it("offers judge recovery only for backend-recoverable failed states", () => {
    const automatic = automation({ planApprovalMode: "automatic" });

    expect(
      getAutomationJudgeRecovery(
        automatic,
        run({ status: "awaiting_plan_approval", planJudgeState: "none" }),
      ),
    ).toBeNull();
    expect(
      getAutomationJudgeRecovery(
        automatic,
        run({ status: "awaiting_plan_approval", planJudgeState: "failed" }),
      ),
    ).toEqual(expect.objectContaining({ kind: "plan", actionLabel: "Retry plan judge" }));
    expect(
      getAutomationJudgeRecovery(
        automation(),
        run({ status: "completed", judgeState: "none" }),
      ),
    ).toBeNull();
    expect(
      getAutomationJudgeRecovery(
        automation(),
        run({ status: "completed", judgeState: "failed" }),
      ),
    ).toEqual(
      expect.objectContaining({
        kind: "terminal",
        actionLabel: "Retry terminal judge",
      }),
    );
  });

  it("labels a completed automatic plan judge", () => {
    expect(
      getAutomationRunJudgeLabel(
        run({ status: "awaiting_plan_approval", planJudgeState: "done" }),
        automation({ planApprovalMode: "automatic" }),
      ),
    ).toBe("Plan judge complete");
  });

  it("collects the core run view flags from one selector", () => {
    const view = getAutomationRunView(
      automation(),
      run({ status: "running", planPhase: true }),
    );

    expect(view).toEqual(
      expect.objectContaining({
        isOpen: true,
        isCancellable: true,
        holdsGoalAuthority: true,
        composerReadOnly: true,
        statusLabel: "Running",
        statusTone: "accent",
        judgeLabel: null,
        stageLabel: "Run 1 planning",
        pr: {
          rowLabel: "Current PR",
          value: "Running",
        },
      }),
    );
  });

  it("keeps selector invariants in lockstep across status and judge-state pairs", () => {
    const statuses: AutomationRunStatus[] = [
      "pending",
      "provisioning",
      "running",
      "awaiting_plan_approval",
      "published",
      "completed",
      "merged",
      "pr_closed",
      "agent_failed",
      "cancelled",
    ];
    const judgeStates: AutomationJudgeState[] = [
      "none",
      "in_progress",
      "done",
      "failed",
      "skipped",
    ];

    for (const status of statuses) {
      for (const judgeState of judgeStates) {
        const view = getAutomationRunView(
          automation(),
          run({ status, judgeState, prNumber: 593 }),
        );
        const expectedOpen =
          openStatuses.includes(status) ||
          (signalTerminalStatuses.includes(status) &&
            judgePendingStates.includes(judgeState));

        expect(view.isOpen).toBe(expectedOpen);
        expect(view.pr.rowLabel).toBe(expectedOpen ? "Current PR" : "Last PR");
        if (status === "cancelled") {
          expect(view.stageLabel).not.toBe("Waiting for judge");
        }
      }
    }
  });

  it("uses status-neutral PR copy for cancelled runs", () => {
    const view = getAutomationRunView(
      automation(),
      run({ status: "cancelled", judgeState: "none" }),
    );

    expect(view.judgeLabel).toBeNull();
    expect(view.stageLabel).toBe("Cancelled");
    expect(
      describeAutomationRunPrState(
        run({ status: "cancelled", judgeState: "none", prNumber: 593 }),
      ),
    ).toBe("PR #593 on cancelled run");
    expect(
      describeAutomationRunPrState(run({ status: "published", prNumber: 593 })),
    ).toBe("Current PR #593");
  });

  it("keeps components from calling describeAutomationStage directly", () => {
    const allowedFiles = new Set([
      join(process.cwd(), "src/components/automations/automationRunView.ts"),
      join(process.cwd(), "src/components/automations/automationRunBadges.ts"),
      join(process.cwd(), "src/components/automations/automationStage.ts"),
    ]);
    const offenders = collectSourceFiles(join(process.cwd(), "src/components"))
      .filter((path) => !allowedFiles.has(path))
      .filter((path) => readFileSync(path, "utf8").includes("describeAutomationStage"));

    expect(offenders).toEqual([]);
  });
});
