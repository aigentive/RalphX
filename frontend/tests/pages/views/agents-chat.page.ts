import { expect, type Locator, type Page } from "@playwright/test";

import { BasePage } from "../base.page";

export class AgentsChatPage extends BasePage {
  readonly bottomSpacer: Locator;
  readonly chrome: Locator;
  readonly composer: Locator;
  readonly composerInput: Locator;
  readonly lastRenderedRow: Locator;
  readonly messages: Locator;
  readonly panel: Locator;
  readonly scroller: Locator;
  readonly scrollToBottomButton: Locator;

  constructor(page: Page) {
    super(page);
    this.chrome = page.getByTestId("chat-below-transcript-chrome");
    this.composer = page.getByTestId("agents-conversation-composer");
    this.composerInput = this.composer.locator("textarea");
    this.messages = page.getByTestId("integrated-chat-messages");
    this.bottomSpacer = this.messages
      .getByTestId("chat-transcript-bottom-spacer")
      .filter({ visible: true });
    this.lastRenderedRow = this.messages
      .locator('[data-chat-last-rendered-row="true"]')
      .filter({ visible: true });
    this.panel = page.getByTestId("integrated-chat-panel");
    this.scroller = this.messages.locator('[data-chat-virtuoso-scroller="true"]');
    this.scrollToBottomButton = page.getByTestId("chat-scroll-to-bottom-button");
  }

  async open(): Promise<void> {
    await this.page.getByTestId("nav-agents").click();
    await expect(this.page.getByTestId("agents-view")).toBeVisible();
  }

  async seedConversation(conversationId: string, empty = false, messagePairs = 1): Promise<void> {
    await this.page.evaluate(async ({ id, isEmpty, pairCount }) => {
      const projectId = "project-mock-1";
      const createdAt = "2026-08-04T12:00:00.000Z";
      const conversation = {
        id, contextType: "project", contextId: projectId, claudeSessionId: null,
        providerSessionId: `thread-${id}`, providerHarness: "codex",
        upstreamProvider: "openai", providerProfile: null, agentMode: "edit",
        automationId: null, automationRunId: null, coordinationMode: "solo",
        title: isEmpty ? "Empty composer inset" : "Composer inset behavior",
        messageCount: isEmpty ? 0 : pairCount * 2,
        lastMessageAt: isEmpty ? null : createdAt, createdAt, updatedAt: createdAt,
        archivedAt: null,
      };
      const message = (suffix: string, role: "user" | "assistant", content: string) => ({
        id: `${id}-${suffix}`, sessionId: null, projectId, taskId: null, role,
        content, metadata: null, parentMessageId: null, conversationId: id,
        toolCalls: null, contentBlocks: null, sender: null,
        attributionSource: role === "assistant" ? "provider" : "native",
        providerHarness: role === "assistant" ? "codex" : null,
        providerSessionId: role === "assistant" ? `thread-${id}` : null,
        upstreamProvider: role === "assistant" ? "openai" : null,
        providerProfile: null, logicalModel: null, effectiveModelId: null,
        logicalEffort: null, effectiveEffort: null, inputTokens: null,
        outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null,
        estimatedUsd: null, createdAt,
      });
      const messages = isEmpty ? [] : Array.from({ length: pairCount }, (_, index) => [
        message(`user-${index}`, "user", `Existing request ${index + 1}: verify the transcript remains stable while new content arrives.`),
        message(`assistant-${index}`, "assistant", `Existing response ${index + 1}: the virtualized history is ready for bottom-follow verification.`),
      ]).flat();
      const chat = await import("/src/api-mock/chat");
      chat.seedMockConversation(conversation, messages);
      await chat.mockStartAgentConversation({
        projectId, content: "Seed composer inset", conversationId: id,
        providerHarness: "codex", modelId: "gpt-5.4", mode: "edit",
        base: { kind: "current_branch", ref: "feature/inset", displayName: "Current branch" },
      });
      const queryClient = window.__queryClient;
      if (!queryClient) throw new Error("Expected query client");
      const hydratedConversation = await chat.mockGetConversation(id);
      queryClient.setQueryData(["chat", "conversations", id], hydratedConversation);
      queryClient.setQueryData(
        ["chat", "conversations", id, "summary"],
        hydratedConversation?.conversation ?? conversation,
      );
      queryClient.setQueryData(
        ["agents", "project-conversations", projectId, "archived", false, "search", ""],
        {
          pages: [{
            conversations: [{ ...conversation, projectId, ideationSessionId: null }],
            total: 1, limit: 6, offset: 0, hasMore: false,
          }],
          pageParams: [0],
        },
      );
      const workspace = await chat.mockGetAgentConversationWorkspace(id);
      queryClient.setQueryData(["agents", "conversation-workspace", id], workspace);
      const publicationStates = ["active", "draft", "merged", "closed", "uncommitted", "unpushed"];
      const sidebar = await chat.mockListAgentSidebarConversations({
        projectIds: [projectId], includeArchived: false, archivedOnly: false,
        publicationStates, groupBy: "project", limitPerGroup: 8,
        offsets: { [projectId]: 0 }, pinnedConversationIds: [], priorityConversationIds: [],
      });
      const group = sidebar.groups.find((entry) => entry.key === projectId);
      if (!group) throw new Error("Expected seeded sidebar group");
      queryClient.setQueryData([
        "agents", "sidebar-conversations", "project", projectId, "archived", false,
        "search", "", "states", publicationStates, "pinned", [], "priority", [],
        "page-size", 8, "initial-limit", 8,
      ], { pages: [group], pageParams: [0] });
      await queryClient.refetchQueries({
        queryKey: ["agents", "sidebar-conversations", "inbox"], type: "active",
      });
    }, { id: conversationId, isEmpty: empty, pairCount: messagePairs });

    const row = this.page.getByTestId(`agents-session-${conversationId}`);
    await expect(row).toBeVisible();
    await row.getByRole("button").first().click();
    await expect(this.composer).toBeVisible();
  }

  async geometry() {
    return this.scroller.evaluate((element) => ({
      clientHeight: element.clientHeight,
      scrollHeight: element.scrollHeight,
      scrollTop: element.scrollTop,
    }));
  }

  async growComposer(): Promise<void> {
    await this.composerInput.fill("one\ntwo\nthree\nfour\nfive\nsix\nseven");
  }

  async clearComposer(): Promise<void> {
    await this.composerInput.fill("");
  }
}
