import { expect, type Page } from "@playwright/test";

import type { AgentsChatPage } from "./agents-chat.page";

const PROJECT_ID = "project-mock-1";

export class AgentsChatTurnPage {
  constructor(
    private readonly page: Page,
    private readonly chat: AgentsChatPage,
    private readonly conversationId: string,
    private readonly runId: string,
  ) {}

  async expectAtTrueBottom(): Promise<void> {
    await expect.poll(() => this.bottomError()).toBeLessThanOrEqual(2);
  }

  async send(content: string): Promise<void> {
    await this.chat.composerInput.fill(content);
    await this.chat.composerInput.press("Enter");
    await expect(this.chat.composerInput).toHaveValue("");
    await expect(this.page.getByText(content, { exact: true })).toBeVisible();
  }

  async returnToBottom(): Promise<void> {
    if (await this.bottomError() <= 2) return;
    await expect(this.chat.scrollToBottomButton).toBeVisible();
    await expect(this.chat.scrollToBottomButton).toBeEnabled();
    await this.chat.scrollToBottomButton.evaluate((button: HTMLButtonElement) => button.click());
    await this.expectAtTrueBottom();
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
    await expect(this.page.getByText(content, { exact: true })).toBeVisible();
  }

  private async emit(event: string, payload: unknown): Promise<void> {
    await this.page.evaluate(({ eventName, eventPayload }) => {
      if (!window.__eventBus) throw new Error("Expected mock event bus");
      window.__eventBus.emit(eventName, eventPayload);
    }, { eventName: event, eventPayload: payload });
  }

  private async bottomError(): Promise<number> {
    const [{ clientHeight, scrollHeight, scrollTop }, lastRow, typing, chrome] = await Promise.all([
      this.chat.geometry(),
      this.chat.lastRenderedRow.evaluateAll(([element]) => {
        if (!element) return null;
        const { bottom } = element.getBoundingClientRect();
        return { bottom };
      }),
      this.chat.messages.getByTestId("chat-typing-indicator").evaluateAll(([element]) => {
        if (!element) return null;
        const { bottom } = element.getBoundingClientRect();
        return { bottom };
      }),
      this.chat.chrome.evaluate((element) => {
        const { top } = element.getBoundingClientRect();
        return { top };
      }),
    ]);
    const tail = typing ?? lastRow;
    if (!tail || !chrome) return Number.POSITIVE_INFINITY;
    return Math.max(
      Math.abs(scrollHeight - clientHeight - scrollTop),
      Math.max(0, tail.bottom - chrome.top),
    );
  }
}
