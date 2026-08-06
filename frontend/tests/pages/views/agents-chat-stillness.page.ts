import type { Page } from "@playwright/test";

import { BasePage } from "../base.page";

const SPACER = '[data-testid="chat-transcript-bottom-spacer"]';

export interface TranscriptMovement {
  frames: number;
  movingFrames: number;
  maxJumpPx: number;
}

/**
 * Measures whether the rendered transcript actually holds still. The viewport
 * position of the footer spacer is the visible end of the transcript, so any
 * change between consecutive frames is content the reader saw move - whether it
 * came from a scroll write, a browser clamp, or a virtualizer relayout.
 */
export class AgentsChatStillnessPage extends BasePage {
  constructor(page: Page) {
    super(page);
  }

  async measure(frames = 40): Promise<TranscriptMovement> {
    return this.page.evaluate(async ({ frameCount, selector }) => {
      const positions: number[] = [];
      for (let index = 0; index < frameCount; index += 1) {
        await new Promise((resolve) => requestAnimationFrame(resolve));
        const spacer = document.querySelector<HTMLElement>(selector);
        if (!spacer) continue;
        positions.push(spacer.getBoundingClientRect().top);
      }
      let movingFrames = 0;
      let maxJumpPx = 0;
      for (let index = 1; index < positions.length; index += 1) {
        const delta = Math.abs(positions[index]! - positions[index - 1]!);
        if (delta <= 2) continue;
        movingFrames += 1;
        maxJumpPx = Math.max(maxJumpPx, delta);
      }
      return { frames: positions.length, movingFrames, maxJumpPx: Math.round(maxJumpPx) };
    }, { frameCount: frames, selector: SPACER });
  }
}
