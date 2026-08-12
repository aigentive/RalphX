import type { Page } from "@playwright/test";
import type {
  AgentConversationWorkspacePublicationEvent,
  AgentWorkspaceMaintenanceOperationHoldReason,
  AgentWorkspacePrAutofixFingerprintSpend,
} from "@/api/chat";

import { getActiveWorkspaceConversationId } from "./publish-workspace.fixtures";

export type HeldWorkspaceOverrides = {
  holdReason: AgentWorkspaceMaintenanceOperationHoldReason;
  prAutofixFingerprintSpend?: AgentWorkspacePrAutofixFingerprintSpend | null;
  timelineEvents?: AgentConversationWorkspacePublicationEvent[];
};

/**
 * Patches the already-seeded conversation-workspace query (from
 * openRepairPendingScenario) into a held maintenance operation for the given
 * hold reason.
 */
export async function seedHeldWorkspaceScenario(
  page: Page,
  overrides: HeldWorkspaceOverrides,
) {
  const conversationId = await getActiveWorkspaceConversationId(page);
  await page.evaluate(
    async ({ conversationId, input }) => {
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
        throw new Error("Expected seeded workspace before applying hold override");
      }
      const heldWorkspace = {
        ...workspace,
        publicationPushStatus: "refreshed",
        prSupervisionStatus: "held",
        maintenanceOperation: {
          operationId: `maintenance-held-${input.holdReason}`,
          generation: 2,
          source: "pr_autofix",
          stage: "held",
          status: "held",
          holdReason: input.holdReason,
          summary: "RalphX paused this repair.",
          blocker: null,
          automaticContinuation: false,
          startedAt: "2026-08-02T10:00:00Z",
          updatedAt: "2026-08-02T10:01:00Z",
        },
        prAutofixFingerprintSpend: input.prAutofixFingerprintSpend ?? null,
      };
      seedMockAgentConversationWorkspace(heldWorkspace);
      queryClient.setQueryData(workspaceKey, heldWorkspace);
      if (input.timelineEvents) {
        // AgentsPublishHoldCard reads its Details timeline from this inline
        // query key; there is no exported factory, so keep this literal in
        // sync with AgentsPublishHoldCard.tsx.
        queryClient.setQueryData(
          ["agents-publish-hold-timeline", conversationId],
          input.timelineEvents,
        );
      }
    },
    { conversationId, input: overrides },
  );
}
