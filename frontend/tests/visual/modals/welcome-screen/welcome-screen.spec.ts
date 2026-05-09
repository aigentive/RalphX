/**
 * Visual regression tests for WelcomeScreen component
 */

import { test, expect } from "@playwright/test";
import { WelcomeScreenPage } from "../../../pages/modals/welcome-screen.page";
import { openWelcomeScreen } from "../../../helpers/welcome-screen.helpers";

test.describe("WelcomeScreen Visual Tests", () => {
  let welcomeScreenPage: WelcomeScreenPage;

  test.beforeEach(async ({ page }) => {
    await page.goto("http://localhost:5173");
    welcomeScreenPage = new WelcomeScreenPage(page);

    // Open the welcome screen overlay
    await openWelcomeScreen(page);
    await expect(welcomeScreenPage.container).toBeVisible();
  });

  test("should render welcome screen with all elements", async ({ page }) => {
    // Verify all key elements are present
    await expect(welcomeScreenPage.container).toBeVisible();
    await expect(welcomeScreenPage.title).toContainText("RalphX");
    await expect(welcomeScreenPage.tagline).toContainText(
      "The best way to ship software with AI",
    );
    await expect(page.getByTestId("welcome-provider-step")).toHaveAttribute(
      "data-status",
      "complete",
    );
    await expect(page.getByTestId("welcome-project-step")).toHaveAttribute(
      "data-status",
      "complete",
    );
    await expect(welcomeScreenPage.createProjectButton).toBeVisible();
    await expect(welcomeScreenPage.keyboardHint).toBeHidden();

    // Close button should be visible (manually opened)
    await expect(welcomeScreenPage.closeButton).toBeVisible();

    // Take full page screenshot
    await expect(page).toHaveScreenshot("welcome-screen-initial.png", {
      fullPage: true,
    });
  });

  test("should show animated constellation background", async ({ page }) => {
    // Verify constellation container exists
    await expect(welcomeScreenPage.constellation).toBeVisible();

    // Take screenshot showing the constellation
    await expect(page).toHaveScreenshot("welcome-screen-constellation.png", {
      fullPage: true,
    });
  });

  test("should display close button when manually opened", async ({ page }) => {
    // Close button should be visible
    await expect(welcomeScreenPage.closeButton).toBeVisible();

    // Hover over close button to check hover state
    await welcomeScreenPage.closeButton.hover();

    await expect(page).toHaveScreenshot("welcome-screen-close-button-hover.png", {
      fullPage: true,
    });
  });

  test("should display primary action button with correct styling", async ({ page }) => {
    // Verify button is visible and has correct text
    await expect(welcomeScreenPage.createProjectButton).toBeVisible();
    await expect(welcomeScreenPage.createProjectButton).toContainText(
      /Start Your First Project|Continue/,
    );

    // Hover over button to check hover state
    await welcomeScreenPage.createProjectButton.hover();

    await expect(page).toHaveScreenshot("welcome-screen-cta-button-hover.png", {
      fullPage: true,
    });
  });

  test("should hide project shortcut hint when projects already exist", async ({ page }) => {
    await expect(welcomeScreenPage.keyboardHint).toBeHidden();

    await expect(page).toHaveScreenshot("welcome-screen-keyboard-hint.png", {
      fullPage: true,
    });
  });

  test("should close welcome screen via close button", async ({ page }) => {
    // Click close button
    await welcomeScreenPage.close();

    // Wait for welcome screen to disappear
    await expect(welcomeScreenPage.container).toBeHidden();

    await expect(page).toHaveScreenshot("welcome-screen-closed.png", {
      fullPage: true,
    });
  });
});
