/**
 * Remote Access pairing journey (PR 1.7) — runs against the web-mode mock host
 * in src/mocks/tauri-api-core.ts (`mockRemoteHost`).
 *
 * PHASE-REVIEW EXECUTION: this spec is written test-first but intentionally
 * skipped in the PR 1.7 CI lane per the PR contract (no Playwright execution in
 * this PR). Remove `.skip` during the phase review pass, alongside the C-7
 * WKWebView verification.
 */

import { expect, test } from "@playwright/test";

import { setupSettings } from "../fixtures/setup.fixtures";

test.describe.skip("Remote Access pairing journey (phase-review execution)", () => {
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
    await page.click('[data-testid="settings-section-remote-access"]');
    await page.waitForSelector('[data-testid="remote-access-section"]');
  });

  test("enables the listener, pairs a device, and cancels the code", async ({
    page,
  }) => {
    // Shell paints with the listener still disabled.
    const enableToggle = page.getByTestId("remote-enable-toggle");
    await expect(enableToggle).toHaveAttribute("aria-checked", "false");

    // Enable → optimistic flip, then bind address from the mock host.
    await enableToggle.click();
    await expect(enableToggle).toHaveAttribute("aria-checked", "true");
    await expect(page.getByText(/Listening on/)).toBeVisible();

    // Advertised endpoint for serve mode.
    await expect(page.getByTestId("remote-endpoints")).toContainText(
      "https://ralphx-host.tailnet.ts.net",
    );

    // Generate a pairing code.
    await page.getByTestId("remote-pair-device").click();
    const card = page.getByTestId("remote-pairing-card");
    await expect(card).toBeVisible();

    // Grouped manual-entry code (R-12) and hash-fragment URL (§3.7).
    await expect(card.getByTestId("remote-pairing-code")).toHaveText(
      "rxp_MOCK CODE MOCK CODE MOCK CODE MOCK CODE",
    );
    await expect(card.getByTestId("remote-pairing-url-value")).toHaveText(
      "ralphx://pair?host=https%3A%2F%2Fralphx-host.tailnet.ts.net#code=rxp_MOCKCODEMOCKCODEMOCKCODEMOCKCODE",
    );
    await expect(card.getByTestId("remote-pairing-countdown")).toContainText(
      /Expires in (10:00|9:5\d)/,
    );

    // Cancel is immediate: the card disappears without waiting for the backend.
    await card.getByTestId("remote-pairing-cancel").click();
    await expect(card).toBeHidden();
    await expect(page.getByText("No outstanding pairing codes.")).toBeVisible();
  });

  test("grants agent control only through the explicit warning", async ({
    page,
  }) => {
    const toggle = page.getByTestId("remote-agent-control-dev-mock-1");
    await expect(toggle).toHaveAttribute("aria-checked", "false");

    await toggle.click();
    const warning = page.getByTestId("remote-agent-warning");
    await expect(warning).toBeVisible();
    await expect(warning).toContainText(/kanban/i);
    await expect(warning).toContainText(/start and steer agents/i);
    await expect(warning).toContainText(/code execution on this Mac/i);

    // Cancel first: nothing committed.
    await page.getByTestId("remote-agent-warning-cancel").click();
    await expect(toggle).toHaveAttribute("aria-checked", "false");

    // Confirm path commits the grant.
    await toggle.click();
    await page.getByTestId("remote-agent-warning-confirm").click();
    await expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  test("disconnects sessions and revokes the device immediately", async ({
    page,
  }) => {
    // Live session from the mock seed.
    const sessionRow = page.getByTestId("remote-session-sess-mock-1");
    await expect(sessionRow).toBeVisible();
    await sessionRow.getByTestId("remote-session-disconnect-sess-mock-1").click();
    await expect(sessionRow).toBeHidden();

    // Revoke goes through the confirm dialog, then the row shows Revoked.
    await page.getByTestId("remote-device-revoke-dev-mock-1").click();
    await page.getByTestId("remote-device-revoke-confirm-action").click();
    await expect(
      page.getByTestId("remote-device-dev-mock-1").getByText("Revoked"),
    ).toBeVisible();
  });
});
