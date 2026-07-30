import { expect, test } from "@playwright/test";

import { captureGuideScreenshot, setupCapture } from "./fixtures/capture.fixtures";
import {
  applyFirstRunOnboarding,
  applyGuideScenario,
  hydrateGuidePlanningArtifactCache,
} from "./fixtures/guide-scenario.fixtures";
import {
  openArtifacts,
  openArtifactTab,
  openGuideConversation,
} from "./fixtures/guide-navigation.fixtures";
import { WelcomeScreenPage } from "../pages/modals/welcome-screen.page";
import { ProjectCreationWizardPage } from "../pages/modals/project-creation-wizard.page";
import { SettingsPage } from "../pages/settings.page";
import { openProjectCreationWizard } from "../helpers/project-creation-wizard.helpers";

test("welcome-provider-step", async ({ page }) => {
  await setupCapture(page); await applyGuideScenario(page, "guide_onboarding");
  await applyFirstRunOnboarding(page);
  const welcome = new WelcomeScreenPage(page);
  await expect(welcome.container).toBeVisible();
  // The guide's step is "click Set Up Provider", so the capture has to show the
  // Provider step as current rather than the already-configured Continue state.
  await expect(page.getByText("Choose your agent harness.")).toBeVisible();
  await expect(page.getByRole("button", { name: /Set Up Provider/ })).toBeVisible();
  await captureGuideScreenshot(page, "welcome-provider-step.png");
});

test("project-creation-wizard", async ({ page }) => {
  await setupCapture(page); await applyGuideScenario(page, "guide_onboarding");
  const wizard = new ProjectCreationWizardPage(page);
  await openProjectCreationWizard(page);
  await wizard.waitForModal();
  await captureGuideScreenshot(page, "project-creation-wizard.png");
});

test("agents-workspace-overview", async ({ page }) => {
  await setupCapture(page); await applyGuideScenario(page, "guide_tour");
  await openGuideConversation(page, "guide_tour");
  await openArtifacts(page);
  await hydrateGuidePlanningArtifactCache(page, "conversation-guide_tour");
  await openArtifactTab(page, "plan");
  // The guide describes this pane as where you inspect what RalphX produced, so
  // the capture must show real artifact content, not the empty plan picker.
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();
  await expect(page.getByText("Acceptance criteria")).toBeVisible();
  await captureGuideScreenshot(page, "agents-workspace-overview.png");
});

test("start-mode-picker", async ({ page }) => {
  await setupCapture(page); await applyGuideScenario(page, "guide_tour");
  await page.getByTestId("nav-agents").click();
  await page.getByRole("button", { name: "New agent" }).click();
  await page.getByTestId("agents-start-mode-chip").click();
  await expect(page.getByTestId("agents-start-mode-edit")).toBeVisible();
  await captureGuideScreenshot(page, "start-mode-picker.png");
});

test("settings-overview", async ({ page }) => {
  await setupCapture(page); await applyGuideScenario(page, "guide_tour");
  const settings = new SettingsPage(page);
  await settings.openViaStore("providers");
  await settings.waitForSection("providers", "Providers");
  await captureGuideScreenshot(page, "settings-overview.png");
});
