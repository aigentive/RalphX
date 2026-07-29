import type { Page } from "@playwright/test";

/**
 * Seeds a workspace stuck in the repair-pending publish state: the backend has
 * routed it back to the agent (`needs_agent`) and PR supervision is blocked.
 * Active maintenance must replace that stale PR-supervision presentation.
 */
export async function seedRepairPendingWorkspace(
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
          title: "Repair pending workspace",
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
        content: "Seed repair pending visual",
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
      const repairWorkspace = {
        ...workspace,
        mode: "edit" as const,
        publicationPrNumber: 78,
        publicationPrUrl: "https://github.com/mock/project/pull/78",
        publicationPrStatus: null,
        publicationPushStatus: "needs_agent",
        prSupervisionStatus: "blocked",
        prSupervisionSummary: "GitHub reported PR supervision is blocked.",
      };
      seedMockAgentConversationWorkspace(repairWorkspace);
      await window.__queryClient.cancelQueries({
        queryKey: ["agents", "sidebar-conversations"],
      });
      window.__queryClient.removeQueries({
        queryKey: ["agents", "sidebar-conversations"],
      });
      window.__queryClient.removeQueries({
        queryKey: ["agents", "project-conversations"],
      });
    },
    { conversationId, projectId },
  );
}
