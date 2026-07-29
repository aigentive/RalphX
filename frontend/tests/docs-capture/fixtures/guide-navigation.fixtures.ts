import { expect, type Page } from "@playwright/test";

export async function openAgents(page: Page): Promise<void> {
  await page.getByTestId("nav-agents").click();
  await expect(page.getByTestId("agents-view")).toBeVisible();
}

export async function openGuideConversation(page: Page, scenario: string): Promise<void> {
  await openAgents(page);
  const row = page.getByTestId(`agents-session-conversation-${scenario}`);
  await expect(row).toBeVisible({ timeout: 15000 });
  await row.getByRole("button").first().click();
  await page.evaluate(async (conversationId) => {
    const { mockGetAgentConversationWorkspace } = await import(
      "/src/api-mock/chat"
    );
    const workspace = await mockGetAgentConversationWorkspace(conversationId);
    if (!workspace || !window.__queryClient) {
      throw new Error("Expected guide workspace query fixture");
    }
    window.__queryClient.setQueryData(
      ["agents", "conversation-workspace", conversationId],
      workspace,
    );
  }, `conversation-${scenario}`);
}

export async function openArtifacts(page: Page): Promise<void> {
  const pane = page.getByTestId("agents-artifact-pane");
  const openButton = page.getByRole("button", { name: "Open artifacts" });
  const closeButton = page.getByRole("button", { name: "Close artifacts" });
  await Promise.race([
    openButton.waitFor({ state: "visible" }),
    closeButton.waitFor({ state: "visible" }),
  ]);
  if (await closeButton.isVisible()) return;
  await openButton.click();
  await expect(pane).toBeVisible();
}

export async function openPublish(page: Page): Promise<void> {
  await openArtifacts(page);
  await page.getByTestId("agents-artifact-tab-publish").click();
  await expect(page.getByTestId("agents-publish-pane")).toBeVisible();
}
