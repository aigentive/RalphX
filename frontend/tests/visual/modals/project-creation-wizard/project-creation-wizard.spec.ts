import { test, expect } from "@playwright/test";
import { ProjectCreationWizardPage } from "../../../pages/modals/project-creation-wizard.page";
import { setupApp } from "../../../fixtures/setup.fixtures";
import { openProjectCreationWizard } from "../../../helpers/project-creation-wizard.helpers";

/**
 * Visual regression tests for the ProjectCreationWizard intent chooser and the
 * two non-clone intents. Clone lives in project-creation-wizard-clone.spec.ts.
 *
 * The wizard opens on an explicit intent chooser (Clone / Create New / Add
 * Existing); each intent then shows its own step. The worktree-first settings
 * (base branch, derived worktree path, advanced parent override) are shared by
 * the intents that register a project.
 *
 * These run in web mode, where the Tauri commands are served by
 * `frontend/src/mocks/tauri-api-core.ts`.
 */

test.describe("ProjectCreationWizard", () => {
  let wizardPage: ProjectCreationWizardPage;

  test.beforeEach(async ({ page }) => {
    wizardPage = new ProjectCreationWizardPage(page);
    await setupApp(page);
    await openProjectCreationWizard(page);
    await wizardPage.waitForIntentChooser();
  });

  test("opens on the intent chooser", async () => {
    await expect(wizardPage.modal).toBeVisible();
    await expect(wizardPage.title).toHaveText("Create New Project");

    // All three intents are offered up front.
    await expect(wizardPage.intentClone).toBeVisible();
    await expect(wizardPage.intentCreate).toBeVisible();
    await expect(wizardPage.intentExisting).toBeVisible();

    // No form is shown until an intent is chosen.
    await expect(wizardPage.workingDirectoryInput).not.toBeVisible();
    await expect(wizardPage.clone.urlInput).not.toBeVisible();
  });

  test("returns to the chooser from a step", async () => {
    await wizardPage.chooseAddExisting();
    await expect(wizardPage.title).toHaveText("Add Existing Repository");

    await wizardPage.goBack();
    await expect(wizardPage.intentClone).toBeVisible();
    await expect(wizardPage.workingDirectoryInput).not.toBeVisible();
  });

  test("renders Add Existing with default worktree state", async () => {
    await wizardPage.chooseAddExisting();

    await expect(wizardPage.projectNameInput).toBeVisible();
    await expect(wizardPage.workingDirectoryInput).toBeVisible();
    await expect(wizardPage.browseFolderButton).toBeVisible();

    // Worktree configuration is the default contract now
    await expect(wizardPage.baseBranchSelect).toBeVisible();
    await expect(wizardPage.worktreePathInput).toBeVisible();
    await expect(wizardPage.advancedSettingsTrigger).toBeVisible();
    await expect(wizardPage.worktreeParentInput).not.toBeVisible();

    await expect(wizardPage.createButton).toBeVisible();
    await expect(wizardPage.cancelButton).toBeVisible();
  });

  test("fills project name and selects folder", async ({ page }) => {
    await wizardPage.chooseAddExisting();

    await wizardPage.fillProjectName("My Test Project");
    await expect(wizardPage.projectNameInput).toHaveValue("My Test Project");

    // Click browse folder button (mock will auto-fill)
    await wizardPage.clickBrowseFolder();
    await page.waitForTimeout(200);

    const workingDir = await wizardPage.getWorkingDirectory();
    expect(workingDir).toContain("/Users/test/projects/test-project");
  });

  test("probes the chosen folder and shows a candidate card", async () => {
    await wizardPage.chooseAddExisting();
    await wizardPage.clickBrowseFolder();

    // The probe is debounced and deferred past a paint boundary.
    await expect(wizardPage.candidateCard).toBeVisible({ timeout: 5000 });
  });

  test("reveals advanced worktree settings on demand", async () => {
    await wizardPage.chooseAddExisting();
    await expect(wizardPage.worktreeParentInput).not.toBeVisible();

    await wizardPage.openAdvancedSettings();

    await expect(wizardPage.worktreeParentInput).toBeVisible();
  });

  test("Create New asks for a parent folder and a name", async () => {
    await wizardPage.chooseCreateNew();

    await expect(wizardPage.newProjectParentInput).toBeVisible();
    await expect(wizardPage.newProjectFolderNameInput).toBeVisible();

    await wizardPage.browseParentButton.click();
    await wizardPage.newProjectFolderNameInput.fill("my-app");

    // The live preview proves the two inputs compose into one destination.
    await expect(wizardPage.newProjectDestinationPreview).toBeVisible();
    await expect(wizardPage.newProjectDestinationPreview).toContainText("my-app");
  });

  // ── Snapshots ─────────────────────────────────────────────────────────────
  // `-default` now captures the intent chooser rather than the old single form;
  // `-advanced` and `-filled` keep their meaning but route through Add Existing.

  test("matches snapshot - default", async () => {
    await wizardPage.waitForAnimations();

    await expect(wizardPage.modal).toHaveScreenshot("project-creation-wizard-default.png");
  });

  test("matches snapshot - add existing", async () => {
    await wizardPage.chooseAddExisting();
    await wizardPage.waitForAnimations();

    await expect(wizardPage.modal).toHaveScreenshot("project-creation-wizard-add-existing.png");
  });

  test("matches snapshot - create new", async () => {
    await wizardPage.chooseCreateNew();
    await wizardPage.waitForAnimations();

    await expect(wizardPage.modal).toHaveScreenshot("project-creation-wizard-create-new.png");
  });

  test("matches snapshot - advanced settings open", async () => {
    await wizardPage.chooseAddExisting();
    await wizardPage.openAdvancedSettings();
    await expect(wizardPage.worktreeParentInput).toBeVisible();
    await wizardPage.waitForAnimations();

    await expect(wizardPage.modal).toHaveScreenshot("project-creation-wizard-advanced.png");
  });

  test("matches snapshot - with filled form", async ({ page }) => {
    await wizardPage.chooseAddExisting();

    await wizardPage.fillProjectName("My Test Project");
    await wizardPage.clickBrowseFolder();
    await page.waitForTimeout(200);

    // Worktree settings are visible by default; advanced settings carry the custom parent path
    await wizardPage.selectBaseBranch("main");
    await wizardPage.openAdvancedSettings();
    await wizardPage.worktreeParentInput.fill("/Users/test/projects/.ralphx");

    await wizardPage.waitForAnimations();

    await expect(wizardPage.modal).toHaveScreenshot("project-creation-wizard-filled.png");
  });
});
