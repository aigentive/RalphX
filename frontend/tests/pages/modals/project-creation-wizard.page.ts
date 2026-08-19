import { Page, Locator } from "@playwright/test";
import { BasePage } from "../base.page";
import { ProjectCreationWizardClonePage } from "./project-creation-wizard-clone.page";

/**
 * Page Object for ProjectCreationWizard modal.
 *
 * The wizard opens on an intent chooser (Clone / Create New / Add Existing)
 * rather than straight into a form, so every form interaction must first pick an
 * intent via `chooseClone()` / `chooseCreateNew()` / `chooseAddExisting()`.
 * Clone-specific locators live on `.clone`.
 */
export class ProjectCreationWizardPage extends BasePage {
  readonly modal: Locator;
  readonly title: Locator;

  // Intent chooser (landing step)
  readonly intentClone: Locator;
  readonly intentCreate: Locator;
  readonly intentExisting: Locator;
  readonly backButton: Locator;

  // Add Existing
  readonly projectNameInput: Locator;
  readonly workingDirectoryInput: Locator;
  readonly browseFolderButton: Locator;
  readonly candidateCard: Locator;
  readonly candidateRecoveryAction: Locator;
  readonly recentRepositoriesList: Locator;
  readonly recentRepositoryItem: Locator;

  // Create New destination
  readonly newProjectParentInput: Locator;
  readonly newProjectFolderNameInput: Locator;
  readonly newProjectDestinationPreview: Locator;
  readonly browseParentButton: Locator;

  // Worktree-first settings, shared by every registering intent
  readonly baseBranchSelect: Locator;
  readonly worktreePathInput: Locator;
  readonly advancedSettingsTrigger: Locator;
  readonly worktreeParentInput: Locator;
  readonly worktreeParentVerdict: Locator;

  readonly createButton: Locator;
  readonly cancelButton: Locator;
  readonly errorMessage: Locator;

  readonly clone: ProjectCreationWizardClonePage;

  constructor(page: Page) {
    super(page);

    const modal = page.locator('[data-testid="project-creation-wizard"]');
    this.modal = modal;
    // The header title changes per step, so assert its text rather than match on it.
    this.title = modal.getByRole("heading").first();

    this.intentClone = modal.locator('[data-testid="intent-clone"]');
    this.intentCreate = modal.locator('[data-testid="intent-create"]');
    this.intentExisting = modal.locator('[data-testid="intent-existing"]');
    this.backButton = modal.locator('[data-testid="wizard-back-button"]');

    this.projectNameInput = modal.locator('[data-testid="project-name-input"]');
    this.workingDirectoryInput = modal.locator('[data-testid="folder-input"]');
    this.browseFolderButton = modal.locator('[data-testid="browse-button"]');
    this.candidateCard = modal.locator('[data-testid="candidate-card"]');
    this.candidateRecoveryAction = modal.locator('[data-testid="candidate-recovery-action"]');
    this.recentRepositoriesList = modal.locator('[data-testid="recent-repositories-list"]');
    this.recentRepositoryItem = modal.locator('[data-testid="recent-repository-item"]');

    this.newProjectParentInput = modal.locator('[data-testid="new-project-parent-input"]');
    this.newProjectFolderNameInput = modal.locator(
      '[data-testid="new-project-folder-name-input"]'
    );
    this.newProjectDestinationPreview = modal.locator(
      '[data-testid="new-project-destination-preview"]'
    );
    this.browseParentButton = modal.locator('[data-testid="browse-parent-button"]');

    this.baseBranchSelect = modal.locator('[data-testid="base-branch-select"]');
    this.worktreePathInput = modal.locator('[data-testid="worktree-path-display"]');
    this.advancedSettingsTrigger = modal.locator('[data-testid="advanced-settings-trigger"]');
    this.worktreeParentInput = modal.locator('[data-testid="worktree-parent-input"]');
    this.worktreeParentVerdict = modal.locator('[data-testid="worktree-parent-verdict"]');

    this.createButton = modal.locator('[data-testid="create-button"]');
    this.cancelButton = modal.locator('[data-testid="cancel-button"]');
    this.errorMessage = modal.locator('[data-testid="wizard-error"]');

    this.clone = new ProjectCreationWizardClonePage(page, modal);
  }

  async waitForModal() {
    await this.modal.waitFor({ state: "visible", timeout: 5000 });
  }

  /** Wait for the intent chooser (the landing step) to be interactive. */
  async waitForIntentChooser() {
    await this.waitForModal();
    await this.intentClone.waitFor({ state: "visible", timeout: 5000 });
  }

  async chooseAddExisting() {
    await this.intentExisting.click();
    await this.workingDirectoryInput.waitFor({ state: "visible", timeout: 5000 });
  }

  async chooseCreateNew() {
    await this.intentCreate.click();
    await this.newProjectFolderNameInput.waitFor({ state: "visible", timeout: 5000 });
  }

  async chooseClone() {
    await this.intentClone.click();
    await this.clone.urlInput.waitFor({ state: "visible", timeout: 5000 });
  }

  /** Return to the intent chooser. */
  async goBack() {
    await this.backButton.click();
    await this.intentClone.waitFor({ state: "visible", timeout: 5000 });
  }

  async fillProjectName(name: string) {
    await this.projectNameInput.fill(name);
  }

  async clickBrowseFolder() {
    await this.browseFolderButton.click();
  }

  async openAdvancedSettings() {
    await this.advancedSettingsTrigger.click();
  }

  /** Pick a base branch from the portal-rendered select. */
  async selectBaseBranch(branch: string) {
    await this.baseBranchSelect.click();
    const option = this.page.locator('[role="option"]').filter({ hasText: branch });
    await option.waitFor({ state: "visible", timeout: 5000 });
    await option.click();
  }

  async getWorkingDirectory(): Promise<string> {
    return this.workingDirectoryInput.inputValue();
  }
}
