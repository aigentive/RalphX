import { expect, test, type Page } from "@playwright/test";

import {
  captureFocusedGuideScreenshot,
  captureGuideScreenshot,
  setupCapture,
} from "./fixtures/capture.fixtures";
import { applyGuideScenario } from "./fixtures/guide-scenario.fixtures";
import {
  openArtifactTab,
  openGuideConversation,
} from "./fixtures/guide-navigation.fixtures";

/**
 * Atlassian settings resolve during first paint, so the connected fixture has to
 * be installed before `setupCapture` navigates. Seeding it afterwards races the
 * already-resolved settings query and freezes the Jira tab out of the pane.
 */
async function useConnectedAtlassian(page: Page): Promise<void> {
  await page.addInitScript(() => {
    window.__mockGuideAtlassianConnected = true;
  });
}

test("composer-jira-reference", async ({ page }) => {
  await useConnectedAtlassian(page);
  await setupCapture(page);
  await applyGuideScenario(page, "guide_tour");
  await openGuideConversation(page, "guide_tour");

  const composer = page.locator("textarea.agent-composer-textarea").last();
  await composer.click();
  // The guide's point is that you search instead of knowing the key, so the
  // capture types a plain word rather than an issue key.
  await composer.pressSequentially("@jira:release", { delay: 20 });

  await expect(page.getByText("REL-214", { exact: false }).first()).toBeVisible();
  // Park the pointer off the composer: leaving it over the textarea keeps the
  // send control in its hover-expanded state, which clips against the composer
  // edge and reads as a rendering bug in the published image.
  await page.mouse.move(0, 0);
  await captureFocusedGuideScreenshot(page, "composer-jira-reference.png");
});

test("jira-issue-tab", async ({ page }) => {
  await useConnectedAtlassian(page);
  await setupCapture(page);
  await applyGuideScenario(page, "guide_tour");
  await openGuideConversation(page, "guide_tour");
  await openArtifactTab(page, "jira");
  // The guide tells the reader to plan against the ticket's own wording, so the
  // capture must show resolved issue content, not the "Loading Jira..." shell.
  await expect(page.getByText("Acceptance Criteria")).toBeVisible();
  await expect(page.getByText("Attachments (1)")).toBeVisible();
  await captureGuideScreenshot(page, "jira-issue-tab.png");
});
