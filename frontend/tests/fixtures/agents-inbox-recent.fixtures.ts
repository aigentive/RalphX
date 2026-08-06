import type { Page } from "@playwright/test";

export type RecentInboxScenario = "populated" | "empty-needs" | "zero" |
  "stale-zero" | "done-zero" | "filtered-zero";

export async function seedRecentInboxScenario(
  page: Page,
  scenario: RecentInboxScenario,
): Promise<void> {
  await page.evaluate(async (seededScenario) => {
    const projectId = "project-mock-1";
    const now = new Date().toISOString();
    const {
      mockStartAgentConversation,
      seedMockAgentConversationWorkspace,
      seedMockConversation,
    } = await import("/src/api-mock/chat");

    window.__mockChatApi?.reset();
    const seedConversation = async (
      id: string,
      title: string,
      lane: "needs" | "working" | "done",
    ) => {
      seedMockConversation(
        {
          id,
          contextType: "project",
          contextId: projectId,
          claudeSessionId: null,
          providerSessionId: `thread-${id}`,
          providerHarness: "codex",
          upstreamProvider: "openai",
          providerProfile: null,
          agentMode: "edit",
          automationId: null,
          automationRunId: null,
          coordinationMode: "solo",
          title,
          messageCount: 0,
          lastMessageAt: now,
          createdAt: now,
          updatedAt: now,
          archivedAt: null,
        },
        [],
      );
      const result = await mockStartAgentConversation({
        projectId,
        content: "Seed Recent inbox visual state",
        conversationId: id,
        providerHarness: "codex",
        modelId: "gpt-5.4",
        mode: "edit",
        base: { kind: "current_branch", ref: "main", displayName: "main" },
      });
      if (result.workspace && lane !== "needs") {
        seedMockAgentConversationWorkspace({
          ...result.workspace,
          ...(lane === "working"
            ? { prSupervisionStatus: "monitoring" }
            : { publicationPrStatus: "merged" }),
        });
      }
    };

    const denseRecent =
      seededScenario === "populated" || seededScenario === "filtered-zero";
    const needsCount = denseRecent
      ? 10
      : seededScenario === "zero" || seededScenario === "empty-needs"
        ? 0
        : 2;
    const workingCount =
      seededScenario === "zero"
        ? 0
        : denseRecent || seededScenario === "empty-needs"
          ? 3
          : 1;
    const doneCount = seededScenario === "zero" ? 7 : 0;
    await Promise.all([
      ...Array.from({ length: needsCount }, (_, index) =>
        seedConversation(
          `recent-needs-${index + 1}`, `Needs you: review ${index + 1}`, "needs",
        ),
      ),
      ...Array.from({ length: workingCount }, (_, index) =>
        seedConversation(
          `recent-working-${index + 1}`, `Working: supervised PR ${index + 1}`, "working",
        ),
      ),
      ...Array.from({ length: doneCount }, (_, index) =>
        seedConversation(
          `recent-done-${index + 1}`, `Done: merged PR ${index + 1}`, "done",
        ),
      ),
    ]);
  }, scenario);
}
