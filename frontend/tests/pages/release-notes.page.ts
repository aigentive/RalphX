import type { Locator, Page } from "@playwright/test";

import { BasePage } from "./base.page";

export class ReleaseNotesPage extends BasePage {
  readonly dialog: Locator;
  readonly stableChannel: Locator;
  readonly nightlyChannel: Locator;
  readonly useNightly: Locator;

  constructor(page: Page) {
    super(page);
    this.dialog = page.getByRole("dialog", { name: "Release Notes" });
    this.stableChannel = this.dialog.getByRole("tab", { name: /Stable/ });
    this.nightlyChannel = this.dialog.getByRole("tab", { name: /Nightly/ });
    this.useNightly = this.dialog.getByTestId("release-notes-use-channel-button");
  }

  async mockReleaseMetadata() {
    await this.page.route(
      "https://api.github.com/repos/aigentive/ralphx.app/releases*",
      (route) =>
        route.fulfill({
          contentType: "application/json",
          body: JSON.stringify([
            {
              tag_name: "v0.77.0",
              published_at: "2026-07-23T08:00:00Z",
              name: "RalphX 0.77.0",
              body: "## Nightly 0.77.0\n\n- Faster release delivery\n- Channel-aware history",
              draft: false,
              prerelease: true,
            },
            {
              tag_name: "v0.76.1",
              published_at: "2026-07-22T08:00:00Z",
              name: "RalphX 0.76.1",
              body: "## Nightly 0.76.1\n\n- Reliability improvements",
              draft: false,
              prerelease: true,
            },
            {
              tag_name: "v0.76.0",
              published_at: "2026-07-21T08:00:00Z",
              name: "RalphX 0.76.0",
              body: "## Stable 0.76.0\n\n- Stable channel foundation",
              draft: false,
              prerelease: false,
            },
          ]),
        }),
    );
  }

  async openFromNativeMenu() {
    await this.page.evaluate(async () => {
      const emit = (
        window as Window & {
          __mockTauriEmit?: (event: string, payload?: unknown) => Promise<void>;
        }
      ).__mockTauriEmit;
      if (!emit) throw new Error("Mock Tauri event emitter is unavailable");
      await emit("ralphx://show-release-notes");
    });
    await this.dialog.waitFor({ state: "visible" });
  }

  async browseNightly() {
    await this.nightlyChannel.click();
    await this.activeVersionHeading("0.77.0").waitFor();
  }

  activeVersionHeading(version: string) {
    return this.dialog.getByRole("heading", { name: `v${version}` });
  }
}
