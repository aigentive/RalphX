import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

import { setupApp } from "../../../fixtures/setup.fixtures";

type ThemeName = "dark" | "light" | "high-contrast";

const THEMES: ThemeName[] = ["dark", "light", "high-contrast"];

const TICKETING_FLAGS = {
  activityPage: true,
  extensibilityPage: true,
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
  // The provider requires a project selection before tickets/statuses load; pick
  // the mock project so the list, kanban, and detail render. Scope to the dashboard
  // since "Project" also labels the top-bar project selector.
  const dashboard = page.getByTestId("ticketing-dashboard");
  const projectSelect = dashboard.getByRole("combobox", { name: "Project" });
  await projectSelect.click();
  await page.getByRole("option", { name: /^RalphX/ }).click();
  await expect(projectSelect).toContainText("RalphX");
  await expect(dashboard.getByTestId("ticketing-visible-count")).toHaveText("3");
  await expect(dashboard.getByRole("button", { name: "Statuses" })).toBeVisible();
}

async function expectNoAxeViolations(page: Page, selector = '[data-testid="ticketing-dashboard"]') {
  const results = await new AxeBuilder({ page })
    .include(selector)
    .analyze();
  expect(results.violations).toEqual([]);
}

async function expectFocusInsideTicketDialog(page: Page) {
  await expect.poll(() =>
    page.evaluate(() => Boolean(document.activeElement?.closest('[role="dialog"]'))),
  ).toBe(true);
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
      // A search term keeps the Reset button mounted (it only renders with active
      // filters) so its keyboard-focus position is deterministic.
      await page.getByLabel("Search tickets").fill("RX");
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

    test("ticketing surfaces do not leak primitive theme tokens", async ({ page }) => {
      const leaks = await page
        .locator('[data-testid="ticketing-dashboard"] *')
        .evaluateAll((elements) => {
          const primitiveStyle = /(?:#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\()/;
          const paletteClass = /\b(?:bg|text|border)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3}\b/;
          return elements.flatMap((element) => {
            const style = element.getAttribute("style") ?? "";
            const className = typeof element.className === "string" ? element.className : "";
            const issues: string[] = [];
            if (primitiveStyle.test(style)) {
              issues.push(`${element.tagName.toLowerCase()} style=${style}`);
            }
            if (paletteClass.test(className)) {
              issues.push(`${element.tagName.toLowerCase()} class=${className}`);
            }
            return issues;
          });
        });

      expect(leaks).toEqual([]);
    });

    test("reduced motion and font scale remain readable", async ({ page }) => {
      await page.evaluate(async () => {
        const { useThemeStore } = await import("/src/stores/themeStore");
        useThemeStore.getState().setMotion("reduce");
        useThemeStore.getState().setFontScale("lg");
      });

      await expect(page.locator("html")).toHaveAttribute("data-motion", "reduce");
      await expect(page.locator("html")).toHaveAttribute("data-font-scale", "lg");
      await expect(page.getByRole("button", { name: /RX-1/ })).toBeVisible();

      const metrics = await page.getByRole("button", { name: "Refresh tickets" }).evaluate((button) => {
        const buttonStyle = getComputedStyle(button);
        const rootStyle = getComputedStyle(document.documentElement);
        return {
          fontSize: Number.parseFloat(rootStyle.fontSize),
          transitionDuration: buttonStyle.transitionDuration,
        };
      });
      expect(metrics.fontSize).toBeGreaterThan(16);
      expect(["0.01ms", "1e-05s"]).toContain(metrics.transitionDuration);
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
      await expect(page.getByRole("heading", { name: "RalphX Work" })).toBeVisible();
      await expectNoAxeViolations(page, '[role="dialog"]');
      await expect(page).toHaveScreenshot(`ticketing-detail-${theme}.png`, {
        fullPage: false,
        maxDiffPixelRatio: 0.02,
      });
    });

    test("detail sheet traps keyboard focus", async ({ page }) => {
      await page.getByRole("button", { name: /RX-1/ }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await page.getByRole("button", { name: "Close" }).focus();
      await expectFocusInsideTicketDialog(page);

      for (let index = 0; index < 12; index += 1) {
        await page.keyboard.press("Tab");
        await expectFocusInsideTicketDialog(page);
      }
    });
  });
}
