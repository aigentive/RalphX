import type { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { requestAutomationRunOpen } from "@/components/automations/automationRunNavigation";

import { navigateNotification } from "./notificationNavigation";

vi.mock("@/components/automations/automationRunNavigation", () => ({
  requestAutomationRunOpen: vi.fn(),
}));

const target = {
  kind: "automation_run" as const,
  projectId: "project-1",
  automationId: "automation-1",
  runId: "run-1",
  conversationId: "run-conversation-1",
  setupConversationId: "setup-conversation-1",
};

describe("navigateNotification", () => {
  beforeEach(() => vi.clearAllMocks());

  it.each([
    ["automation_plan_approval", "plan"],
    ["automation_run_failed", "automation"],
    ["automation_run_completed", "pr"],
  ] as const)("maps %s to the %s automation tab intent", (category, tabHint) => {
    navigateNotification(
      { id: "notification-1", category, target },
      {} as QueryClient,
    );

    expect(requestAutomationRunOpen).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ setupConversationId: "setup-conversation-1" }),
      expect.objectContaining({ tabHint }),
    );
  });
});
