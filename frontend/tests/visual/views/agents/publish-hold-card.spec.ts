import { expect, test } from "@playwright/test";

import type { AgentConversationWorkspacePublicationEvent } from "@/api/chat";

import { dismissProviderCliUpdateToasts } from "../../../fixtures/setup.fixtures";
import { AgentsPublishPage } from "../../../pages/views/agents-publish.page";
import { seedHeldWorkspaceScenario } from "./publish-hold-card.fixtures";

const BASE_PARITY_TIMELINE: AgentConversationWorkspacePublicationEvent[] = [
  {
    id: "hold-timeline-1",
    conversationId: "hold-base-parity",
    step: "checks_rerun_requested",
    status: "succeeded",
    summary: "Asked GitHub to re-run the cancelled checks",
    classification: null,
    attemptId: null,
    createdAt: "2026-08-02T09:58:00Z",
  },
  {
    id: "hold-timeline-2",
    conversationId: "hold-base-parity",
    step: "held",
    status: "blocked",
    summary: "Checks were cancelled before completing",
    classification: "base_parity_transient",
    attemptId: null,
    createdAt: "2026-08-02T10:01:00Z",
  },
];

test.describe("Agents publish hold card (redesigned four-layer)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.addInitScript(() => window.localStorage.clear());
  });

  test("base-parity transient hold renders collapsed and expanded Details", async ({
    page,
  }, testInfo) => {
    await dismissProviderCliUpdateToasts(page);
    const publish = new AgentsPublishPage(page);
    await publish.openRepairPendingScenario();
    await seedHeldWorkspaceScenario(page, {
      holdReason: "pr_autofix_base_parity_transient",
      timelineEvents: BASE_PARITY_TIMELINE,
    });
    await expect(publish.holdCard).toBeVisible();

    await expect(
      publish.holdCard.getByText(
        "Repair paused — checks were cancelled or timed out",
      ),
    ).toBeVisible();
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-court-chip"),
    ).toHaveText("Waiting on a re-run");
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-primary"),
    ).toHaveText("Re-run failed checks");
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-secondary"),
    ).toHaveText("Retry repair anyway");
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-more-trigger"),
    ).toBeVisible();
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-details-content"),
    ).not.toBeVisible();

    const collapsedShot = await publish.holdCard.screenshot();
    await testInfo.attach("hold-card-base-parity-collapsed", {
      body: collapsedShot,
      contentType: "image/png",
    });
    await expect(publish.holdCard).toHaveScreenshot(
      "hold-card-base-parity-collapsed.png",
      { maxDiffPixelRatio: 0.01 },
    );

    await publish.holdCard
      .getByTestId("agents-publish-hold-details-trigger")
      .click();
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-details-content"),
    ).toBeVisible();
    await expect(
      publish.holdCard.getByTestId(
        "agents-publish-hold-timeline-event-hold-timeline-2",
      ),
    ).toBeVisible();
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-release-conditions"),
    ).toContainText("different result");

    const expandedShot = await publish.holdCard.screenshot();
    await testInfo.attach("hold-card-base-parity-expanded", {
      body: expandedShot,
      contentType: "image/png",
    });
    await expect(publish.holdCard).toHaveScreenshot(
      "hold-card-base-parity-expanded.png",
      { maxDiffPixelRatio: 0.01 },
    );
  });

  test("publication_effect_attention hold renders a single primary action", async ({
    page,
  }) => {
    await dismissProviderCliUpdateToasts(page);
    const publish = new AgentsPublishPage(page);
    await publish.openRepairPendingScenario();
    await seedHeldWorkspaceScenario(page, {
      holdReason: "publication_effect_attention",
    });
    await expect(publish.holdCard).toBeVisible();

    await expect(
      publish.holdCard.getByText("Repair paused — publish not confirmed"),
    ).toBeVisible();
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-court-chip"),
    ).toHaveText("Waiting on you");
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-primary"),
    ).toHaveText("Retry publication");
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-secondary"),
    ).toHaveCount(0);
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-more-trigger"),
    ).toHaveCount(0);
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-spend"),
    ).toHaveCount(0);

    await expect(publish.holdCard).toHaveScreenshot(
      "hold-card-publication-effect-attention.png",
      { maxDiffPixelRatio: 0.01 },
    );
  });

  test("hold with reportable spend stays contained at a narrow panel width", async ({
    page,
  }) => {
    await dismissProviderCliUpdateToasts(page);
    const publish = new AgentsPublishPage(page);
    await publish.openRepairPendingScenario();
    await seedHeldWorkspaceScenario(page, {
      holdReason: "pr_autofix_unchanged_health",
      prAutofixFingerprintSpend: {
        generations: 2,
        minutes: 18,
        budgetMinutes: 45,
        isExhausted: true,
      },
    });
    await expect(publish.holdCard).toBeVisible();

    const spend = publish.holdCard.getByTestId("agents-publish-hold-spend");
    await expect(spend).toContainText("2 generations · 18 min");
    await expect(spend).toContainText("budget exhausted");
    await expect(
      publish.holdCard.getByTestId("agents-publish-hold-primary-caption"),
    ).toContainText("budget exhausted");

    await publish.expectNoPaneOverflow();
    await expect(publish.holdCard).toHaveScreenshot(
      "hold-card-spend-exhausted.png",
      { maxDiffPixelRatio: 0.01 },
    );

    const standardViewport = page.viewportSize() ?? { width: 1440, height: 900 };
    await page.setViewportSize({ width: 960, height: standardViewport.height });
    await publish.expectNoPaneOverflow();
    await expect(publish.holdCard).toHaveScreenshot(
      "hold-card-spend-exhausted-constrained.png",
      { maxDiffPixelRatio: 0.03 },
    );
    await page.setViewportSize(standardViewport);
  });
});
