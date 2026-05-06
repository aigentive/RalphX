import { expect, test, type Locator } from "@playwright/test";
import {
  setupIdeationChatScenario,
  setupTaskChatScenario,
} from "../../../fixtures/chat.fixtures";

async function expandWidget(widget: Locator) {
  await widget.locator('[role="button"]').first().click();
}

async function expectAndAttachScreenshot(
  widget: Locator,
  snapshotName: string,
  _attachmentName: string,
  _attach: (name: string, options: { body: Buffer; contentType: string }) => Promise<void>,
) {
  // Tolerate minor cross-platform font/anti-aliasing rendering differences
  // (Apple Silicon dev vs CI runner) that emit a sub-2% pixel diff without
  // any source change.
  await expect(widget).toHaveScreenshot(snapshotName, { maxDiffPixelRatio: 0.025 });
}

test.describe("Chat Widget Matrix", () => {
  test("proposal widget states", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix");

    const createWidget = page.locator('[data-testid="proposal-widget-created"]');
    const updateWidget = page.locator('[data-testid="proposal-widget-updated"]');
    const deleteWidget = page.locator('[data-testid="proposal-widget-deleted"]');

    await expect(createWidget).toBeVisible();
    await expect(updateWidget).toBeVisible();
    await expect(deleteWidget).toBeVisible();

    await expectAndAttachScreenshot(
      createWidget,
      "proposal-widget-created.png",
      "proposal-widget-created",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      updateWidget,
      "proposal-widget-updated.png",
      "proposal-widget-updated",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      deleteWidget,
      "proposal-widget-deleted.png",
      "proposal-widget-deleted",
      testInfo.attach.bind(testInfo),
    );
  });

  test("verification widget states", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix");

    const roundReportWidget = page.locator('[data-testid="verification-widget-round-report"]');
    const getWidget = page.locator('[data-testid="verification-widget-get"]');
    const pendingWidget = page.locator('[data-testid="verification-widget-pending"]');

    await expect(roundReportWidget).toBeVisible();
    await expect(getWidget).toBeVisible();
    await expect(pendingWidget).toBeVisible();

    await expectAndAttachScreenshot(
      roundReportWidget,
      "verification-widget-round-report.png",
      "verification-widget-round-report",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      getWidget,
      "verification-widget-get.png",
      "verification-widget-get",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      pendingWidget,
      "verification-widget-pending.png",
      "verification-widget-pending",
      testInfo.attach.bind(testInfo),
    );
  });

  test("send message and ideation widget states", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix");

    const sendMessageWidget = page.locator('[data-testid="send-message-widget-broadcast"]');
    const askQuestionWidget = page.locator('[data-testid="ideation-widget-ask-question"]');
    const createPlanWidget = page.locator('[data-testid="ideation-widget-create-plan"]');
    const updatePlanWidget = page.locator('[data-testid="ideation-widget-update-plan"]');

    await expect(sendMessageWidget).toBeVisible();
    await expect(askQuestionWidget).toBeVisible();
    await expect(createPlanWidget).toBeVisible();
    await expect(updatePlanWidget).toBeVisible();

    await sendMessageWidget.getByRole("button").click();

    await expectAndAttachScreenshot(
      sendMessageWidget,
      "send-message-widget-broadcast.png",
      "send-message-widget-broadcast",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      askQuestionWidget,
      "ideation-widget-ask-question.png",
      "ideation-widget-ask-question",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      createPlanWidget,
      "ideation-widget-create-plan.png",
      "ideation-widget-create-plan",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      updatePlanWidget,
      "ideation-widget-update-plan.png",
      "ideation-widget-update-plan",
      testInfo.attach.bind(testInfo),
    );
  });

  test("child session widget states", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix", {
      childSessionOverrides: {
        "child-session-loading-1": { delayMs: 10_000 },
        "child-session-error-1": {
          error: "Unable to load child session in visual test",
        },
      },
    });

    const activeWidget = page.locator('[data-testid="child-session-widget-active"]').first();
    const pendingWidget = page.locator('[data-testid="child-session-widget-pending"]').first();
    const loadingWidget = page.locator('[data-testid="child-session-widget-loading"]').first();
    const errorWidget = page.locator('[data-testid="child-session-widget-error"]').first();

    await expect(activeWidget).toBeVisible();
    await expect(pendingWidget).toBeVisible();
    await expect(loadingWidget).toBeVisible();
    await expect(errorWidget).toBeVisible();

    await expandWidget(activeWidget);
    await expandWidget(loadingWidget);
    await expandWidget(errorWidget);

    await expectAndAttachScreenshot(
      activeWidget,
      "child-session-widget-active.png",
      "child-session-widget-active",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      pendingWidget,
      "child-session-widget-pending.png",
      "child-session-widget-pending",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      loadingWidget,
      "child-session-widget-loading.png",
      "child-session-widget-loading",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      errorWidget,
      "child-session-widget-error.png",
      "child-session-widget-error",
      testInfo.attach.bind(testInfo),
    );
  });

  test("native delegation task card states", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix");

    const completedDelegationCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "ralphx-execution-reviewer" })
      .first();
    const failedDelegationCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "ralphx-execution-fixer" })
      .first();
    const cancelledDelegationCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "ralphx-merge-auditor" })
      .first();
    const agentCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "frontend-researcher" })
      .first();
    const taskCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "Run repository smoke checks" })
      .first();

    await expect(completedDelegationCard).toBeVisible();
    await expect(failedDelegationCard).toBeVisible();
    await expect(cancelledDelegationCard).toBeVisible();
    await expect(agentCard).toBeVisible();
    await expect(taskCard).toBeVisible();

    await expectAndAttachScreenshot(
      completedDelegationCard,
      "delegation-widget-collapsed.png",
      "delegation-widget-collapsed",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      failedDelegationCard,
      "delegation-widget-failed.png",
      "delegation-widget-failed",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      cancelledDelegationCard,
      "delegation-widget-cancelled.png",
      "delegation-widget-cancelled",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      agentCard,
      "agent-widget-collapsed.png",
      "agent-widget-collapsed",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      taskCard,
      "task-widget-collapsed.png",
      "task-widget-collapsed",
      testInfo.attach.bind(testInfo),
    );

    await completedDelegationCard.getByRole("button").click();
    await expectAndAttachScreenshot(
      completedDelegationCard,
      "delegation-widget-expanded.png",
      "delegation-widget-expanded",
      testInfo.attach.bind(testInfo),
    );

    await agentCard.getByRole("button").click();
    await expectAndAttachScreenshot(
      agentCard,
      "agent-widget-expanded.png",
      "agent-widget-expanded",
      testInfo.attach.bind(testInfo),
    );

    await taskCard.getByRole("button").click();
    await expectAndAttachScreenshot(
      taskCard,
      "task-widget-expanded.png",
      "task-widget-expanded",
      testInfo.attach.bind(testInfo),
    );
  });

  test("review widget states", async ({ page }, testInfo) => {
    await setupTaskChatScenario(page, "review_widget_matrix");

    const completeWidget = page.locator('[data-testid="review-widget-complete"]');
    const notesWidget = page.locator('[data-testid="review-widget-notes"]');

    await expect(completeWidget).toBeVisible();
    await expect(notesWidget).toBeVisible();

    await completeWidget.click();
    await notesWidget.getByRole("button").click();

    await expectAndAttachScreenshot(
      completeWidget,
      "review-widget-complete.png",
      "review-widget-complete",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      notesWidget,
      "review-widget-notes.png",
      "review-widget-notes",
      testInfo.attach.bind(testInfo),
    );
  });

  test("merge widget states", async ({ page }, testInfo) => {
    await setupTaskChatScenario(page, "merge_widget_matrix");

    const targetWidget = page.locator('[data-testid="merge-widget-target"]');
    const conflictWidget = page.locator('[data-testid="merge-widget-conflict"]');
    const incompleteWidget = page.locator('[data-testid="merge-widget-incomplete"]');
    const completeWidget = page.locator('[data-testid="merge-widget-complete"]');

    await expect(targetWidget).toBeVisible();
    await expect(conflictWidget).toBeVisible();
    await expect(incompleteWidget).toBeVisible();
    await expect(completeWidget).toBeVisible();

    await conflictWidget.getByRole("button").click();
    await incompleteWidget.getByRole("button").click();

    await expectAndAttachScreenshot(
      targetWidget,
      "merge-widget-target.png",
      "merge-widget-target",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      conflictWidget,
      "merge-widget-conflict.png",
      "merge-widget-conflict",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      incompleteWidget,
      "merge-widget-incomplete.png",
      "merge-widget-incomplete",
      testInfo.attach.bind(testInfo),
    );
    await expectAndAttachScreenshot(
      completeWidget,
      "merge-widget-complete.png",
      "merge-widget-complete",
      testInfo.attach.bind(testInfo),
    );
  });
});
