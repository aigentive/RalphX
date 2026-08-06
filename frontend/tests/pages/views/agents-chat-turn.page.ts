import { expect, type Page } from "@playwright/test";

import type { AgentsChatPage } from "./agents-chat.page";

const PROJECT_ID = "project-mock-1";
const FINALIZED_MESSAGE_HANDOFF_TIMEOUT_MS = 15_000;

export class AgentsChatTurnPage {
  constructor(
    private readonly page: Page,
    private readonly chat: AgentsChatPage,
    private readonly conversationId: string,
    private readonly runId: string,
  ) {}

  async expectAtTrueBottom(): Promise<void> {
    await expect.poll(
      async () => (await this.bottomMetrics()).scrollError,
      { message: "scroll offset should reach the true scrollHeight bottom" },
    )
      .toBeLessThanOrEqual(2);
    await expect.poll(
      async () => (await this.bottomMetrics()).composerInsetError,
      { message: "bottom spacer should align with the pinned composer" },
    )
      .toBeLessThanOrEqual(2);
  }

  async send(content: string): Promise<void> {
    await this.chat.composerInput.fill(content);
    await this.chat.composerInput.press("Enter");
    await expect(this.chat.composerInput).toHaveValue("");
    await expect(this.page.getByText(content, { exact: true })).toBeVisible();
  }

  async start(): Promise<void> {
    await this.emit("agent:run_started", {
      run_id: this.runId,
      conversation_id: this.conversationId,
      context_type: "project",
      context_id: PROJECT_ID,
      provider_harness: "codex",
      provider_session_id: `thread-${this.conversationId}`,
    });
  }

  async stream(text: string, sequence: number): Promise<void> {
    await this.emit("agent:chunk", {
      text,
      conversation_id: this.conversationId,
      context_type: "project",
      context_id: PROJECT_ID,
      run_id: this.runId,
      seq: sequence,
      block_index: sequence,
      append_to_previous: false,
    });
  }

  async complete(): Promise<void> {
    await this.emit("agent:run_completed", {
      run_id: this.runId,
      conversation_id: this.conversationId,
      context_type: "project",
      context_id: PROJECT_ID,
      provider_harness: "codex",
      provider_session_id: `thread-${this.conversationId}`,
    });
  }

  async finalize(content: string): Promise<void> {
    const messageId = `${this.conversationId}-final`;
    const createdAt = "2026-08-05T02:30:00.000Z";
    const contentBlocks = [{ type: "text", text: content }];
    // agent:message_created schedules a fallback refetch whenever the
    // render-ready handoff loses the race against timeline hydration. The
    // backend has already persisted the message by the time it emits, so the
    // mock store must hold it too or the refetch erases the whole turn.
    await this.persist(messageId, content, contentBlocks, createdAt);
    await this.emit("agent:message_created", {
      conversation_id: this.conversationId,
      context_type: "project",
      context_id: PROJECT_ID,
      role: "assistant",
      message_id: messageId,
      content,
      created_at: createdAt,
      render_ready: {
        message: {
          id: messageId,
          conversation_id: this.conversationId,
          role: "assistant",
          content,
          content_blocks: contentBlocks,
          created_at: createdAt,
        },
        timeline_items: [{
          id: `block:${messageId}:0`,
          conversation_id: this.conversationId,
          message_id: messageId,
          run_id: this.runId,
          sequence: 10_000,
          block_index: 0,
          role: "assistant",
          kind: "text",
          status: "finalized",
          content,
          content_blocks: contentBlocks,
          tool_call: null,
          metadata: null,
          provider_harness: "codex",
          provider_session_id: `thread-${this.conversationId}`,
          created_at: createdAt,
          updated_at: createdAt,
          finalized_at: createdAt,
        }],
      },
    });
    // agent:message_created cancels the in-flight timeline queries and then
    // invalidates them. In web mode nothing re-triggers the invalidated query
    // promptly - there is no window focus and no backend push - so the
    // persisted row reached the DOM anywhere between 0.5s and >21s. Driving the
    // refetch here stands in for that trigger and makes the handoff
    // deterministic without weakening what the assertion checks.
    await this.refetchTimeline();
    await expect(this.page.getByText(content, { exact: true }))
      .toBeVisible({ timeout: FINALIZED_MESSAGE_HANDOFF_TIMEOUT_MS });
  }

  private async refetchTimeline(): Promise<void> {
    await this.page.evaluate(async (id) => {
      await window.__queryClient?.refetchQueries({
        queryKey: ["chat", "conversations", id, "timeline"],
      });
    }, this.conversationId);
  }

  private async persist(
    messageId: string,
    content: string,
    contentBlocks: { type: string; text: string }[],
    createdAt: string,
  ): Promise<void> {
    await this.page.evaluate(async ({ blocks, id, conversationId, projectId, text, timestamp }) => {
      const chat = await import("/src/api-mock/chat");
      chat.appendMockConversationMessage(conversationId, {
        id, sessionId: null, projectId, taskId: null, role: "assistant",
        content: text, metadata: null, parentMessageId: null, conversationId,
        toolCalls: null, contentBlocks: blocks, sender: null,
        attributionSource: "provider", providerHarness: "codex",
        providerSessionId: `thread-${conversationId}`, upstreamProvider: "openai",
        providerProfile: null, logicalModel: null, effectiveModelId: null,
        logicalEffort: null, effectiveEffort: null, inputTokens: null,
        outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null,
        estimatedUsd: null, createdAt: timestamp,
      });
    }, {
      blocks: contentBlocks,
      conversationId: this.conversationId,
      id: messageId,
      projectId: PROJECT_ID,
      text: content,
      timestamp: createdAt,
    });
  }

  private async emit(event: string, payload: unknown): Promise<void> {
    await this.page.evaluate(({ eventName, eventPayload }) => {
      if (!window.__eventBus) throw new Error("Expected mock event bus");
      window.__eventBus.emit(eventName, eventPayload);
    }, { eventName: event, eventPayload: payload });
  }

  private async bottomMetrics(): Promise<{
    scrollError: number;
    composerInsetError: number;
  }> {
    const [{ clientHeight, scrollHeight, scrollTop }, spacer, chrome] = await Promise.all([
      this.chat.geometry(),
      this.chat.bottomSpacer.evaluateAll(([element]) => {
        if (!element) return null;
        const { top } = element.getBoundingClientRect();
        return { top };
      }),
      this.chat.chrome.evaluate((element) => {
        const { top } = element.getBoundingClientRect();
        return { top };
      }),
    ]);
    if (!spacer || !chrome) {
      return {
        scrollError: Number.POSITIVE_INFINITY,
        composerInsetError: Number.POSITIVE_INFINITY,
      };
    }
    return {
      scrollError: Math.abs(scrollHeight - clientHeight - scrollTop),
      composerInsetError: Math.abs(spacer.top - chrome.top),
    };
  }
}
