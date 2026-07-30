import { expect, test } from "@playwright/test";

import {
  captureGuideScreenshot,
  setupCapture,
} from "./fixtures/capture.fixtures";
import {
  applyGuideScenario,
  hydrateGuidePlanningArtifactCache,
} from "./fixtures/guide-scenario.fixtures";
import {
  openArtifacts,
  openArtifactTab,
  openGuideConversation,
  openPublish,
} from "./fixtures/guide-navigation.fixtures";

test("plan-bundle", async ({ page }) => {
  await setupCapture(page);
  await applyGuideScenario(page, "guide_planning");
  await openGuideConversation(page, "guide_planning");
  await openArtifacts(page);
  await hydrateGuidePlanningArtifactCache(page, "conversation-guide_planning");
  await openArtifactTab(page, "plan");
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Blueprint" })).toBeVisible();
  // The guide tells readers to read the outcome, scope, and acceptance
  // criteria here, so the capture must show a plan that actually has them.
  await expect(page.getByText("Acceptance criteria")).toBeVisible();
  await captureGuideScreenshot(page, "plan-bundle.png");
});

test("agent-workspace-implementing", async ({ page }) => {
  await setupCapture(page);
  await applyGuideScenario(page, "guide_implementing");
  await openGuideConversation(page, "guide_implementing");
  await expect(page.getByTestId("integrated-chat-panel")).toBeVisible();
  await expect(
    page.getByText("I’m implementing the release checklist", { exact: false }),
  ).toBeVisible();
  await captureGuideScreenshot(page, "agent-workspace-implementing.png");
});

test("commit-publish-tab", async ({ page }) => {
  await setupCapture(page);
  await applyGuideScenario(page, "guide_implementing");
  await openGuideConversation(page, "guide_implementing");
  await openPublish(page);
  await expect(page.getByTestId("agents-artifact-tab-publish")).toBeVisible();
  // The guides document the connected-GitHub happy path, so the publish action
  // must be offered rather than the local-only commit fallback.
  await expect(page.getByTestId("agents-publish-confirm")).toBeVisible();
  // Each file row mounts its own paged-diff query, and the first attempt for a
  // row can error before React Query's retry succeeds. Settle the network first
  // so every row has resolved, otherwise the capture freezes a transient
  // "Could not load diff rows." state that the assertions alone would miss.
  await expect(page.getByTestId("paged-diff-view").first()).toBeVisible();
  await page.waitForLoadState("networkidle");
  await expect(page.getByTestId("paged-diff-error")).toHaveCount(0);
  await expect(page.getByTestId("paged-diff-loading")).toHaveCount(0);
  await expect(page.getByText("@@ -18,6 +18,9 @@").first()).toBeVisible();
  await captureGuideScreenshot(page, "commit-publish-tab.png");
});
