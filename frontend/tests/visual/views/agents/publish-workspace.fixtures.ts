import type { Page } from "@playwright/test";

/**
 * Returns the conversation id behind the currently active
 * agents-conversation-workspace query, seeded by
 * AgentsPublishPage#openRepairPendingScenario. Avoids depending on that
 * scenario's private conversation-id constant.
 */
export async function getActiveWorkspaceConversationId(
  page: Page,
): Promise<string> {
  return page.evaluate(() => {
    const queryClient = window.__queryClient;
    if (!queryClient) {
      throw new Error("Expected an active query client");
    }
    const activeWorkspaceQuery = queryClient
      .getQueryCache()
      .findAll({ queryKey: ["agents", "conversation-workspace"] })
      .find(
        (query) =>
          query.queryKey.length === 3 &&
          query.queryKey[2] != null &&
          query.state.data !== undefined,
      );
    if (!activeWorkspaceQuery) {
      throw new Error("Expected a seeded conversation-workspace query");
    }
    return activeWorkspaceQuery.queryKey[2] as string;
  });
}
