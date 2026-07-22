import type { Page } from "@playwright/test";
export async function seedMergedWorkspace(
  page: Page,
  conversationId: string,
  projectId: string,
) {
  await page.evaluate(
    async ({ conversationId, projectId }) => {
      const {
        mockGetAgentConversationWorkspace,
        mockStartAgentConversation,
        seedMockAgentConversationWorkspace,
        seedMockConversation,
      } = await import("/src/api-mock/chat");
      const createdAt = "2026-07-20T10:00:00.000Z";
      seedMockConversation(
        {
          id: conversationId,
          contextType: "project",
          contextId: projectId,
          claudeSessionId: null,
          providerSessionId: `thread-${conversationId}`,
          providerHarness: "codex",
          upstreamProvider: "openai",
          providerProfile: null,
          agentMode: "edit",
          coordinationMode: "solo",
          title: "Merged publish history",
          messageCount: 0,
          lastMessageAt: null,
          createdAt,
          updatedAt: createdAt,
          archivedAt: null,
        },
        [],
      );
      await mockStartAgentConversation({
        projectId,
        conversationId,
        content: "Seed terminal publish visual",
        providerHarness: "codex",
        modelId: "gpt-5.4",
        mode: "edit",
        base: {
          kind: "project_default",
          ref: "main",
          displayName: "Project default (main)",
        },
      });
      const workspace = await mockGetAgentConversationWorkspace(conversationId);
      if (!workspace || !window.__queryClient) {
        throw new Error("Expected seeded workspace and query client");
      }
      const terminalWorkspace = {
        ...workspace,
        status: "missing" as const,
        publicationPrNumber: 451,
        publicationPrUrl: "https://github.com/mock/project/pull/451",
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      };
      seedMockAgentConversationWorkspace(terminalWorkspace);
      window.__queryClient.removeQueries({ queryKey: ["agents", "sidebar-conversations"] });
      window.__queryClient.removeQueries({ queryKey: ["agents", "project-conversations"] });
    },
    { conversationId, projectId },
  );
}
