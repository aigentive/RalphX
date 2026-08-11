import type { Locator, Page } from "@playwright/test";

import { BasePage } from "./base.page";

export class StartupPage extends BasePage {
  readonly screen: Locator;
  readonly status: Locator;
  readonly retryButton: Locator;

  constructor(page: Page) {
    super(page);
    this.screen = page.getByTestId("startup-screen");
    this.status = page.getByRole("status");
    this.retryButton = page.getByRole("button", { name: "Retry startup" });
  }

  async open(
    scenario:
      | "long-running"
      | "app-state-ready"
      | "background-restoring"
      | "failed",
    theme: "light" | "dark",
  ) {
    await this.page.goto(`/?test=startup&scenario=${scenario}&theme=${theme}`);
    await this.page.locator("html").waitFor({ state: "attached" });
    if (scenario !== "background-restoring") {
      await this.screen.waitFor({ state: "visible" });
    }
  }
}
