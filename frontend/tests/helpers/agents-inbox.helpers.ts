import { expect, type Locator, type Page } from "@playwright/test";

// Top-level inbox filters. Recent covers the former needs and working lanes,
// which now render as two groups inside Recent's single scroller.
const AGENT_INBOX_FILTERS = ["recent", "stale", "done"] as const;
const INBOX_CONVERSATION_TIMEOUT_MS = 15_000;
// Selecting a filter mounts its list; the rows need a paint before they are
// visible, so each candidate gets its own bounded wait rather than an instant
// check that can never observe the row it just asked for.
const INBOX_FILTER_ROW_TIMEOUT_MS = 3_000;

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
    for (const filter of AGENT_INBOX_FILTERS) {
      const chip = page.getByTestId(`agents-inbox-lane-chip-${filter}`);
      await chip.click();
      await expect(chip).toHaveAttribute("aria-selected", "true");
      const revealed = await row
        .waitFor({ state: "visible", timeout: INBOX_FILTER_ROW_TIMEOUT_MS })
        .then(() => true)
        .catch(() => false);
      if (revealed) {
        return;
      }
    }
    throw new Error(
      `Conversation ${conversationId} is not rendered in any inbox filter`,
    );
  }).toPass({ timeout: INBOX_CONVERSATION_TIMEOUT_MS });

  return row;
}
