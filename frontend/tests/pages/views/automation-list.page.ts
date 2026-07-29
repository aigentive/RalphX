import { expect, type Locator, type Page } from "@playwright/test";

import { BasePage } from "../base.page";

export class AutomationListPage extends BasePage {
  readonly navAutomations: Locator;
  readonly view: Locator;
  readonly list: Locator;
  readonly toolbar: Locator;

  constructor(page: Page) {
    super(page);
    this.navAutomations = page.getByTestId("nav-automations");
    this.view = page.getByTestId("automations-view");
    this.list = page.getByTestId("automations-list");
    this.toolbar = page.getByTestId("automations-list-toolbar");
  }

  group(id: "attention" | "running" | "finished" | "drafts"): Locator {
    return this.page.getByTestId(`automations-group-${id}`);
  }

  async open(): Promise<void> {
    await this.navAutomations.click();
    await expect(this.page.getByTestId("automations-view-shell")).toBeVisible();
  }
}
