import { Locator, Page } from "@playwright/test";

type MockCloneOutcome = "success" | "auth_failed" | "unknown";

/**
 * Clone-intent surface of the ProjectCreationWizard: URL entry, the optional
 * GitHub repo picker, advanced clone options, live progress, and the
 * auth-failure recovery card.
 *
 * Composed onto ProjectCreationWizardPage as `.clone` so the wizard page object
 * stays within the page-object size limit.
 */
export class ProjectCreationWizardClonePage {
  // Configure phase
  readonly urlInput: Locator;
  readonly parentInput: Locator;
  readonly folderNameInput: Locator;
  readonly destinationPreview: Locator;
  readonly startButton: Locator;

  // GitHub repo picker (only rendered when gh auth reports authenticated)
  readonly repoPickerTrigger: Locator;
  readonly repoPicker: Locator;
  readonly repoPickerItem: Locator;

  // Advanced clone options
  readonly advancedTrigger: Locator;
  readonly depthInput: Locator;
  readonly singleBranchToggle: Locator;
  readonly submodulesToggle: Locator;

  // Running phase
  readonly progress: Locator;
  readonly cancelButton: Locator;
  readonly consoleTrigger: Locator;
  readonly consoleOutput: Locator;

  // Terminal frames
  readonly authCard: Locator;
  readonly authLoginButton: Locator;
  readonly useSshButton: Locator;
  readonly retryButton: Locator;
  readonly settingsDestination: Locator;

  constructor(
    private readonly page: Page,
    modal: Locator
  ) {
    this.urlInput = modal.locator('[data-testid="clone-url-input"]');
    this.parentInput = modal.locator('[data-testid="clone-parent-input"]');
    this.folderNameInput = modal.locator('[data-testid="clone-folder-name-input"]');
    this.destinationPreview = modal.locator('[data-testid="clone-destination-preview"]');
    this.startButton = modal.locator('[data-testid="clone-start-button"]');

    this.repoPickerTrigger = modal.locator('[data-testid="github-repo-picker-trigger"]');
    this.repoPicker = modal.locator('[data-testid="github-repo-picker"]');
    this.repoPickerItem = modal.locator('[data-testid="github-repo-picker-item"]');

    this.advancedTrigger = modal.locator('[data-testid="clone-advanced-options-trigger"]');
    this.depthInput = modal.locator('[data-testid="clone-depth-input"]');
    this.singleBranchToggle = modal.locator('[data-testid="clone-single-branch-toggle"]');
    this.submodulesToggle = modal.locator('[data-testid="clone-submodules-toggle"]');

    this.progress = modal.locator('[data-testid="clone-progress"]');
    this.cancelButton = modal.locator('[data-testid="clone-cancel-button"]');
    this.consoleTrigger = modal.locator('[data-testid="clone-console-trigger"]');
    this.consoleOutput = modal.locator('[data-testid="clone-console-output"]');

    this.authCard = modal.locator('[data-testid="clone-auth-card"]');
    this.authLoginButton = modal.locator('[data-testid="clone-auth-login-button"]');
    this.useSshButton = modal.locator('[data-testid="clone-use-ssh-button"]');
    this.retryButton = modal.locator('[data-testid="clone-retry-button"]');
    this.settingsDestination = modal.locator('[data-testid="clone-settings-destination"]');
  }

  /** Enter a clone URL and wait out VALIDATE_DEBOUNCE_MS plus the mocked round trip. */
  async fillUrl(url: string) {
    await this.urlInput.fill(url);
    await this.page.waitForTimeout(500);
  }

  async openAdvancedOptions() {
    await this.advancedTrigger.click();
    await this.depthInput.waitFor({ state: "visible", timeout: 5000 });
  }

  async expandRepoPicker() {
    await this.repoPickerTrigger.click();
    await this.repoPicker.waitFor({ state: "visible", timeout: 5000 });
  }

  async start() {
    await this.startButton.click();
    await this.progress.waitFor({ state: "visible", timeout: 5000 });
  }

  /**
   * Web mode has no `project:clone_*` events, so the mocked `get_clone_job_status`
   * poll decides the terminal frame. Set this before starting a clone.
   */
  async forceOutcome(outcome: MockCloneOutcome) {
    await this.page.evaluate((value) => {
      (window as Window & { __mockCloneJobOutcome?: string }).__mockCloneJobOutcome = value;
    }, outcome);
  }
}
