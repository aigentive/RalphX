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
    // Emits through the app's event bus, not `__mockTauriEmit`: the native-menu handler
    // in `UpdateChecker.events.ts` subscribes via `useEventBus()`, so in web mode it
    // registers on MockEventBus and never sees the mock Tauri `listen` registry.
    // `window.__eventBus` is the seam EventProvider publishes for exactly this
    // (see tests/helpers/permission.helpers.ts).
    await this.page.evaluate(() => {
      const eventBus = (
        window as Window & {
          __eventBus?: { emit: (event: string, payload?: unknown) => void };
        }
      ).__eventBus;
      if (!eventBus) {
        throw new Error("EventBus not available. Make sure app is running in web mode.");
      }
      eventBus.emit("ralphx://show-release-notes", undefined);
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
