import { expect, type Locator, type Page } from "@playwright/test";

const AGENT_INBOX_LANES = ["needs", "working", "stale", "done"] as const;
const INBOX_CONVERSATION_TIMEOUT_MS = 15_000;

export async function revealAgentInboxConversation(
  page: Page,
  conversationId: string,
): Promise<Locator> {
  const row = page.getByTestId(`agents-session-${conversationId}`);
  if (await row.isVisible()) {
    return row;
  }

  await expect(page.getByTestId("agents-inbox-lane-chips")).toBeVisible();
  await expect(async () => {
    for (const lane of AGENT_INBOX_LANES) {
      const chip = page.getByTestId(`agents-inbox-lane-chip-${lane}`);
      await chip.click();
      await expect(chip).toHaveAttribute("aria-selected", "true");
      if (await row.isVisible()) {
        return;
      }
    }
    throw new Error(
      `Conversation ${conversationId} is not rendered in any inbox lane`,
    );
  }).toPass({ timeout: INBOX_CONVERSATION_TIMEOUT_MS });

  return row;
}
