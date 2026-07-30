import { expect, test, type Page } from "@playwright/test";

import { setupApp } from "../../../fixtures/setup.fixtures";

const projectId = "project-mock-1";
const conversationId = "conv-team-visual";

async function installTeamRoutes(page: Page) {
  await page.route("**/api/managed_team/status/**", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        session: {
          id: "team-visual-1",
          projectId,
          coordinatorConversationId: conversationId,
          status: "active",
          configuredConcurrency: 3,
          effectiveConcurrency: 2,
          automaticWakeLimit: 4,
          version: 1,
          createdAt: "2026-07-28T10:00:00Z",
          updatedAt: "2026-07-28T10:01:00Z",
        },
        members: [
          {
            id: "member-visual-1",
            teamId: "team-visual-1",
            name: "Scout",
            normalizedName: "scout",
            canonicalAgentName: "ralphx-general-explorer",
            roleSummary: "Investigates focused questions.",
            status: "idle",
            generation: 1,
          },
        ],
        usage: {
          tokens: 0,
          costMicros: 0,
          members: [],
        },
      }),
    });
  });
  await page.route("**/api/agent_tasks/list", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        success: true,
        tasks: [
          {
            task_id: "team-task-1",
            task_number: 1,
            title: "Inspect the Team view",
            state: "active",
            owner_agent: "Scout",
            blocked_by: [],
            blocks: [],
            availability: "ready",
            updated_at: "2026-07-28T10:01:00Z",
          },
        ],
      }),
    });
  });
}

async function seedTeamConversation(page: Page) {
  await page.evaluate(
    async ({ targetConversationId, targetProjectId }) => {
      const { seedMockConversation, mockStartAgentConversation } =
        await import("/src/api-mock/chat");

      seedMockConversation(
        {
          id: targetConversationId,
          contextType: "project",
          contextId: targetProjectId,
          claudeSessionId: null,
          providerSessionId: `thread-${targetConversationId}`,
          providerHarness: "codex",
          upstreamProvider: "openai",
          providerProfile: null,
          agentMode: "edit",
          automationId: null,
          automationRunId: null,
          coordinationMode: "rx_native_team",
          title: "Team visual fixture",
          messageCount: 0,
          lastMessageAt: null,
          createdAt: "2026-07-28T10:00:00Z",
          updatedAt: "2026-07-28T10:00:00Z",
          archivedAt: null,
        },
        [],
      );
      await mockStartAgentConversation({
        projectId: targetProjectId,
        content: "Seed Team fixture",
        conversationId: targetConversationId,
        providerHarness: "codex",
        modelId: "gpt-5.4",
        mode: "edit",
        capabilityIntent: { coordinationMode: "rx_native_team" },
      });

      window.__queryClient?.setQueryData(["featureFlags"], {
        agentConversationTeam: true,
      });
    },
    { targetConversationId: conversationId, targetProjectId: projectId },
  );
}

test("Team mode opens its roster board and message recipient selector", async ({ page }) => {
  await installTeamRoutes(page);
  await setupApp(page);
  await seedTeamConversation(page);
  await page.getByTestId("nav-agents").click();

  await expect(page.getByTestId("agents-view")).toBeVisible();
  const row = page.getByTestId(`agents-session-${conversationId}`);
  await expect(row).toBeVisible({ timeout: 15_000 });
  await row.getByRole("button").first().click();
  await page.evaluate(
    async ({ targetConversationId, targetProjectId }) => {
      const { useAgentSessionStore } =
        await import("/src/stores/agentSessionStore");
      useAgentSessionStore
        .getState()
        .setRuntimeForConversation(targetConversationId, targetProjectId, {
          provider: "codex",
          modelId: "gpt-5.4",
        });
      const current = window.__queryClient?.getQueryData<Record<string, boolean>>(
        ["featureFlags"],
      ) ?? {};
      window.__queryClient?.setQueryData(["featureFlags"], {
        ...current,
        agentConversationTeam: true,
      });
    },
    { targetConversationId: conversationId, targetProjectId: projectId },
  );
  const teamTabShortcut = page
    .getByTestId("agents-chat-header-toolbar")
    .getByRole("button", { name: "Team" });
  await expect(teamTabShortcut).toBeVisible();
  await teamTabShortcut.click();

  await expect(page.getByTestId("agents-team-panel")).toBeVisible();
  await expect(page.getByTestId("team-roster")).toContainText("Scout");
  await expect(page.getByTestId("team-task-board")).toContainText(
    "Inspect the Team view",
  );
  await expect(page.getByTestId("team-composer-target")).toBeVisible();
  await expect(page.getByLabel("Team message recipient")).toHaveValue(
    "coordinator",
  );
});
