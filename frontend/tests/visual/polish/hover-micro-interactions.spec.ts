import path from "node:path";
import { fileURLToPath } from "node:url";

import { test, type Page } from "@playwright/test";

import { setupApp } from "../../fixtures/setup.fixtures";

const THEMES = ["dark", "light", "high-contrast"] as const;
const SPEC_DIR = path.dirname(fileURLToPath(import.meta.url));
const OUT_DIR = path.resolve(SPEC_DIR, "../../../.artifacts/hover-polish");

async function setTheme(page: Page, theme: (typeof THEMES)[number]) {
  await page.evaluate(async (next) => {
    const { useThemeStore } = await import("/src/stores/themeStore");
    useThemeStore.getState().setTheme(next as "dark" | "light" | "high-contrast");
  }, theme);
}

for (const theme of THEMES) {
  test(`hover micro-interactions — ${theme}`, async ({ page }) => {
    await setupApp(page);
    await setTheme(page, theme);

    const topbar = page.getByTestId("app-header");
    const rail = page.getByTestId("left-nav-rail");
    const agentsSidebar = page.getByTestId("agents-sidebar");

    const shot = (name: string, body: Buffer) => {
      // body is the screenshot buffer; write deterministic filename to OUT_DIR.
    };

    await topbar.screenshot({ path: path.join(OUT_DIR, `${theme}-topbar-rest.png`) });
    await rail.screenshot({ path: path.join(OUT_DIR, `${theme}-rail-rest.png`) });
    await agentsSidebar.screenshot({ path: path.join(OUT_DIR, `${theme}-sidebar-rest.png`) });

    await page.getByTestId("theme-selector-trigger").hover();
    await topbar.screenshot({
      path: path.join(OUT_DIR, `${theme}-theme-trigger-hover.png`),
    });

    await page.getByTestId("theme-selector-trigger").click();
    await page.screenshot({
      path: path.join(OUT_DIR, `${theme}-theme-trigger-open.png`),
      clip: { x: 0, y: 0, width: 1280, height: 220 },
    });
    await page.keyboard.press("Escape");

    await page.getByTestId("font-scale-selector-trigger").hover();
    await topbar.screenshot({
      path: path.join(OUT_DIR, `${theme}-font-trigger-hover.png`),
    });
    await page.mouse.move(0, 0);

    await page.getByTestId("nav-automations").hover();
    await rail.screenshot({ path: path.join(OUT_DIR, `${theme}-rail-inactive-hover.png`) });
    await page.mouse.move(0, 0);

    await page.getByTestId("agents-new-agent").hover();
    await agentsSidebar.screenshot({
      path: path.join(OUT_DIR, `${theme}-sidebar-new-hover.png`),
    });

    await page.getByTestId("agents-filters-trigger").hover();
    await agentsSidebar.screenshot({
      path: path.join(OUT_DIR, `${theme}-sidebar-filters-hover.png`),
    });

    await page.getByTestId("agents-sort-trigger").hover();
    await agentsSidebar.screenshot({
      path: path.join(OUT_DIR, `${theme}-sidebar-sort-hover.png`),
    });

    const addProject = page.getByTestId("agents-add-project");
    if (await addProject.isVisible().catch(() => false)) {
      await addProject.hover();
      await agentsSidebar.screenshot({
        path: path.join(OUT_DIR, `${theme}-sidebar-addproject-hover.png`),
      });
    }

    void shot;
  });
}
