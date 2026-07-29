import { expect, test } from "@playwright/test";

import { captureGuideScreenshot, setupCapture } from "./fixtures/capture.fixtures";
import {
  applyGuideScenario,
  hydrateGuideLocalReviewArtifactCache,
} from "./fixtures/guide-scenario.fixtures";
import { openGuideConversation, openPublish } from "./fixtures/guide-navigation.fixtures";
import { SettingsPage } from "../pages/settings.page";

test("review-run", async ({ page }) => {
  await setupCapture(page); await applyGuideScenario(page, "guide_local_review");
  await openGuideConversation(page, "guide_local_review");
  await openPublish(page);
  await page.getByTestId("agents-publish-tab-review").click();
  await hydrateGuideLocalReviewArtifactCache(page);
  await page.getByRole("tab", { name: "Overview" }).click();
  await expect(
    page.getByText("The release is close to ready.", { exact: false }),
  ).toBeVisible();
  await captureGuideScreenshot(page, "review-run.png");
});

test("review-requested-changes", async ({ page }) => {
  await setupCapture(page); await applyGuideScenario(page, "guide_local_review");
  await openGuideConversation(page, "guide_local_review"); await openPublish(page);
  await page.getByTestId("agents-publish-tab-review").click();
  await hydrateGuideLocalReviewArtifactCache(page);
  await page.getByRole("tab", { name: "Requested Changes" }).click();
  await expect(page.getByText("Add the final rollback owner to the checklist.")).toBeVisible();
  await captureGuideScreenshot(page, "review-requested-changes.png");
});

test("pr-review-monitor", async ({ page }) => {
  await setupCapture(page); await applyGuideScenario(page, "guide_pr_review");
  await openGuideConversation(page, "guide_pr_review");
  await expect(page.getByTestId("agent-workspace-pr-review-card")).toBeVisible();
  // The transcript paints skeleton placeholders before it hydrates. Waiting on
  // the card alone froze those grey bars into the published screenshot.
  await expect(
    page.getByText("The release path is mapped", { exact: false }),
  ).toBeVisible();
  await captureGuideScreenshot(page, "pr-review-monitor.png");
});

test("settings-github", async ({ page }) => {
  await setupCapture(page); await applyGuideScenario(page, "guide_github_settings");
  const settings = new SettingsPage(page);
  await settings.openViaStore("github"); await settings.waitForSection("github", "GitHub");
  await captureGuideScreenshot(page, "settings-github.png");
});
