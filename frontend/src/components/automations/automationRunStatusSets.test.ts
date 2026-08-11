import { describe, expect, it } from "vitest";

import {
  AutomationJudgeStateSchema,
  AutomationRunStatusSchema,
} from "@/api/automations.schemas";
import {
  OPEN_AUTOMATION_RUN_STATUSES,
  OPEN_JUDGE_PENDING_STATES,
  SIGNAL_TERMINAL_AUTOMATION_RUN_STATUSES,
  TERMINAL_AUTOMATION_RUN_STATUSES,
} from "./automationRunStatusSets";

describe("automation run status sets", () => {
  it("stay in lockstep with the zod run-status enum", () => {
    const allStatuses = new Set(AutomationRunStatusSchema.options);
    const openStatuses = new Set<string>(OPEN_AUTOMATION_RUN_STATUSES);
    const signalTerminalStatuses = new Set<string>(
      SIGNAL_TERMINAL_AUTOMATION_RUN_STATUSES,
    );
    const terminalStatuses = new Set<string>(TERMINAL_AUTOMATION_RUN_STATUSES);
    const closedStatuses = AutomationRunStatusSchema.options.filter(
      (status) =>
        !openStatuses.has(status) &&
        !signalTerminalStatuses.has(status) &&
        !terminalStatuses.has(status),
    );
    const covered = new Set([
      ...OPEN_AUTOMATION_RUN_STATUSES,
      ...SIGNAL_TERMINAL_AUTOMATION_RUN_STATUSES,
      ...TERMINAL_AUTOMATION_RUN_STATUSES,
      ...closedStatuses,
    ]);

    expect(covered).toEqual(allStatuses);
    expect(closedStatuses).toEqual([]);
    expect(SIGNAL_TERMINAL_AUTOMATION_RUN_STATUSES).toContain("completed");
    expect(OPEN_AUTOMATION_RUN_STATUSES).toContain("awaiting_plan_approval");
    expect(TERMINAL_AUTOMATION_RUN_STATUSES).not.toContain(
      "awaiting_plan_approval",
    );
  });

  it("keeps open judge-pending states narrower than the full judge enum", () => {
    expect(OPEN_JUDGE_PENDING_STATES).toEqual([
      "none",
      "in_progress",
      "failed",
    ]);
    expect(AutomationJudgeStateSchema.options).toEqual([
      "none",
      "in_progress",
      "done",
      "failed",
      "skipped",
    ]);
  });
});
