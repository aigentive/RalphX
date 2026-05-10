import { expect, test, type Locator } from "@playwright/test";
import {
  setupIdeationChatScenario,
  setupTaskChatScenario,
} from "../../../fixtures/chat.fixtures";
import { IDEATION_REPLAY_CONTEXTS } from "@/api-mock/chat-scenarios";

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
  // (Apple Silicon dev vs CI runner). Empirically these widget cards drift
  // up to ~3% pixels even with no source change; 4% gives headroom while
  // still catching meaningful visual regressions.
  await expect(widget).toHaveScreenshot(snapshotName, { maxDiffPixelRatio: 0.04 });
}

type ChildSessionStatusOverride = {
  response?: unknown;
  error?: string;
  delayMs?: number;
};

async function focusIdeationWidgetBlocks(
  page: Parameters<typeof setupIdeationChatScenario>[0],
  blockIds: string[],
  childSessionOverrides?: Record<string, ChildSessionStatusOverride>,
) {
  await page.evaluate(async ({ conversationId, selectedBlockIds, overrides }) => {
    const mockChatApi = (window as Window).__mockChatApi;
    const queryClient = (window as Window).__queryClient;

    if (!mockChatApi || !queryClient) {
      throw new Error("Expected mock chat API and query client to be available");
    }

    mockChatApi.seedScenario("ideation_widget_matrix");
    for (const [sessionId, override] of Object.entries(overrides ?? {})) {
      mockChatApi.setChildSessionStatusOverride(sessionId, override);
    }

    const payload = await mockChatApi.getConversation(conversationId);
    const selected = new Set(selectedBlockIds);
    const messages = payload.messages.map((message) => {
      if (message.id !== "msg-ideation-widget-assistant-1") {
        return message;
      }
      return {
        ...message,
        contentBlocks:
          message.contentBlocks?.filter((block) => {
            if (!block || typeof block !== "object" || block.type !== "tool_use") {
              return true;
            }
            return typeof block.id === "string" && selected.has(block.id);
          }) ?? null,
      };
    });

    mockChatApi.replaceMessages(conversationId, messages);
    const focusedPayload = await mockChatApi.getConversation(conversationId);
    queryClient.setQueryData(["chat", "conversations", conversationId], focusedPayload);
    queryClient.setQueryData(["chat", "conversations", conversationId, "history"], {
      pages: [
        {
          conversation: focusedPayload.conversation,
          messages: focusedPayload.messages,
          limit: 40,
          offset: 0,
          totalMessageCount: focusedPayload.messages.length,
          hasOlder: false,
        },
      ],
      pageParams: [0],
    });
    queryClient.setQueryData(["chat", "conversations", conversationId, "timeline"], {
      pages: [await mockChatApi.getConversationTimelinePage(conversationId, 40, null)],
      pageParams: [null],
    });
  }, {
    conversationId: IDEATION_REPLAY_CONTEXTS.ideation_widget_matrix.conversationId,
    selectedBlockIds: blockIds,
    overrides: childSessionOverrides ?? {},
  });
}

const CHILD_SESSION_VISUAL_OVERRIDES = {
  "child-session-loading-1": { delayMs: 120_000 },
  "child-session-error-1": {
    error: "Unable to load child session in visual test",
  },
};

test.describe("Chat Widget Matrix", () => {
  test("proposal widget states", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix");
    await focusIdeationWidgetBlocks(page, [
      "proposal-create-1",
      "proposal-update-1",
      "proposal-delete-1",
    ]);

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
    await focusIdeationWidgetBlocks(page, [
      "verification-update-1",
      "verification-get-1",
      "verification-pending-1",
    ]);

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
    await focusIdeationWidgetBlocks(page, [
      "send-message-broadcast-1",
      "ask-question-1",
      "plan-create-1",
      "plan-update-1",
    ]);

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

  test("active child session widget state", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix", {
      childSessionOverrides: CHILD_SESSION_VISUAL_OVERRIDES,
    });

    await focusIdeationWidgetBlocks(page, ["child-session-active-1"]);
    const activeWidget = page.locator('[data-testid="child-session-widget-active"]').first();
    await expect(activeWidget).toBeVisible();
    await expandWidget(activeWidget);
    await expectAndAttachScreenshot(
      activeWidget,
      "child-session-widget-active.png",
      "child-session-widget-active",
      testInfo.attach.bind(testInfo),
    );
  });

  test("pending child session widget state", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix", {
      childSessionOverrides: CHILD_SESSION_VISUAL_OVERRIDES,
    });

    await focusIdeationWidgetBlocks(page, ["child-session-pending-1"]);
    const pendingWidget = page.locator('[data-testid="child-session-widget-pending"]').first();
    await expect(pendingWidget).toBeVisible();
    await expectAndAttachScreenshot(
      pendingWidget,
      "child-session-widget-pending.png",
      "child-session-widget-pending",
      testInfo.attach.bind(testInfo),
    );
  });

  test("loading child session widget state", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix", {
      childSessionOverrides: CHILD_SESSION_VISUAL_OVERRIDES,
    });

    await focusIdeationWidgetBlocks(page, ["child-session-loading-1"], CHILD_SESSION_VISUAL_OVERRIDES);
    const loadingWidget = page.locator('[data-testid="child-session-widget-loading"]').first();
    await expect(loadingWidget).toBeVisible();
    await expandWidget(loadingWidget);
    await expectAndAttachScreenshot(
      loadingWidget,
      "child-session-widget-loading.png",
      "child-session-widget-loading",
      testInfo.attach.bind(testInfo),
    );
  });

  test("error child session widget state", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix", {
      childSessionOverrides: CHILD_SESSION_VISUAL_OVERRIDES,
    });

    await focusIdeationWidgetBlocks(page, ["child-session-error-1"], CHILD_SESSION_VISUAL_OVERRIDES);
    const errorWidget = page.locator('[data-testid="child-session-widget-error"]').first();
    await expect(errorWidget).toBeVisible();
    await expandWidget(errorWidget);
    await expectAndAttachScreenshot(
      errorWidget,
      "child-session-widget-error.png",
      "child-session-widget-error",
      testInfo.attach.bind(testInfo),
    );
  });

  test("native delegation task card states", async ({ page }, testInfo) => {
    await setupIdeationChatScenario(page, "ideation_widget_matrix");

    await focusIdeationWidgetBlocks(page, ["delegate-wait-1"]);
    const completedDelegationCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "ralphx-execution-reviewer" })
      .first();
    await expect(completedDelegationCard).toBeVisible();
    await expectAndAttachScreenshot(
      completedDelegationCard,
      "delegation-widget-collapsed.png",
      "delegation-widget-collapsed",
      testInfo.attach.bind(testInfo),
    );
    await completedDelegationCard.getByRole("button").click();
    await expectAndAttachScreenshot(
      completedDelegationCard,
      "delegation-widget-expanded.png",
      "delegation-widget-expanded",
      testInfo.attach.bind(testInfo),
    );

    await focusIdeationWidgetBlocks(page, ["delegate-start-failed-1"]);
    const failedDelegationCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "ralphx-execution-fixer" })
      .first();
    await expect(failedDelegationCard).toBeVisible();
    await expectAndAttachScreenshot(
      failedDelegationCard,
      "delegation-widget-failed.png",
      "delegation-widget-failed",
      testInfo.attach.bind(testInfo),
    );

    await focusIdeationWidgetBlocks(page, ["delegate-start-cancelled-1"]);
    const cancelledDelegationCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "ralphx-merge-auditor" })
      .first();
    await expect(cancelledDelegationCard).toBeVisible();
    await expectAndAttachScreenshot(
      cancelledDelegationCard,
      "delegation-widget-cancelled.png",
      "delegation-widget-cancelled",
      testInfo.attach.bind(testInfo),
    );

    await focusIdeationWidgetBlocks(page, ["agent-card-1"]);
    const agentCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "frontend-researcher" })
      .first();
    await expect(agentCard).toBeVisible();
    await expectAndAttachScreenshot(
      agentCard,
      "agent-widget-collapsed.png",
      "agent-widget-collapsed",
      testInfo.attach.bind(testInfo),
    );
    await agentCard.getByRole("button").click();
    await expectAndAttachScreenshot(
      agentCard,
      "agent-widget-expanded.png",
      "agent-widget-expanded",
      testInfo.attach.bind(testInfo),
    );

    await focusIdeationWidgetBlocks(page, ["task-card-1"]);
    const taskCard = page
      .locator('[data-testid="task-tool-call-card"]')
      .filter({ hasText: "Run repository smoke checks" })
      .first();

    await expect(taskCard).toBeVisible();
    await expectAndAttachScreenshot(
      taskCard,
      "task-widget-collapsed.png",
      "task-widget-collapsed",
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
