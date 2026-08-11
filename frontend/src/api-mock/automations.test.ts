import { describe, expect, it } from "vitest";

import { mockAutomationsApi } from "./automations";

describe("mockAutomationsApi", () => {
  it("returns deterministic list and detail fixtures", async () => {
    await expect(mockAutomationsApi.list({ projectId: "project-1" })).resolves.toEqual(
      expect.arrayContaining([
        expect.objectContaining({ status: "paused" }),
        expect.objectContaining({ status: "active" }),
        expect.objectContaining({ status: "completed" }),
        expect.objectContaining({ status: "draft" }),
      ]),
    );

    await expect(mockAutomationsApi.get("automation-42")).resolves.toMatchObject({
      automation: {
        id: "automation-42",
        projectId: "mock-project",
        status: "draft",
      },
      runs: [],
      usage: {
        inputTokens: 0,
        outputTokens: 0,
        estimatedUsd: null,
      },
    });
  });

  it("mirrors create and settings inputs into automation fixtures", async () => {
    await expect(
      mockAutomationsApi.createDraft({
        projectId: "project-7",
        name: "Nightly cleanup",
      }),
    ).resolves.toMatchObject({
      automation: {
        projectId: "project-7",
        name: "Nightly cleanup",
      },
      setupConversationId: null,
    });

    await expect(
      mockAutomationsApi.updateSettings({
        id: "automation-7",
        name: "Renamed",
        maxRuns: 9,
        maxConsecutiveFailures: 2,
        planApprovalMode: "automatic",
        prMergeMode: "automatic",
        planDeepVerification: true,
      }),
    ).resolves.toMatchObject({
      id: "automation-7",
      name: "Renamed",
      maxRuns: 9,
      maxConsecutiveFailures: 2,
      planApprovalMode: "automatic",
      prMergeMode: "automatic",
      planDeepVerification: true,
    });
  });

  it("returns control-state fixtures for automation actions", async () => {
    await expect(
      mockAutomationsApi.pause({
        id: "automation-1",
        reasonCode: "release_freeze",
        reasonDetail: "Waiting on base branch",
      }),
    ).resolves.toMatchObject({
      id: "automation-1",
      status: "paused",
      pausedReasonCode: "release_freeze",
      pausedReasonDetail: "Waiting on base branch",
    });
    await expect(mockAutomationsApi.pause({ id: "automation-2" })).resolves.toMatchObject({
      pausedReasonCode: "user_paused",
      pausedReasonDetail: null,
    });
    await expect(mockAutomationsApi.resume("automation-1")).resolves.toMatchObject({
      id: "automation-1",
      status: "active",
    });
    await expect(mockAutomationsApi.stop("automation-1")).resolves.toMatchObject({
      id: "automation-1",
      status: "stopped",
    });
    await expect(mockAutomationsApi.cancelRun({
      id: "automation-1",
      runId: "run-99",
    })).resolves.toMatchObject({
      automationId: "automation-1",
      id: "run-99",
      status: "cancelled",
    });
    await expect(mockAutomationsApi.delete("automation-1")).resolves.toBeUndefined();
  });

  it("exposes scheduler placeholder responses and setup-agent helpers", async () => {
    await expect(mockAutomationsApi.triggerRunNow("automation-1")).resolves.toMatchObject({
      scheduled: false,
      reason: expect.stringContaining("scheduler phase"),
    });
    await expect(
      mockAutomationsApi.skipJudge({ id: "automation-1", runId: "run-1" }),
    ).resolves.toMatchObject({
      scheduled: false,
      reason: expect.stringContaining("judge phase"),
    });

    await expect(
      mockAutomationsApi.setupAgent.getAutomation("conversation-1"),
    ).resolves.toMatchObject({
      automation: { id: "mock-automation-1" },
      runs: [],
    });
    await expect(
      mockAutomationsApi.setupAgent.updateAutomation("conversation-1", {
        name: "Setup updated",
        maxRuns: 13,
        maxConsecutiveFailures: 4,
      }),
    ).resolves.toMatchObject({
      name: "Setup updated",
      maxRuns: 13,
      maxConsecutiveFailures: 4,
    });
    await expect(
      mockAutomationsApi.setupAgent.finalizeAutomation("conversation-1"),
    ).resolves.toMatchObject({
      status: "active",
    });
  });
});
