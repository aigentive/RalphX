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

/** Budget for one open/click attempt, short enough to leave room for retries. */
const ARTIFACT_STEP_TIMEOUT_MS = 2_000;
/** Budget for the pane to settle into a stable open state. */
const ARTIFACT_SETTLE_TIMEOUT_MS = 15_000;

/**
 * The header control is a toggle, and the pane also auto-opens on its own once
 * the conversation's attached ideation session resolves. A single click that
 * lands inside that window reads the freshly auto-opened state and closes the
 * pane again — on slower machines that leaves the tab row mounted but hidden,
 * so later tab clicks fail with "element is not visible" or "detached from the
 * DOM". Re-assert until the pane is actually open.
 */
export async function openArtifacts(page: Page): Promise<void> {
  const pane = page.getByTestId("agents-artifact-pane");
  const openButton = page.getByRole("button", { name: "Open artifacts" });
  await expect(async () => {
    if (!(await pane.isVisible())) {
      await openButton.click({ timeout: ARTIFACT_STEP_TIMEOUT_MS });
    }
    await expect(pane).toBeVisible({ timeout: ARTIFACT_STEP_TIMEOUT_MS });
  }).toPass({ timeout: ARTIFACT_SETTLE_TIMEOUT_MS });
}

/**
 * Opens the artifact pane and selects a tab, retrying the pair together so a
 * pane that re-closes between the two steps is reopened instead of failing.
 */
export async function openArtifactTab(page: Page, tab: string): Promise<void> {
  const pane = page.getByTestId("agents-artifact-pane");
  const openButton = page.getByRole("button", { name: "Open artifacts" });
  const tabButton = page.getByTestId(`agents-artifact-tab-${tab}`);
  await expect(async () => {
    if (!(await pane.isVisible())) {
      await openButton.click({ timeout: ARTIFACT_STEP_TIMEOUT_MS });
      await expect(pane).toBeVisible({ timeout: ARTIFACT_STEP_TIMEOUT_MS });
    }
    await tabButton.click({ timeout: ARTIFACT_STEP_TIMEOUT_MS });
  }).toPass({ timeout: ARTIFACT_SETTLE_TIMEOUT_MS });
}

export async function openPublish(page: Page): Promise<void> {
  await openArtifactTab(page, "publish");
  await expect(page.getByTestId("agents-publish-pane")).toBeVisible();
}
