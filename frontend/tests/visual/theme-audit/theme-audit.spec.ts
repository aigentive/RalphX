/**
 * Theme Audit — captures each major surface under dark / light / high-contrast
 * so a human or follow-up agent can walk the screenshots and flag token
 * mismatches, invisible text, or overlay glitches.
 *
 * Output: `.artifacts/theme-audit/<theme>/<view>.png`
 *
 * Run manually only — not part of normal CI. Triggered via:
 *   RALPHX_THEME_AUDIT=1 npx playwright test tests/visual/theme-audit
 *
 * Does NOT assert visually — it only captures frames. The audit itself is
 * done by reading the saved PNGs afterwards.
 */

import { test, expect, type Page } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

const ENABLED = process.env.RALPHX_THEME_AUDIT === "1";
const OUTPUT_ROOT = join(process.cwd(), "..", ".artifacts", "theme-audit");

type ThemeName = "dark" | "light" | "high-contrast";
const THEMES: ThemeName[] = ["dark", "light", "high-contrast"];

async function applyTheme(page: Page, theme: ThemeName) {
  await page.addInitScript((t) => {
    try {
      localStorage.setItem("ralphx-theme", t);
    } catch {
      /* noop */
    }
  }, theme);
}

async function saveScreenshot(page: Page, theme: ThemeName, view: string) {
  const dir = join(OUTPUT_ROOT, theme);
  await mkdir(dir, { recursive: true });
  const path = join(dir, `${view}.png`);
  await page.screenshot({ path, fullPage: true });
  return path;
}

async function waitForApp(page: Page) {
  await page.waitForSelector('[data-testid="app-header"]', { timeout: 15000 });
}

async function openView(page: Page, view: string) {
  await page.click(`[data-testid="nav-${view}"]`);
  await page.waitForTimeout(400);
}

for (const theme of THEMES) {
  test.describe(`Theme audit — ${theme}`, () => {
    test.skip(!ENABLED, "Set RALPHX_THEME_AUDIT=1 to run the theme audit.");

    test.beforeEach(async ({ page }) => {
      await applyTheme(page, theme);
      await page.goto("/");
      await waitForApp(page);
    });

    test("agents", async ({ page }) => {
      await openView(page, "agents");
      await page.waitForTimeout(600);
      const path = await saveScreenshot(page, theme, "agents");
      expect(path).toContain(".png");
    });

    test("activity", async ({ page }) => {
      await openView(page, "activity");
      await page.waitForSelector('[data-testid="activity-view"]', { timeout: 10000 });
      const path = await saveScreenshot(page, theme, "activity");
      expect(path).toContain(".png");
    });

    test("insights", async ({ page }) => {
      await openView(page, "insights");
      await page.waitForTimeout(800);
      const path = await saveScreenshot(page, theme, "insights");
      expect(path).toContain(".png");
    });

    test("extensibility", async ({ page }) => {
      await openView(page, "extensibility");
      await page.waitForSelector('[data-testid="extensibility-view"]', { timeout: 10000 });
      const path = await saveScreenshot(page, theme, "extensibility");
      expect(path).toContain(".png");
    });

    test("settings", async ({ page }) => {
      await page.evaluate(() => {
        const uiStore = (window as unknown as {
          __uiStore?: { getState(): { openModal(t: string): void } };
        }).__uiStore;
        uiStore?.getState().openModal("settings");
      });
      await page.waitForSelector('[data-testid="settings-dialog"]', { timeout: 10000 });
      await page.waitForTimeout(400);
      const path = await saveScreenshot(page, theme, "settings");
      expect(path).toContain(".png");
    });

    test("reviews panel", async ({ page }) => {
      await page.click('[data-testid="reviews-toggle"]');
      await page.waitForSelector('[data-testid="notifications-panel"]', { timeout: 10000 });
      const path = await saveScreenshot(page, theme, "reviews");
      expect(path).toContain(".png");
    });

  });
}
