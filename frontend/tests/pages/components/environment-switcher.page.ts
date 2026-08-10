import { expect, type Locator, type Page } from "@playwright/test";

import { BasePage } from "../base.page";

export class EnvironmentSwitcherPage extends BasePage {
  readonly trigger: Locator;
  readonly listbox: Locator;

  constructor(page: Page) {
    super(page);
    this.trigger = page.getByRole("button", { name: "Switch environment" });
    this.listbox = page.getByRole("listbox", { name: "Environments" });
  }

  async seedTwoEnvironmentRegistry(): Promise<void> {
    await this.page.evaluate(async () => {
      const [{ useEnvironmentStore }, { useUiStore }] = await Promise.all([
        import("/src/stores/environmentStore"),
        import("/src/stores/uiStore"),
      ]);
      useUiStore.getState().setFeatureFlags({
        ...useUiStore.getState().featureFlags,
        remoteEnvironments: true,
      });
      useEnvironmentStore.getState().setEnvironments([
        {
          id: "studio-mac",
          environmentId: "studio-host",
          name: "Studio-Mac",
          baseUrl: "https://studio-mac.test",
          candidateUrls: [],
          scopes: ["ui:read", "ui:operate"],
          protocolVersion: 1,
          status: "active",
          createdAt: "2026-07-28T00:00:00Z",
          lastConnectedAt: null,
        },
      ]);
      useEnvironmentStore.getState().setConnectionState("studio-mac", "connected");
    });
  }

  async open(): Promise<void> {
    await this.trigger.click();
    await expect(this.listbox).toBeVisible();
  }

  option(name: string): Locator {
    return this.listbox.getByRole("option", { name: new RegExp(name) });
  }

  async switchTo(name: string): Promise<void> {
    await this.option(name).click();
    await expect(this.listbox).toBeHidden();
    await expect(this.trigger).toContainText(name);
  }
}
