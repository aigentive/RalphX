import type { Page } from "@playwright/test";

import { BasePage } from "../base.page";

const SCROLLER = '[data-chat-virtuoso-scroller="true"]';

/**
 * Injects the bistable trailing measurement captured in production scroll
 * traces: reaching the bottom drops a tail gap, and the next frame republishes
 * it. React, Virtuoso, rAF, and the scroll controller all stay real — only the
 * virtualizer's estimate/measure flip is faked, because it depends on row
 * height, render-window boundaries, and sub-pixel rounding we cannot seed.
 */
export class AgentsChatBistablePage extends BasePage {
  constructor(page: Page) {
    super(page);
  }

  async install(gapPx = 20): Promise<void> {
    await this.page.evaluate(({ selector, gap }) => {
      const scroller = document.querySelector<HTMLElement>(selector);
      if (!scroller) throw new Error("Expected chat scroller");
      const tail = document.createElement("div");
      tail.dataset.bistableTail = "true";
      tail.setAttribute("aria-hidden", "true");
      tail.style.flex = "none";
      tail.style.height = `${gap}px`;
      scroller.append(tail);
      let expanded = true;
      let restoreFrame: number | null = null;
      const setExpanded = (next: boolean): void => {
        expanded = next;
        tail.style.height = next ? `${gap}px` : "0px";
      };
      scroller.addEventListener("scroll", () => {
        if (!expanded) return;
        if (scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop > 1) return;
        setExpanded(false);
        if (restoreFrame !== null) return;
        restoreFrame = requestAnimationFrame(() => {
          restoreFrame = null;
          setExpanded(true);
        });
      }, { passive: true });
    }, { gap: gapPx, selector: SCROLLER });
  }

  async distanceToBottom(): Promise<number> {
    return this.page.locator(SCROLLER).evaluate(
      (element) => element.scrollHeight - element.clientHeight - element.scrollTop,
    );
  }
}
