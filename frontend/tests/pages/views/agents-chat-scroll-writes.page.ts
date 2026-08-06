import type { Page } from "@playwright/test";

import { BasePage } from "../base.page";

const SCROLLER = '[data-chat-virtuoso-scroller="true"]';

export interface ChatScrollWrite {
  requested: number;
  before: number;
  fromController: boolean;
}

/**
 * Records every programmatic scroll write on the chat scroller together with the
 * position it moved away from and the module that issued it. Bottom follow is
 * only correct while the controller is the single scroll writer and never asks
 * the reader to move back up the transcript.
 */
export class AgentsChatScrollWritesPage extends BasePage {
  constructor(page: Page) {
    super(page);
  }

  async record(): Promise<void> {
    await this.page.evaluate((selector) => {
      const scroller = document.querySelector<HTMLElement>(selector);
      if (!scroller) throw new Error("Expected chat scroller");
      const writes: unknown[] = [];
      (window as unknown as { __chatScrollWrites: unknown[] }).__chatScrollWrites = writes;
      const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop");
      const read = (): number => descriptor?.get?.call(scroller) as number;
      const note = (requested: number, before: number): void => {
        writes.push({
          requested,
          before,
          fromController: (new Error().stack ?? "").includes("Chat/scroll/controller"),
        });
      };
      Object.defineProperty(scroller, "scrollTop", {
        configurable: true,
        get: read,
        set(next: number) {
          const before = read();
          descriptor?.set?.call(scroller, next);
          note(next, before);
        },
      });
      const scrollTo = scroller.scrollTo.bind(scroller);
      scroller.scrollTo = (options?: ScrollToOptions | number, y?: number): void => {
        const before = read();
        const requested = typeof options === "number" ? (y ?? before) : (options?.top ?? before);
        // Forward the exact arity. Passing a second argument alongside an
        // options object selects the numeric scrollTo(x, y) overload, which
        // coerces the object to 0 and scrolls the transcript to the top.
        if (typeof options === "number") {
          scrollTo(options, y as number);
        } else if (options !== undefined) {
          scrollTo(options);
        } else {
          scrollTo();
        }
        note(requested, before);
      };
    }, SCROLLER);
  }

  async writes(): Promise<ChatScrollWrite[]> {
    return this.page.evaluate(
      () => (window as unknown as { __chatScrollWrites: ChatScrollWrite[] }).__chatScrollWrites,
    );
  }
}
