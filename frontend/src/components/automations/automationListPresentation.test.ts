import { describe, expect, it } from "vitest";

import type { Automation } from "@/api/automations";
import {
  automationListFilterCounts,
  filterAndGroupAutomations,
  automationListSummary,
} from "./automationListPresentation";

function automation(
  id: string,
  name: string,
  status: Automation["status"],
): Automation {
  return { id, name, status } as Automation;
}

const automations = [
  automation("paused", "Review alerts", "paused"),
  automation("active", "Release train", "active"),
  automation("completed", "Migration", "completed"),
  automation("stopped", "Legacy cleanup", "stopped"),
  automation("draft", "New initiative", "draft"),
];

describe("automation list presentation", () => {
  it("groups every automation in product priority order", () => {
    expect(filterAndGroupAutomations(automations, "all", "").map((group) => [
      group.id,
      group.automations.map((item) => item.id),
    ])).toEqual([
      ["attention", ["paused"]],
      ["running", ["active"]],
      ["finished", ["completed", "stopped"]],
      ["drafts", ["draft"]],
    ]);
  });

  it("keeps filter counts stable while search narrows the active group", () => {
    expect(automationListFilterCounts(automations)).toMatchObject({
      all: 5,
      attention: 1,
      running: 1,
      finished: 2,
      drafts: 1,
    });
    expect(filterAndGroupAutomations(automations, "finished", "legacy")).toEqual([
      expect.objectContaining({
        id: "finished",
        automations: [expect.objectContaining({ id: "stopped" })],
      }),
    ]);
  });

  it("summarizes the project list using running and attention counts", () => {
    expect(automationListSummary("Demo Project", automations)).toBe(
      "Demo Project · 5 automations · 1 running · 1 needs attention",
    );
  });
});
