import type { Page } from "@playwright/test";

export async function seedAutomationRuntimeVisualState(
  page: Page,
  input: { automationId: string; conversationId: string; projectId: string },
) {
  await page.evaluate(async ({ automationId, conversationId, projectId }) => {
    if (!window.__queryClient) {
      throw new Error("Expected query client for automation runtime fixture");
    }
    const { mockAutomationsApi } = await import("/src/api-mock/automations");
    const detail = await mockAutomationsApi.get(automationId);
    const baseRun = await mockAutomationsApi.cancelRun({
      id: automationId,
      runId: `${automationId}-run-base`,
    });
    const runs = [
      {
        ...baseRun,
        id: `${automationId}-run-2`,
        runIndex: 2,
        status: "awaiting_plan_approval" as const,
        judgeState: "none" as const,
        planPhase: true,
        planArtifactId: `${automationId}-plan-2`,
        conversationId: `${automationId}-conversation-2`,
        runPrompt: "Verify the release plan.",
        promptAuthor: "judge" as const,
        startedAt: "2026-07-22T10:45:00.000Z",
        finishedAt: null,
      },
      {
        ...baseRun,
        id: `${automationId}-run-1`,
        runIndex: 1,
        status: "merged" as const,
        judgeState: "done" as const,
        planPhase: false,
        conversationId: `${automationId}-conversation-1`,
        branchName: "ralphx/release/run-1",
        prNumber: 742,
        prUrl: "https://github.com/aigentive/ralphx.app/pull/742",
        prMergedAt: "2026-07-22T10:30:00.000Z",
        startedAt: "2026-07-22T10:00:00.000Z",
        finishedAt: "2026-07-22T10:30:00.000Z",
      },
    ];
    window.__queryClient.setQueryData(
      ["automations", "detail", automationId],
      {
        ...detail,
        automation: {
          ...detail.automation,
          id: automationId,
          projectId,
          name: "Release readiness",
          status: "active",
          setupConversationId: conversationId,
          goalPrompt: "Keep the release ready.",
          modelId: "gpt-5.6",
          logicalEffort: "high",
          maxRuns: 5,
        },
        runs,
      },
    );
    window.__queryClient.setQueryData(
      ["agents", "conversation-runtime-index", conversationId],
      {
        conversationId,
        rows: [{
          id: `workspace:${conversationId}`,
          group: "main",
          kind: "workspace",
          lifecycle: "running",
          statusLabel: "Running",
          title: "Workspace chat",
          mode: "agent",
          orderIndex: 0,
          orderStartedAt: "2026-07-22T10:00:00.000Z",
          completedAt: null,
          conversationId,
          contextType: "project",
          contextId: conversationId,
          taskId: null,
          agentRunId: `${automationId}-agent-run`,
          parentSessionId: null,
          childSessionId: null,
          providerHarness: "codex",
          providerSessionId: `${automationId}-session`,
          errorMessage: null,
        }],
      },
    );
  }, input);
}
