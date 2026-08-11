/**
 * Opt-in theme audit. The theme is changed through Settings, then the retained
 * root views and the Agents planning artifact surfaces are captured.
 *
 * Run manually:
 *   RALPHX_THEME_SWITCH_AUDIT=1 npx playwright test tests/visual/theme-audit/theme-switch-via-settings.spec.ts
 */

import { test, expect, type Page } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { setupApp } from "../../fixtures/setup.fixtures";
import type { ChatConversation } from "@/types/chat-conversation";

const ENABLED = process.env.RALPHX_THEME_SWITCH_AUDIT === "1";
const OUTPUT_ROOT = join(
  process.cwd(),
  "..",
  ".artifacts",
  "theme-switch-audit",
);
const PROJECT_ID = "project-mock-1";
const CONVERSATION_ID = "theme-audit-planning-conversation";
const SESSION_ID = "theme-audit-planning-session";
const ARTIFACT_ID = "theme-audit-planning-artifact";
const TASK_ID = "theme-audit-planning-task";

type ThemeName = "dark" | "light" | "high-contrast";
const THEMES: ThemeName[] = ["dark", "light", "high-contrast"];
const ROOT_VIEWS = [
  "agents",
  "automations",
  "github",
  "insights",
  "extensibility",
  "activity",
] as const;

const planningConversation: ChatConversation = {
  id: CONVERSATION_ID,
  contextType: "project",
  contextId: PROJECT_ID,
  claudeSessionId: null,
  providerSessionId: `thread-${CONVERSATION_ID}`,
  providerHarness: "codex",
  upstreamProvider: "openai",
  providerProfile: null,
  agentMode: "ideation",
  automationId: null,
  automationRunId: null,
  coordinationMode: "solo",
  title: "Plan Agents workspace theme coverage",
  messageCount: 0,
  lastMessageAt: null,
  createdAt: "2026-07-20T10:00:00.000Z",
  updatedAt: "2026-07-20T10:00:00.000Z",
  archivedAt: null,
};

async function saveScreenshot(page: Page, theme: ThemeName, view: string) {
  const dir = join(OUTPUT_ROOT, theme);
  await mkdir(dir, { recursive: true });
  await page.screenshot({ path: join(dir, `${view}.png`), fullPage: true });
}

async function switchThemeViaSettings(page: Page, theme: ThemeName) {
  await page.evaluate(() => window.__uiStore?.getState().openModal("settings"));
  const settingsDialog = page.getByTestId("settings-dialog");
  await expect(settingsDialog).toBeVisible();
  await settingsDialog.getByTestId("settings-nav-application").click();
  await settingsDialog.getByTestId("settings-leaf-accessibility").click();

  const themeTrigger = settingsDialog.getByTestId("theme-selector");
  await expect(themeTrigger).toBeVisible();
  await themeTrigger.click();
  const labels: Record<ThemeName, string> = {
    dark: "Dark (default)",
    light: "Light",
    "high-contrast": "High contrast",
  };
  await page.getByRole("option").filter({ hasText: labels[theme] }).click();
  await page.waitForTimeout(300);
  await page.evaluate(() => window.__uiStore?.getState().closeModal());
  await page.waitForTimeout(300);
}

async function captureSettings(page: Page, theme: ThemeName) {
  await page.evaluate(() => window.__uiStore?.getState().openModal("settings"));
  await expect(page.getByTestId("settings-dialog")).toBeVisible();
  await saveScreenshot(page, theme, "settings");
  await page.evaluate(() => window.__uiStore?.getState().closeModal());
}

async function openRootView(page: Page, view: (typeof ROOT_VIEWS)[number]) {
  const navItem = page.getByTestId(`nav-${view}`);
  await navItem.click();
  await expect(navItem).toHaveAttribute("aria-current", "page");
  await page.waitForTimeout(300);
}

async function seedPlanningScenario(page: Page) {
  await page.evaluate(
    async ({ conversation, projectId, sessionId, artifactId, taskId }) => {
      const queryClient = window.__queryClient;
      const mockStore = window.__mockStore;
      if (!queryClient || !mockStore) {
        throw new Error("Expected mock store and query client to be available");
      }
      window.__mockChatApi?.reset();
      mockStore.tasks.clear();

      const {
        mockGetAgentConversationWorkspace,
        mockGetConversation,
        mockStartAgentConversation,
        seedMockAgentConversationWorkspace,
        seedMockConversation,
      } = await import("/src/api-mock/chat");
      const { mockIdeationApi } = await import("/src/api-mock/ideation");
      const { createMockTask } = await import("/src/test/mock-data");

      seedMockConversation(conversation, []);
      await mockStartAgentConversation({
        projectId,
        content: "Seed deterministic Agents planning coverage",
        conversationId: conversation.id,
        providerHarness: "codex",
        modelId: "gpt-5.4",
        mode: "ideation",
        base: {
          kind: "current_branch",
          ref: "main",
          displayName: "Current branch (main)",
        },
      });

      const now = "2026-07-20T10:00:00.000Z";
      mockIdeationApi.sessions.seedWithData({
        session: {
          id: sessionId,
          projectId,
          title: "Theme audit planning state",
          titleSource: null,
          status: "accepted",
          planArtifactId: artifactId,
          seedTaskId: null,
          parentSessionId: null,
          createdAt: now,
          updatedAt: now,
          archivedAt: null,
          convertedAt: "2026-07-20T10:02:00.000Z",
          verificationStatus: "verified",
          verificationInProgress: false,
          gapScore: null,
          sessionPurpose: "general",
          acceptanceStatus: "accepted",
        },
        proposals: [],
        messages: [],
      });

      const workspace = await mockGetAgentConversationWorkspace(
        conversation.id,
      );
      if (!workspace) throw new Error("Expected seeded Agents workspace");
      const linkedWorkspace = {
        ...workspace,
        linkedIdeationSessionId: sessionId,
        linkedPlanBranchId: null,
      };
      seedMockAgentConversationWorkspace(linkedWorkspace);

      const task = createMockTask({
        id: taskId,
        projectId,
        title: "Capture embedded planning surfaces",
        internalStatus: "backlog",
        ideationSessionId: sessionId,
        planArtifactId: sessionId,
        executionPlanId: sessionId,
      });
      mockStore.tasks.set(taskId, task);
      queryClient.setQueryData(["ideation", "settings"], {
        tasksEnabled: true,
        tasksFeatureState: "enabled",
        autoVerifyDraftPlans: true,
        autoVerifyPlans: false,
        requireAcceptForFinalize: false,
        requireVerificationForAccept: false,
        externalOverrides: {
          autoVerifyPlans: null,
          requireVerificationForAccept: null,
          requireAcceptForFinalize: null,
        },
      });
      queryClient.setQueryData(["tasks", "list", projectId], [task]);
      queryClient.setQueryData(
        ["tasks", "session-history", projectId, sessionId],
        {
          hasHistory: true,
          taskCount: 1,
        },
      );
      queryClient.setQueryData(["agents", "artifact", artifactId], {
        id: artifactId,
        type: "design_doc",
        name: "Agent Plan",
        content: {
          type: "inline",
          text: "# Agent Plan\n\nCapture the retained roots and embedded task surfaces.",
        },
        metadata: { createdAt: now, createdBy: "theme-audit", version: 1 },
        derivedFrom: [],
        bucketId: undefined,
      });
      queryClient.setQueryData(
        ["chat", "conversations", conversation.id],
        await mockGetConversation(conversation.id),
      );
      queryClient.setQueryData(
        ["agents", "conversation-workspace", conversation.id],
        linkedWorkspace,
      );
      queryClient.setQueryData(
        ["ideation", "sessions", "detail", sessionId, "with-data"],
        await mockIdeationApi.sessions.getWithData(sessionId),
      );
      await queryClient.invalidateQueries({
        queryKey: ["agents", "sidebar-conversations"],
      });
    },
    {
      conversation: planningConversation,
      projectId: PROJECT_ID,
      sessionId: SESSION_ID,
      artifactId: ARTIFACT_ID,
      taskId: TASK_ID,
    },
  );
}

async function hydratePlanningCaches(page: Page) {
  await page.evaluate(
    async ({ projectId, sessionId, artifactId, taskId }) => {
      const queryClient = window.__queryClient;
      const task = window.__mockStore?.tasks.get(taskId);
      if (!queryClient || !task)
        throw new Error("Expected seeded planning caches");
      const { mockIdeationApi } = await import("/src/api-mock/ideation");
      queryClient.setQueryData(["ideation", "settings"], {
        tasksEnabled: true,
        tasksFeatureState: "enabled",
        autoVerifyDraftPlans: true,
        autoVerifyPlans: false,
        requireAcceptForFinalize: false,
        requireVerificationForAccept: false,
        externalOverrides: {
          autoVerifyPlans: null,
          requireVerificationForAccept: null,
          requireAcceptForFinalize: null,
        },
      });
      queryClient.setQueryData(["tasks", "list", projectId], [task]);
      queryClient.setQueryData(
        ["tasks", "session-history", projectId, sessionId],
        {
          hasHistory: true,
          taskCount: 1,
        },
      );
      queryClient.setQueryData(["agents", "artifact", artifactId], {
        id: artifactId,
        type: "design_doc",
        name: "Agent Plan",
        content: {
          type: "inline",
          text: "# Agent Plan\n\nCapture the retained roots and embedded task surfaces.",
        },
        metadata: {
          createdAt: "2026-07-20T10:00:00.000Z",
          createdBy: "theme-audit",
          version: 1,
        },
        derivedFrom: [],
        bucketId: undefined,
      });
      queryClient.setQueryData(
        ["ideation", "sessions", "detail", sessionId, "with-data"],
        await mockIdeationApi.sessions.getWithData(sessionId),
      );
      const { usePlanStore } = await import("/src/stores/planStore");
      await usePlanStore
        .getState()
        .setActivePlan(projectId, sessionId, "quick_switcher", sessionId);
    },
    {
      projectId: PROJECT_ID,
      sessionId: SESSION_ID,
      artifactId: ARTIFACT_ID,
      taskId: TASK_ID,
    },
  );
}

async function openPlanningArtifacts(page: Page) {
  await openRootView(page, ROOT_VIEWS[0]);
  const row = page.getByTestId(`agents-session-${CONVERSATION_ID}`);
  await expect(row).toBeVisible();
  await row.getByRole("button").first().click();
  await page.evaluate(async (conversationId) => {
    const { useAgentSessionStore } =
      await import("/src/stores/agentSessionStore");
    useAgentSessionStore
      .getState()
      .setRuntimeForConversation(conversationId, "project-mock-1", {
        provider: "codex",
        modelId: "gpt-5.4",
      });
  }, CONVERSATION_ID);
  await page
    .getByTestId("integrated-chat-header")
    .getByRole("button", { name: "Open artifacts" })
    .click();
  const pane = page.getByTestId("agents-artifact-pane");
  await expect(pane).toBeVisible();
  await hydratePlanningCaches(page);
  await expect(page.getByTestId("agents-artifact-tab-plan")).toBeVisible();
  return pane;
}

for (const theme of THEMES) {
  test.describe(`Theme switch via Settings — ${theme}`, () => {
    test.skip(!ENABLED, "Set RALPHX_THEME_SWITCH_AUDIT=1 to run this audit.");

    test(`switches to ${theme} and captures retained roots plus Agents planning`, async ({
      page,
    }) => {
      await setupApp(page);
      await switchThemeViaSettings(page, theme);
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      await seedPlanningScenario(page);

      for (const view of ROOT_VIEWS) {
        await openRootView(page, view);
        await saveScreenshot(page, theme, view);
      }
      await captureSettings(page, theme);

      await page.getByTestId("reviews-toggle").click();
      await expect(page.getByTestId("notifications-panel")).toBeVisible();
      await saveScreenshot(page, theme, "reviews");
      await page.getByTestId("reviews-toggle").click();

      const pane = await openPlanningArtifacts(page);
      await pane.getByTestId("agents-artifact-tab-plan").click();
      await expect(
        page.getByTestId("agents-artifact-content-plan"),
      ).toBeVisible();
      await expect(
        page.getByText(
          "Capture the retained roots and embedded task surfaces.",
        ),
      ).toBeVisible();
      await saveScreenshot(page, theme, "agents-plan");

      await pane.getByTestId("agents-artifact-tab-tasks").click();
      await expect(
        page.getByTestId("agents-artifact-content-tasks"),
      ).toBeVisible();
      await pane.getByRole("button", { name: "Kanban" }).click();
      await hydratePlanningCaches(page);
      await expect(page.getByTestId("task-board")).toBeVisible();
      await expect(page.getByTestId(`task-card-${TASK_ID}`)).toBeVisible();
      await saveScreenshot(page, theme, "agents-tasks-kanban");

      await pane.getByRole("button", { name: "Graph" }).click();
      await expect(page.getByTestId("task-graph-view")).toBeVisible();
      await expect(
        page
          .getByTestId("task-node")
          .filter({ hasText: "Capture embedded planning surfaces" }),
      ).toBeVisible();
      await saveScreenshot(page, theme, "agents-tasks-graph");
    });
  });
}
