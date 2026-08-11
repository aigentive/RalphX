import { Page } from "@playwright/test";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";

interface UiStoreHandle {
  getState(): {
    setActiveQuestion(sessionId: string, question: AskUserQuestionPayload): void;
  };
}

/**
 * Select the mock ideation session used by web-mode chat tests.
 * The question banner is scoped to the current session id.
 */
export async function selectIdeationSession(page: Page, sessionId = "session-mock-1"): Promise<void> {
  await page.evaluate(async (id) => {
    const { useIdeationStore } = await import("/src/stores/ideationStore");
    const session = {
      id,
      projectId: "project-mock-1",
      title: "Demo Ideation Session",
      titleSource: null,
      status: "active",
      planArtifactId: null,
      seedTaskId: null,
      parentSessionId: null,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      archivedAt: null,
      convertedAt: null,
      verificationStatus: "unverified",
      verificationInProgress: false,
      gapScore: null,
      sessionPurpose: "general",
      acceptanceStatus: null,
    };
    useIdeationStore.getState().selectSession(session);
  }, sessionId);
}

/**
 * Trigger an ask-user-question banner in web mode by directly manipulating uiStore.
 *
 * In web mode, visibility is controlled by uiStore.activeQuestions[sessionId].
 */
export async function triggerAskUserQuestionBanner(
  page: Page,
  question: AskUserQuestionPayload,
  sessionId = question.sessionId ?? "session-mock-1"
): Promise<void> {
  await page.evaluate(({ payload, targetSessionId }) => {
    const uiStore = (window as Window & { __uiStore?: UiStoreHandle }).__uiStore;
    if (uiStore && typeof uiStore.getState === "function") {
      uiStore.getState().setActiveQuestion(targetSessionId, {
        ...payload,
        sessionId: targetSessionId,
      });
    } else {
      throw new Error("uiStore not available. Make sure app is running in web mode.");
    }
  }, { payload: question, targetSessionId: sessionId });

  await page.waitForTimeout(200);
}

/**
 * Create a sample single-select question for testing
 */
export function createSingleSelectQuestion(): AskUserQuestionPayload {
  return {
    requestId: "req-single-select",
    taskId: "test-task-123",
    sessionId: "session-mock-1",
    header: "Authentication Method",
    question: "Which authentication method should we use?",
    options: [
      { label: "JWT tokens", description: "Recommended for APIs" },
      { label: "Session cookies", description: "Traditional web sessions" },
      { label: "OAuth only", description: "Third-party auth providers" },
    ],
    multiSelect: false,
  };
}

/**
 * Create a sample multi-select question for testing
 */
export function createMultiSelectQuestion(): AskUserQuestionPayload {
  return {
    requestId: "req-multi-select",
    taskId: "test-task-456",
    sessionId: "session-mock-1",
    header: "Features to Enable",
    question: "Which features do you want to enable?",
    options: [
      { label: "Dark mode", description: "Enable dark theme support" },
      { label: "Analytics", description: "Track user behavior" },
      { label: "Notifications", description: "Push notification support" },
    ],
    multiSelect: true,
  };
}
