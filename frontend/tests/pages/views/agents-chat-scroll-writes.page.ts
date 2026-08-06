import { expect, type Page } from "@playwright/test";

import { BasePage } from "../base.page";

const SCROLLER = '[data-chat-virtuoso-scroller="true"]';
const VIEWPORT_HEIGHT_PX = 900;
// Virtuoso owns bottom follow now, so an upward write is its own actuator
// rather than a rewind of the reader: when its size tree resolves shorter than
// the live scroller extent it lands the last item's end above the current
// position, then re-descends. Over 50 recorded runs of the two specs below,
// 18 produced none at all, most of the rest produced a single 91px correction,
// and the largest was 195px. These are mid-settle intermediates, so the useful
// gates are the exact one (the controller writes nothing), the cumulative one
// (a bounded correction repeating every frame is still jitter), and the
// end-state one the specs assert directly. The per-write ceiling is only a
// tripwire for the regression class this work removed, whose writes were of a
// different order: 373px with the settle guard, 473px for a single corrective
// controller write, ~1150px unguarded.
const MAX_SETTLE_REWIND_PX = VIEWPORT_HEIGHT_PX / 2;
const MAX_TOTAL_REWIND_PX = VIEWPORT_HEIGHT_PX;

export interface ChatScrollWrite {
  requested: number;
  before: number;
  fromController: boolean;
}

/**
 * Records every programmatic scroll write on the chat scroller together with the
 * position it moved away from and the module that issued it. Bottom follow is
 * only correct while Virtuoso is the single scroll writer and never walks the
 * reader back up the transcript.
 */
export class AgentsChatScrollWritesPage extends BasePage {
  constructor(page: Page) {
    super(page);
  }

  async record(): Promise<void> {
    await this.page.evaluate((selector) => {
      const scroller = document.querySelector<HTMLElement>(selector);
      if (!scroller) throw new Error("Expected chat scroller");
      const writes: { requested: number; before: number; origin: Error }[] = [];
      (window as unknown as { __chatScrollWrites: unknown[] }).__chatScrollWrites = writes;
      const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop");
      const read = (): number => descriptor?.get?.call(scroller) as number;
      // This recorder sits inside Virtuoso's scroll path, so it has to stay
      // cheap enough not to change what it measures. Reading `.stack` here
      // symbolizes the trace on every write, and that cost alone pushed the
      // settled transcript 203-411px off the bottom and produced 311-350px
      // corrections that never occur without the recorder installed. Keep the
      // unformatted Error and resolve attribution in `writes()` instead, and
      // bound collection depth: a controller-issued write puts the controller
      // frame within the first few frames.
      const previousStackLimit = Error.stackTraceLimit;
      Error.stackTraceLimit = 6;
      (window as unknown as { __restoreChatScrollStackLimit: () => void })
        .__restoreChatScrollStackLimit = () => {
          Error.stackTraceLimit = previousStackLimit;
        };
      const note = (requested: number, before: number): void => {
        writes.push({ requested, before, origin: new Error() });
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

  /** Resolves deferred write attribution once the transcript has settled. */
  async writes(): Promise<ChatScrollWrite[]> {
    return this.page.evaluate(() => {
      const recorded = (window as unknown as {
        __chatScrollWrites: { requested: number; before: number; origin: Error }[];
        __restoreChatScrollStackLimit?: () => void;
      }).__chatScrollWrites;
      (window as unknown as { __restoreChatScrollStackLimit?: () => void })
        .__restoreChatScrollStackLimit?.();
      return recorded.map(({ requested, before, origin }) => ({
        requested,
        before,
        fromController: (origin.stack ?? "").includes("Chat/scroll/controller"),
      }));
    });
  }

  /**
   * The bottom-follow contract: the controller issues no raw scroll write at
   * all, and Virtuoso's own writes never walk the reader up the transcript by
   * an amount they could perceive, individually or cumulatively.
   */
  async expectSingleWriterNoRewind(): Promise<void> {
    const recorded = await this.writes();
    expect(recorded.length).toBeGreaterThan(0);
    // One measured corrective write under followOutput took scrollError from
    // 119 to 228 with a 473px jump. The controller writes no scrollTop at all.
    expect(recorded.filter(({ fromController }) => fromController)).toEqual([]);

    const rewinds = recorded
      .filter(({ requested, before }) => requested < before)
      .map(({ requested, before }) => before - requested);
    const describe = `upward writes: [${rewinds.join(", ")}]px`;
    expect(Math.max(0, ...rewinds), describe).toBeLessThanOrEqual(MAX_SETTLE_REWIND_PX);
    expect(rewinds.reduce((total, px) => total + px, 0), describe)
      .toBeLessThanOrEqual(MAX_TOTAL_REWIND_PX);
  }
}
