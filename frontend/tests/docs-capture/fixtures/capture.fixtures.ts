import { expect, type Page } from "@playwright/test";

import { dismissProviderCliUpdateToasts } from "../../fixtures/setup.fixtures";
import { PROD_UI_FEATURE_FLAGS } from "@/api-mock/guide-scenarios";

export { PROD_UI_FEATURE_FLAGS } from "@/api-mock/guide-scenarios";

export async function setupCapture(page: Page): Promise<void> {
  await page.clock.setFixedTime(new Date("2026-06-15T10:00:00.000Z"));
  await dismissProviderCliUpdateToasts(page);
  await page.addInitScript((flags) => {
    localStorage.setItem("ralphx-theme", "dark");
    localStorage.setItem("ralphx-motion", "reduce");
    localStorage.removeItem("ralphx-font-scale");
    window.__mockUiFeatureFlags = flags;
    // Every guide that shows GitHub state documents the connected happy path,
    // so seed authenticated gh before first paint. Setting this later races the
    // already-resolved gh-auth queries and freezes a "not authenticated" panel.
    window.__mockGhAuthStatus = true;
    // SSH origin keeps the publish surface's Git access card in its healthy
    // state; an HTTPS origin adds a credential-helper repair prompt that no
    // guide step explains.
    window.__mockGitAuthDiagnostics = {
      fetchUrl: "git@github.com:ralphx/release-companion.git",
      pushUrl: "git@github.com:ralphx/release-companion.git",
      fetchKind: "SSH",
      pushKind: "SSH",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: false,
      canSwitchToSsh: false,
      suggestedSshUrl: null,
    };
  }, PROD_UI_FEATURE_FLAGS);
  await page.goto("/", { waitUntil: "commit" });
  await page.getByTestId("app-header").waitFor({ state: "visible" });
  await expect(page.getByTestId("nav-activity")).toHaveCount(0);
  await expect(page.getByTestId("nav-extensibility")).toHaveCount(0);
}

/**
 * Capture at the project device scale so committed baselines are 3456×2160.
 *
 * To re-baseline, use `npm run test:docs-capture:update`. A bare
 * `--update-snapshots` will NOT rewrite these files — its preset mode leaves
 * existing baselines in place, so a scenario/mock fix silently keeps shipping
 * the old screenshot. Only `--update-snapshots=all` forces a rewrite.
 */
export async function captureGuideScreenshot(page: Page, name: string): Promise<void> {
  // Drop focus first: a focused control keeps its focus ring and pops its
  // tooltip, which reads as UI noise the guide never mentions.
  await page.evaluate(() => {
    const active = document.activeElement;
    if (active instanceof HTMLElement) active.blur();
  });
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  await expect(page).toHaveScreenshot(name, { scale: "device" });
}
