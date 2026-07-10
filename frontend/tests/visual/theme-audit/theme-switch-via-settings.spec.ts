/**
 * Theme-switch-via-Settings audit — proves the theme selector in the Settings
 * dialog actually flips the UI across all surfaces. Unlike the sibling
 * `theme-audit.spec.ts`, this one does NOT inject theme via localStorage; it
 * clicks the real Settings → Theme dropdown, closes Settings, and then walks
 * every major view capturing screenshots.
 *
 * Output: `.artifacts/theme-switch-audit/<theme>/<view>.png`
 *
 * Run manually:
 *   RALPHX_THEME_SWITCH_AUDIT=1 npx playwright test tests/visual/theme-audit/theme-switch-via-settings.spec.ts
 */

import { test, expect, type Page } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

const ENABLED = process.env.RALPHX_THEME_SWITCH_AUDIT === "1";
const OUTPUT_ROOT = join(process.cwd(), "..", ".artifacts", "theme-switch-audit");

type ThemeName = "dark" | "light" | "high-contrast";
const THEMES: ThemeName[] = ["dark", "light", "high-contrast"];

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

async function openSettings(page: Page) {
  await page.evaluate(() => {
    const uiStore = (window as unknown as {
      __uiStore?: { getState(): { openModal(t: string): void } };
    }).__uiStore;
    uiStore?.getState().openModal("settings");
  });
  await page.waitForSelector('[data-testid="settings-dialog"]', { timeout: 10000 });
}

async function closeSettings(page: Page) {
  await page.evaluate(() => {
    const uiStore = (window as unknown as {
      __uiStore?: { getState(): { closeModal(): void } };
    }).__uiStore;
    uiStore?.getState().closeModal();
  });
  await page.waitForTimeout(300);
}

async function switchThemeViaSettings(page: Page, theme: ThemeName) {
  await openSettings(page);
  // Navigate to Accessibility section (where the Theme select lives)
  await page.evaluate(() => {
    const uiStore = (window as unknown as {
      __uiStore?: { getState(): { openModal(t: string, ctx?: Record<string, unknown>): void } };
    }).__uiStore;
    uiStore?.getState().openModal("settings", { section: "accessibility" });
  });
  // The Accessibility section may not auto-activate via section ctx; click the
  // sidebar item as a fallback.
  const accessibilityNavItem = page.locator('text=Accessibility').first();
  if (await accessibilityNavItem.isVisible().catch(() => false)) {
    await accessibilityNavItem.click();
  }
  await page.waitForTimeout(400);

  // Open the theme selector and pick the target option.
  const themeTrigger = page.locator('[data-testid="theme-selector"]');
  await themeTrigger.waitFor({ timeout: 5000 });
  await themeTrigger.click();

  // Radix Select renders options with label + description concatenated in
  // textContent ("LightNear-white surfaces..."), so match the leading label
  // text via the label span only.
  const labelSpans: Record<ThemeName, string> = {
    dark: "Dark (default)",
    light: "Light",
    "high-contrast": "High contrast",
  };
  await page
    .locator(`[role="option"]`)
    .filter({ has: page.locator(`span:text-is("${labelSpans[theme]}")`) })
    .click();

  // Let theme attributes propagate + close settings.
  await page.waitForTimeout(300);
  await closeSettings(page);
}

async function openView(page: Page, view: string) {
  await page.click(`[data-testid="nav-${view}"]`);
  await page.waitForTimeout(400);
}

for (const theme of THEMES) {
  test.describe(`Theme switch via Settings — ${theme}`, () => {
    test.skip(!ENABLED, "Set RALPHX_THEME_SWITCH_AUDIT=1 to run this audit.");

    test(`switches to ${theme} and captures views`, async ({ page }) => {
      await page.goto("/");
      await waitForApp(page);

      await switchThemeViaSettings(page, theme);

      // Confirm the attribute actually flipped (this is the regression guard).
      const appliedTheme = await page.evaluate(() =>
        document.documentElement.getAttribute("data-theme"),
      );
      expect(appliedTheme).toBe(theme);

      // Capture each major view with the selected theme active.
      const views = [
        "agents",
        "ideation",
        "graph",
        "kanban",
        "activity",
        "extensibility",
      ];

      for (const view of views) {
        await openView(page, view);
        await saveScreenshot(page, theme, view);
      }

      // Settings dialog itself (re-open to capture)
      await openSettings(page);
      await page.waitForTimeout(300);
      await saveScreenshot(page, theme, "settings");
      await closeSettings(page);

      // Reviews panel toggle
      await page.click('[data-testid="reviews-toggle"]');
      await page.waitForSelector('[data-testid="notifications-panel"]', { timeout: 10000 });
      await saveScreenshot(page, theme, "reviews");
      await page.click('[data-testid="reviews-toggle"]');
    });
  });
}
