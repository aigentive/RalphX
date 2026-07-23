import { expect, test } from "@playwright/test";

import { setupApp } from "../../../fixtures/setup.fixtures";
import { ReleaseNotesPage } from "../../../pages/release-notes.page";

test.describe("Release Notes channels", () => {
  let releaseNotes: ReleaseNotesPage;

  test.beforeEach(async ({ page }) => {
    releaseNotes = new ReleaseNotesPage(page);
    await releaseNotes.mockReleaseMetadata();
    await setupApp(page);
    await releaseNotes.openFromNativeMenu();
  });

  test("shows Stable history and its current-channel status", async () => {
    await expect(releaseNotes.stableChannel).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(releaseNotes.stableChannel).toContainText("Current");
    await expect(releaseNotes.activeVersionHeading("0.76.0")).toBeVisible();
    await releaseNotes.waitForAnimations();

    await expect(releaseNotes.dialog).toHaveScreenshot(
      "release-notes-stable.png",
      { maxDiffPixelRatio: 0.01 },
    );
  });

  test("browses Nightly history without changing the current channel", async () => {
    await releaseNotes.browseNightly();

    await expect(releaseNotes.nightlyChannel).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(releaseNotes.stableChannel).toContainText("Current");
    await expect(releaseNotes.useNightly).toBeVisible();
    await expect(releaseNotes.activeVersionHeading("0.77.0")).toBeVisible();
    await releaseNotes.waitForAnimations();

    await expect(releaseNotes.dialog).toHaveScreenshot(
      "release-notes-nightly.png",
      { maxDiffPixelRatio: 0.01 },
    );
  });
});
