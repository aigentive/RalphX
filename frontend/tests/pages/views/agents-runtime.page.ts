import { expect, type Locator, type Page } from "@playwright/test";

import { BasePage } from "../base.page";

export class AgentsRuntimePage extends BasePage {
  readonly runtimeToggle: Locator;
  readonly mainGroup: Locator;
  readonly runsGroup: Locator;
  readonly standaloneRunsWidget: Locator;

  constructor(page: Page) {
    super(page);
    this.runtimeToggle = page.getByTestId("agents-composer-runtimes-toggle");
    this.mainGroup = page.getByTestId("agents-composer-runtimes-group-main");
    this.runsGroup = page.getByTestId("agents-composer-runtimes-group-runs");
    this.standaloneRunsWidget = page.getByTestId("agents-automation-runs-widget");
  }

  runRow(runId: string): Locator {
    return this.runsGroup.getByTestId(`agents-composer-automation-run-${runId}`);
  }

  async openRuntimeRuns(): Promise<void> {
    await expect(this.runtimeToggle).toBeVisible();
    await this.runtimeToggle.click();
    await expect(this.runsGroup).toBeVisible();
  }
}
