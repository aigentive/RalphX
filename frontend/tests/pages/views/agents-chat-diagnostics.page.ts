import type { Page } from "@playwright/test";

import { BasePage } from "../base.page";

export interface ChatScrollDiagnosticEntry {
  event: string;
  source: "controller" | "layout" | "manual";
  state: string | null;
  detail: Record<string, unknown>;
  geometry: {
    scrollTop: number;
    scrollHeight: number;
    clientHeight: number;
    trueBottom: number;
    distanceToBottom: number;
  } | null;
}

export class AgentsChatDiagnosticsPage extends BasePage {
  constructor(page: Page) {
    super(page);
  }

  async start(): Promise<void> {
    await this.page.evaluate(() => {
      if (!window.__chatScrollDiagnostics) {
        throw new Error("Expected development chat scroll diagnostics");
      }
      window.__chatScrollDiagnostics.start();
    });
  }

  async mark(label: string): Promise<void> {
    await this.page.evaluate((nextLabel) => {
      window.__chatScrollDiagnostics?.mark(nextLabel);
    }, label);
  }

  /**
   * Non-destructive snapshot. The recorder keeps a bounded ring of 2000 entries
   * and one streaming turn can exceed that, so early layout events must be read
   * before the controller's scroll/pin volume evicts them.
   */
  async read(): Promise<ChatScrollDiagnosticEntry[]> {
    return this.page.evaluate(() => {
      if (!window.__chatScrollDiagnostics) {
        throw new Error("Expected development chat scroll diagnostics");
      }
      return window.__chatScrollDiagnostics.read();
    });
  }

  async stop(): Promise<ChatScrollDiagnosticEntry[]> {
    return this.page.evaluate(() => {
      if (!window.__chatScrollDiagnostics) {
        throw new Error("Expected development chat scroll diagnostics");
      }
      return window.__chatScrollDiagnostics.stop();
    });
  }
}
