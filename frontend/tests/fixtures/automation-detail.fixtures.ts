import type { Page } from "@playwright/test";

/** Seeds the automations list + detail caches with a three-run automation. */
export async function seedAutomationDetailVisualState(
  page: Page,
  input: { automationId: string; projectId: string },
) {
  await page.evaluate(async ({ automationId, projectId }) => {
    if (!window.__queryClient) {
      throw new Error("Expected query client for automation detail fixture");
    }
    const { mockAutomationsApi } = await import("/src/api-mock/automations");
    const detail = await mockAutomationsApi.get(automationId);
    const baseRun = await mockAutomationsApi.cancelRun({
      id: automationId,
      runId: `${automationId}-run-base`,
    });
    const goalItems = [
      { id: "phase-1", title: "F0 — Correct docs", status: "done" },
      { id: "phase-2", title: "B1 — Skill schema versioning", status: "in_progress" },
      { id: "phase-3", title: "B2 — Update-over-create", status: "pending" },
    ];
    const runs = [
      {
        ...baseRun,
        id: `${automationId}-run-3`, runIndex: 3,
        status: "running" as const, judgeState: "none" as const,
        goalItemId: "phase-2", planArtifactId: `${automationId}-plan-3`,
        conversationId: `${automationId}-conversation-3`,
        branchName: "ralphx/agent-88af9c08", prNumber: null, prUrl: null,
        runPrompt: "Implement B1 skill schema versioning.",
        promptAuthor: "judge" as const, agentSummary: null,
        startedAt: "2026-07-22T12:39:00.000Z", finishedAt: null,
        updatedAt: "2026-07-22T12:39:00.000Z",
      },
      {
        ...baseRun,
        id: `${automationId}-run-2`, runIndex: 2,
        status: "merged" as const, judgeState: "done" as const,
        goalItemId: "phase-1", planArtifactId: `${automationId}-plan-2`,
        conversationId: `${automationId}-conversation-2`,
        branchName: "ralphx/agent-14be02aa", prNumber: 841,
        prUrl: "https://github.com/aigentive/ralphx.app/pull/841",
        prMergedAt: "2026-07-22T11:30:00.000Z",
        diffStatsJson: JSON.stringify({ filesChanged: 4, additions: 62, deletions: 18 }),
        agentSummary: "Corrected the docs and merged PR #841.",
        startedAt: "2026-07-22T11:00:00.000Z",
        finishedAt: "2026-07-22T11:30:00.000Z",
        updatedAt: "2026-07-22T11:30:00.000Z",
      },
      {
        ...baseRun,
        id: `${automationId}-run-1`, runIndex: 1,
        status: "agent_failed" as const, judgeState: "failed" as const,
        conversationId: `${automationId}-conversation-1`,
        errorCode: "publish_failed", errorDetail: "Publish step exited with code 1",
        agentSummary: "Attempted the docs pass but publish failed.",
        startedAt: "2026-07-22T10:00:00.000Z",
        finishedAt: "2026-07-22T10:20:00.000Z",
        updatedAt: "2026-07-22T10:20:00.000Z",
      },
    ];
    const automation = {
      ...detail.automation,
      id: automationId, projectId,
      name: "Release readiness", status: "active" as const,
      goalPrompt: "Keep the release branch ready to ship.",
      goalItemsJson: JSON.stringify(goalItems),
      baseRef: "main", baseDisplayName: "main",
    };
    window.__queryClient.setQueryData(
      ["automations", "detail", automationId],
      { ...detail, automation, runs },
    );
    window.__queryClient.setQueryData(
      ["automations", "list", projectId],
      [automation],
    );
    for (const planId of [`${automationId}-plan-2`, `${automationId}-plan-3`]) {
      window.__queryClient.setQueryData(["artifacts", "detail", planId], {
        id: planId, name: "Run plan",
        artifact_type: "specification", content_type: "inline",
        content: "## Plan\n\n1. Version the skill schema.\n2. Migrate readers.\n3. Add regression coverage.",
        created_at: "2026-07-22T12:40:00.000Z", created_by: "orchestrator",
        version: 1, bucket_id: null, task_id: null, process_id: null,
        derived_from: [],
      });
    }
  }, input);
}
