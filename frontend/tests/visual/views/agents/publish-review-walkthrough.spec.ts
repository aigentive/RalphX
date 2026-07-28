import { expect, test } from "@playwright/test";

import { dismissProviderCliUpdateToasts } from "../../../fixtures/setup.fixtures";
import { AgentsPublishPage } from "../../../pages/views/agents-publish.page";

test.describe("Agents publish review walkthrough", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.addInitScript(() => window.localStorage.clear());
    await dismissProviderCliUpdateToasts(page);
  });

  test("enters from Changes and supports button and guarded keyboard navigation", async ({
    page,
  }) => {
    const publish = new AgentsPublishPage(page);
    await publish.openReviewWalkthroughScenario();

    await publish.enterReviewWalkthrough();
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 1 of 3");
    await expect(publish.reviewWalkthroughCard).toContainText("Avoid stale publish state");
    await expect(publish.reviewWalkthroughHunk).toContainText("reviewWalkthrough");
    await expect(publish.reviewWalkthroughPrevious).toBeDisabled();
    await expect(publish.reviewWalkthroughCard).toHaveScreenshot(
      "publish-review-walkthrough-card.png",
      { maxDiffPixelRatio: 0.01 },
    );

    await publish.reviewWalkthroughNext.click();
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 2 of 3");
    await publish.reviewWalkthroughPrevious.click();
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 1 of 3");

    await publish.pressWalkthroughKey("J");
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 2 of 3");
    await publish.pressWalkthroughKey("K");
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 1 of 3");
    await publish.focusWalkthroughInput();
    await publish.pressWalkthroughKey("J");
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 1 of 3");
  });

  test("review progress, dot jumps, and exit preserve the full non-gating diff view", async ({
    page,
  }) => {
    const publish = new AgentsPublishPage(page);
    await publish.openReviewWalkthroughScenario();
    await expect(publish.publishConfirm).toBeVisible();
    const publishDisabledBeforeWalkthrough = await publish.publishConfirm.isDisabled();

    await publish.enterReviewWalkthrough();
    await publish.reviewWalkthroughMark.click();
    await expect(publish.reviewWalkthroughProgress).toHaveText("1 of 3 reviewed");
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 2 of 3");
    await publish.reviewWalkthroughDot(2).click();
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 3 of 3");

    await publish.reviewWalkthroughExit.click();
    await expect(publish.reviewWalkthrough).toHaveCount(0);
    await expect(publish.reviewWalkthroughUnreviewedFile).toBeVisible();
    await expect(publish.reviewWalkthroughUnreviewedCode).toBeVisible();
    await expect(publish.publishConfirm).toHaveJSProperty(
      "disabled",
      publishDisabledBeforeWalkthrough,
    );
  });

  test("shows completion after the final finding and restarts from the first", async ({
    page,
  }) => {
    const publish = new AgentsPublishPage(page);
    await publish.openReviewWalkthroughScenario();
    await publish.enterReviewWalkthrough();

    await publish.reviewWalkthroughNext.click();
    await publish.reviewWalkthroughNext.click();
    await publish.reviewWalkthroughNext.click();
    await expect(publish.reviewWalkthroughDone).toContainText("0 of 3 reviewed");
    await expect(publish.reviewWalkthroughDone).toHaveScreenshot(
      "publish-review-walkthrough-complete.png",
      { maxDiffPixelRatio: 0.01 },
    );
    await publish.reviewWalkthroughRestart.click();
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 1 of 3");
  });

  test("keeps reviewed state ephemeral across a reload", async ({ page }) => {
    const publish = new AgentsPublishPage(page);
    await publish.openReviewWalkthroughScenario();
    await publish.enterReviewWalkthrough();
    await publish.reviewWalkthroughMark.click();
    await expect(publish.reviewWalkthroughProgress).toHaveText("1 of 3 reviewed");

    await publish.reloadReviewWalkthroughScenario();
    await publish.enterReviewWalkthrough();
    await expect(publish.reviewWalkthroughProgress).toHaveText("0 of 3 reviewed");
    await expect(publish.reviewWalkthroughPosition).toHaveText("Finding 1 of 3");
  });
});
