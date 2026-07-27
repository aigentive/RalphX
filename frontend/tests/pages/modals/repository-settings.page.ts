import { expect, type Locator, type Page } from "@playwright/test";

import { BasePage } from "../base.page";

export class RepositorySettingsPage extends BasePage {
  readonly githubPrModeToggle: Locator;
  readonly localWorkflowsAvailable: Locator;

  constructor(page: Page) {
    super(page);
    this.githubPrModeToggle = page.getByTestId("github-pr-enabled");
    this.localWorkflowsAvailable = page.getByText("Local workflows available", {
      exact: true,
    });
  }

  async expectLocalOnlyState() {
    await expect(this.localWorkflowsAvailable).toBeVisible();
    await expect(this.githubPrModeToggle).toBeDisabled();
  }
}
