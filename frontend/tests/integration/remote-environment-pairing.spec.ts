/**
 * Client pairing journey (PR 2.5) — the CLIENT half, distinct from
 * `remote-access-pairing.spec.ts`, which drives the HOST pane.
 *
 * Runs against the web-mode client registry mock in `src/mocks/tauri-api-core.ts`
 * (`mockRemoteEnvironments`), which upserts on `environmentId` exactly like the Rust
 * registry, so "re-pair updates, never duplicates" is observable end to end.
 */

import { expect, test } from "@playwright/test";

import { setupSettings } from "../fixtures/setup.fixtures";

const PAIRING_URL =
  "ralphx://pair?host=https%3A%2F%2Fmock-host.tailnet.ts.net%3A3849#code=rxp_ABCD1234EFGH";

test.describe("Remote environment pairing journey (client)", () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      (
        window as Window & {
          __mockUiFeatureFlags?: Record<string, boolean>;
        }
      ).__mockUiFeatureFlags = {
        activityPage: true,
        extensibilityPage: true,
        automationsPage: true,
        atlassianOauth: false,
        ticketingDashboard: false,
        remoteEnvironments: true,
      };
    });
    await setupSettings(page);
    await page.click('[data-testid="settings-section-connections"]');
    await page.waitForSelector('[data-testid="connections-section"]');
  });

  test("pastes a pairing link, previews the host, pairs, and lands in the list", async ({
    page,
  }) => {
    // Empty to begin with — nothing paired on this Mac.
    await expect(page.getByTestId("connections-empty")).toBeVisible();

    await page.getByTestId("connections-add").click();
    await expect(page.getByTestId("add-environment-dialog")).toBeVisible();

    // Pasting the link fills host AND code; the code renders in 4-char groups (R-12).
    await page.getByTestId("add-environment-host").fill(PAIRING_URL);
    await expect(page.getByTestId("add-environment-host")).toHaveValue(
      "https://mock-host.tailnet.ts.net:3849",
    );
    await expect(page.getByTestId("add-environment-code")).toHaveValue(
      "rxp_ ABCD 1234 EFGH",
    );

    await page.getByTestId("add-environment-continue").click();

    // Verify step shows descriptor truth — and no project count, which the wire
    // descriptor does not carry.
    await expect(page.getByTestId("add-environment-step-verify")).toBeVisible();
    await expect(page.getByTestId("add-environment-protocol")).toContainText(
      "v1",
    );
    await expect(page.getByText("0.9.4")).toBeVisible();

    await page.getByTestId("add-environment-name").fill("Studio Mac");
    await page.getByTestId("add-environment-pair").click();

    await expect(page.getByTestId("add-environment-success")).toBeVisible();
    await expect(
      page.getByTestId("add-environment-success-banner"),
    ).toContainText("Studio Mac");
    await page.getByTestId("add-environment-done").click();

    // It is in the Connections list…
    const row = page.locator('[data-testid^="connections-row-"]');
    await expect(row).toHaveCount(1);
    await expect(row).toContainText("Studio Mac");

    // …and in the environment switcher, which is the acceptance that matters.
    await page.keyboard.press("Escape");
    await page.getByTestId("environment-switcher-trigger").click();
    await expect(page.getByText("Studio Mac")).toBeVisible();
  });

  test("re-pairing the same host updates the row instead of adding a second", async ({
    page,
  }) => {
    await page.getByTestId("connections-add").click();
    await page.getByTestId("add-environment-host").fill(PAIRING_URL);
    await page.getByTestId("add-environment-continue").click();
    await page.getByTestId("add-environment-name").fill("Studio Mac");
    await page.getByTestId("add-environment-pair").click();
    await page.getByTestId("add-environment-done").click();

    const row = page.locator('[data-testid^="connections-row-"]');
    await expect(row).toHaveCount(1);

    // Re-pair from the row itself: host locked, name prefilled from the existing row.
    await row.getByRole("button", { name: "Re-pair" }).click();
    await expect(page.getByTestId("add-environment-host")).toBeDisabled();
    await page.getByTestId("add-environment-code").fill("rxp_WXYZ9876");
    await page.getByTestId("add-environment-continue").click();

    await expect(
      page.getByTestId("add-environment-already-paired"),
    ).toContainText("updates it");
    await page.getByTestId("add-environment-name").fill("Studio Mac Renamed");
    await page.getByTestId("add-environment-pair").click();
    await page.getByTestId("add-environment-done").click();

    // One host identity, one row — the upsert is visible to the user.
    await expect(row).toHaveCount(1);
    await expect(row).toContainText("Studio Mac Renamed");
  });

  test("a version contradiction parks in a blocked state with no retry", async ({
    page,
  }) => {
    await page.evaluate(() => {
      (
        window as Window & { __mockRemoteEnvironmentSkew?: boolean }
      ).__mockRemoteEnvironmentSkew = true;
    });

    await page.getByTestId("connections-add").click();
    await page.getByTestId("add-environment-host").fill(PAIRING_URL);
    await page.getByTestId("add-environment-continue").click();

    const banner = page.getByTestId("add-environment-blocked-banner");
    await expect(banner).toBeVisible();
    await expect(banner).toContainText("Versions are incompatible");
    await expect(banner).toContainText("client protocol >= 2");

    // It stays blocked: nothing here schedules a retry (A-5).
    await page.waitForTimeout(1500);
    await expect(banner).toBeVisible();
    await expect(page.getByTestId("add-environment-step-verify")).toHaveCount(
      0,
    );

    // Back returns to step 1 so the user can fix it themselves.
    await page.getByTestId("add-environment-blocked-back").click();
    await expect(
      page.getByTestId("add-environment-step-connect"),
    ).toBeVisible();
  });

  test("removing an environment stages it rather than making it disappear", async ({
    page,
  }) => {
    await page.getByTestId("connections-add").click();
    await page.getByTestId("add-environment-host").fill(PAIRING_URL);
    await page.getByTestId("add-environment-continue").click();
    await page.getByTestId("add-environment-name").fill("Studio Mac");
    await page.getByTestId("add-environment-pair").click();
    await page.getByTestId("add-environment-done").click();

    const row = page.locator('[data-testid^="connections-row-"]');
    await expect(row).toHaveCount(1);

    await row.getByRole("button", { name: /^Remove / }).click();
    await expect(page.getByTestId("connections-remove-confirm")).toBeVisible();
    await page.getByTestId("connections-remove-confirm-action").click();

    // Still listed, now explained as in-progress — the reconciler owns the rest.
    await expect(row).toHaveCount(1);
    await expect(row).toContainText("Removing…");
    await expect(row).toContainText("No action needed");
  });

  test("the Connections section is absent while the flag is off", async ({
    page,
  }) => {
    await page.addInitScript(() => {
      (
        window as Window & { __mockUiFeatureFlags?: Record<string, boolean> }
      ).__mockUiFeatureFlags = { remoteEnvironments: false };
    });
    await setupSettings(page);

    await expect(
      page.locator('[data-testid="settings-section-connections"]'),
    ).toHaveCount(0);
  });
});
