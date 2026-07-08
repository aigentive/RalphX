import { describe, expect, it } from "vitest";

import { AutomationRunStatusSchema } from "@/api/automations.schemas";
import {
  OPEN_AUTOMATION_RUN_STATUSES,
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
    expect(closedStatuses).toEqual(["completed"]);
    expect(OPEN_AUTOMATION_RUN_STATUSES).toContain("awaiting_plan_approval");
    expect(TERMINAL_AUTOMATION_RUN_STATUSES).not.toContain(
      "awaiting_plan_approval",
    );
  });
});
