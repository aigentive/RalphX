import { expect, type Locator, type Page } from "@playwright/test";

import { setupApp } from "../../fixtures/setup.fixtures";
import { seedMergedWorkspace } from "../../fixtures/terminal-publish.fixtures";
import { BasePage } from "../base.page";

const TERMINAL_CONVERSATION_ID = "conv-agent-terminal-publish-visual";
const PROJECT_ID = "project-mock-1";

export class AgentsPublishPage extends BasePage {
  readonly terminalHeading: Locator;
  readonly historicalFilter: Locator;
  readonly inlineDiffs: Locator;
  readonly pagedDiffContent: Locator;
  readonly composerContext: Locator;

  constructor(page: Page) {
    super(page);
    this.terminalHeading = page.getByRole("heading", { name: "Pull Request Merged" });
    this.historicalFilter = page.getByTestId("diff-filter-trigger");
    this.inlineDiffs = page.getByTestId("agents-publish-inline-diffs");
    this.pagedDiffContent = this.inlineDiffs
      .getByText("inlineRowsArePaged = true")
      .first();
    this.composerContext = page.getByTestId("agents-composer-workspace-changes");
  }

  async openMergedPublishScenario() {
    await this.installPagedDiffRoute();
    await setupApp(this.page);
    await seedMergedWorkspace(this.page, TERMINAL_CONVERSATION_ID, PROJECT_ID);
    await this.page.getByTestId("nav-agents").click();
    await expect(this.page.getByTestId("agents-view")).toBeVisible();
    const conversation = this.page.getByTestId(
      `agents-session-${TERMINAL_CONVERSATION_ID}`,
    );
    await expect(conversation).toBeVisible();
    await conversation.getByRole("button").first().click();
    await this.page.evaluate(async (conversationId) => {
      const { mockGetAgentConversationWorkspace } = await import("/src/api-mock/chat");
      const workspace = await mockGetAgentConversationWorkspace(conversationId);
      if (!workspace || !window.__queryClient) {
        throw new Error("Expected terminal workspace query fixture");
      }
      window.__queryClient.setQueryData(
        ["agents", "conversation-workspace", conversationId],
        workspace,
      );
    }, TERMINAL_CONVERSATION_ID);
    await this.page.getByRole("button", { name: "Open artifacts" }).click();
    const publishTab = this.page.getByTestId("agents-artifact-tab-publish");
    await expect(publishTab).toBeVisible();
    await publishTab.click();
    await expect(this.page.getByTestId("agents-publish-pane")).toBeVisible();
  }

  private async installPagedDiffRoute() {
    await this.page.route("**/api/agent-workspaces/**/file-diff-page**", async (route) => {
      const url = new URL(route.request().url());
      const filePath = url.searchParams.get("path") ?? "frontend/src/Published.tsx";
      const rows = [
        {
          kind: "hunk_header", header: "@@ -1,2 +1,3 @@",
          old_start: 1, old_lines: 2, new_start: 1, new_lines: 3,
        },
        {
          kind: "line",
          line: {
            kind: "context",
            content: "export function publishedView() {",
            old_line_num: 1,
            new_line_num: 1,
          },
        },
        {
          kind: "line",
          line: {
            kind: "addition",
            content: "  const inlineRowsArePaged = true;",
            old_line_num: null,
            new_line_num: 2,
          },
        },
      ];
      const offset = Number(url.searchParams.get("offset") ?? "0");
      const limit = Number(url.searchParams.get("limit") ?? "200");
      const pageRows = rows.slice(offset, offset + limit);
      const nextOffset =
        offset + pageRows.length < rows.length
          ? offset + pageRows.length
          : null;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          file_path: filePath,
          language: "tsx",
          rows: pageRows,
          offset,
          limit,
          next_offset: nextOffset,
          total_rows: rows.length,
          old_total_lines: 2,
          new_total_lines: 3,
          is_binary: false,
        }),
      });
    });
  }
}
