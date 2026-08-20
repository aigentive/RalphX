import { test, expect } from "@playwright/test";
import { ProjectCreationWizardPage } from "../../../pages/modals/project-creation-wizard.page";
import { setupApp } from "../../../fixtures/setup.fixtures";
import { openProjectCreationWizard } from "../../../helpers/project-creation-wizard.helpers";

/**
 * Visual regression tests for the Clone intent of the ProjectCreationWizard.
 *
 * These run in web mode, where the Tauri commands are served by
 * `frontend/src/mocks/tauri-api-core.ts`. Clone progress in particular is driven
 * by the mocked `get_clone_job_status` phase advance, because web mode has no
 * way to deliver `project:clone_*` events; `clone.forceOutcome()` picks the
 * terminal frame that poll settles on.
 */

const CLONE_URL = "https://github.com/ralphx/example.git";

test.describe("ProjectCreationWizard - clone", () => {
  let wizardPage: ProjectCreationWizardPage;

  test.beforeEach(async ({ page }) => {
    wizardPage = new ProjectCreationWizardPage(page);
    await setupApp(page);
    await openProjectCreationWizard(page);
    await wizardPage.waitForIntentChooser();
    await wizardPage.chooseClone();
  });

  test("validates the URL before enabling the primary action", async () => {
    await expect(wizardPage.title).toHaveText("Clone Repository");
    await expect(wizardPage.clone.startButton).toBeDisabled();

    await wizardPage.clone.fillUrl(CLONE_URL);
    await wizardPage.browseParentButton.click();

    await expect(wizardPage.clone.startButton).toBeEnabled({ timeout: 5000 });
    // The folder name is derived from the URL until the user overrides it.
    await expect(wizardPage.clone.folderNameInput).toHaveValue("example");
  });

  test("exposes advanced clone options on demand", async () => {
    await expect(wizardPage.clone.depthInput).not.toBeVisible();

    await wizardPage.clone.openAdvancedOptions();

    await expect(wizardPage.clone.depthInput).toBeVisible();
    await expect(wizardPage.clone.singleBranchToggle).toBeVisible();
    await expect(wizardPage.clone.submodulesToggle).toBeVisible();
  });

  test("fills the URL from the GitHub repo picker", async () => {
    // The picker fetches on expand, not on step mount.
    await expect(wizardPage.clone.repoPickerTrigger).toBeVisible();
    await wizardPage.clone.expandRepoPicker();

    const firstRepo = wizardPage.clone.repoPickerItem.first();
    await expect(firstRepo).toBeVisible({ timeout: 5000 });
    const nameWithOwner = (await firstRepo.textContent())?.trim() ?? "";
    await firstRepo.click();

    await expect(wizardPage.clone.urlInput).toHaveValue(nameWithOwner);
  });

  test("shows progress and reaches the prefilled settings step", async () => {
    await wizardPage.clone.fillUrl(CLONE_URL);
    await wizardPage.browseParentButton.click();
    await expect(wizardPage.clone.startButton).toBeEnabled({ timeout: 5000 });

    await wizardPage.clone.start();
    await expect(wizardPage.clone.cancelButton).toBeVisible();

    // The status mock settles after its phase sequence, chaining into settings.
    await expect(wizardPage.clone.settingsDestination).toBeVisible({ timeout: 30000 });
    await expect(wizardPage.createButton).toBeVisible();
  });

  test("offers recovery after an auth failure", async () => {
    await wizardPage.clone.forceOutcome("auth_failed");

    await wizardPage.clone.fillUrl(CLONE_URL);
    await wizardPage.browseParentButton.click();
    await expect(wizardPage.clone.startButton).toBeEnabled({ timeout: 5000 });

    await wizardPage.clone.start();

    // A failed clone returns to configure with the auth recovery card attached.
    await expect(wizardPage.clone.authCard).toBeVisible({ timeout: 30000 });
    await expect(wizardPage.clone.retryButton).toBeVisible();
    await expect(wizardPage.clone.urlInput).toBeVisible();
  });

  // ── Snapshots ─────────────────────────────────────────────────────────────

  test("matches snapshot - clone", async () => {
    await wizardPage.waitForAnimations();

    await expect(wizardPage.modal).toHaveScreenshot("project-creation-wizard-clone.png");
  });

  test("matches snapshot - clone progress", async () => {
    await wizardPage.clone.fillUrl(CLONE_URL);
    await wizardPage.browseParentButton.click();
    await expect(wizardPage.clone.startButton).toBeEnabled({ timeout: 5000 });
    await wizardPage.clone.start();

    await wizardPage.waitForAnimations();

    await expect(wizardPage.modal).toHaveScreenshot("project-creation-wizard-clone-progress.png", {
      // The progress bar advances on every mocked status poll.
      maxDiffPixelRatio: 0.05,
    });
  });
});
