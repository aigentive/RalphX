import type { Page } from "@playwright/test";
import type { AgentWorkspacePrAutofixFingerprintSpend } from "@/api/chat";

import { getActiveWorkspaceConversationId } from "./publish-workspace.fixtures";

/**
 * Patches the already-seeded conversation-workspace query (from
 * openRepairPendingScenario) with an explicit autofix-fingerprint-spend
 * value so AgentsPublishAutomationTab's budget panel can be exercised in
 * both its reportable and non-reportable ("Some" with zeros, not null)
 * states.
 */
export async function seedAutomationSpend(
  page: Page,
  spend: AgentWorkspacePrAutofixFingerprintSpend,
) {
  const conversationId = await getActiveWorkspaceConversationId(page);
  await page.evaluate(
    async ({ conversationId, spend }) => {
      const { seedMockAgentConversationWorkspace } = await import(
        "/src/api-mock/chat"
      );
      const { agentWorkspaceKeys } = await import(
        "/src/components/agents/agentWorkspaceQueries"
      );
      const queryClient = window.__queryClient;
      if (!queryClient) {
        throw new Error("Expected an active query client");
      }
      const workspaceKey = agentWorkspaceKeys.workspace(conversationId);
      const workspace =
        queryClient.getQueryData<Record<string, unknown>>(workspaceKey);
      if (!workspace) {
        throw new Error("Expected seeded workspace before applying spend override");
      }
      const nextWorkspace = { ...workspace, prAutofixFingerprintSpend: spend };
      seedMockAgentConversationWorkspace(nextWorkspace);
      queryClient.setQueryData(workspaceKey, nextWorkspace);
    },
    { conversationId, spend },
  );
}
