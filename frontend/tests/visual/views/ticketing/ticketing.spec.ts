import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

import { setupApp } from "../../../fixtures/setup.fixtures";

type ThemeName = "dark" | "light" | "high-contrast";

const THEMES: ThemeName[] = ["dark", "light", "high-contrast"];

const TICKETING_FLAGS = {
  activityPage: true,
  extensibilityPage: true,
  battleMode: true,
  teamMode: false,
  atlassianOauth: false,
  ticketingDashboard: true,
};

async function applyTheme(page: Page, theme: ThemeName) {
  await page.addInitScript((nextTheme) => {
    localStorage.setItem("ralphx-theme", nextTheme);
  }, theme);
}

async function enableTicketing(page: Page) {
  await page.evaluate((flags) => {
    const testWindow = window as Window & {
      __queryClient?: { setQueryData(queryKey: unknown[], data: unknown): void };
      __uiStore?: {
        getState(): {
          setFeatureFlags(flags: typeof TICKETING_FLAGS): void;
          setCurrentView(view: "ticketing"): void;
        };
      };
    };
    testWindow.__queryClient?.setQueryData(["featureFlags"], flags);
    testWindow.__uiStore?.getState().setFeatureFlags(flags);
  }, TICKETING_FLAGS);
}

async function openTicketing(page: Page) {
  await setupApp(page);
  await enableTicketing(page);
  await page.locator('[data-testid="nav-ticketing"]').click();
  await page.locator('[data-testid="ticketing-dashboard"]').waitFor({ timeout: 10000 });
  await expect(page.getByRole("heading", { name: "Ticketing" })).toBeVisible();
}

async function expectNoAxeViolations(page: Page) {
  const results = await new AxeBuilder({ page })
    .include('[data-testid="ticketing-dashboard"]')
    .analyze();
  expect(results.violations).toEqual([]);
}

for (const theme of THEMES) {
  test.describe(`Ticketing view - ${theme}`, () => {
    test.beforeEach(async ({ page }) => {
      await applyTheme(page, theme);
      await openTicketing(page);
    });

    test("list shell matches visual and accessibility contract", async ({ page }) => {
      await expect(page.getByRole("button", { name: /RX-1/ })).toBeVisible();
      await expectNoAxeViolations(page);
      await expect(page).toHaveScreenshot(`ticketing-list-${theme}.png`, {
        fullPage: false,
        maxDiffPixelRatio: 0.02,
      });
    });

    test("filter controls preserve keyboard focus order", async ({ page }) => {
      await page.getByLabel("Search tickets").focus();
      await expect(page.getByLabel("Search tickets")).toBeFocused();

      await page.keyboard.press("Tab");
      await expect(page.getByRole("button", { name: "Reset" })).toBeFocused();

      await page.keyboard.press("Tab");
      await expect(page.getByRole("button", { name: "List view" })).toBeFocused();

      await page.keyboard.press("Tab");
      await expect(page.getByRole("button", { name: "Kanban view" })).toBeFocused();

      await page.keyboard.press("Tab");
      await expect(page.getByRole("button", { name: "Refresh tickets" })).toBeFocused();
    });

    test("kanban shell matches visual contract", async ({ page }) => {
      await page.getByRole("button", { name: "Kanban view" }).click();
      await expect(page.locator('[data-testid="ticket-column-todo"]')).toBeVisible();
      await expect(page).toHaveScreenshot(`ticketing-kanban-${theme}.png`, {
        fullPage: false,
        maxDiffPixelRatio: 0.02,
      });
    });

    test("detail sheet matches visual contract", async ({ page }) => {
      await page.getByRole("button", { name: /RX-1/ }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await expect(page.getByText("RalphX Work")).toBeVisible();
      await expect(page).toHaveScreenshot(`ticketing-detail-${theme}.png`, {
        fullPage: false,
        maxDiffPixelRatio: 0.02,
      });
    });
  });
}
