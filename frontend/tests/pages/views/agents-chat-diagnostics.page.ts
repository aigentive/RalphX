import type { Page } from "@playwright/test";

import { BasePage } from "../base.page";

export interface ChatScrollDiagnosticEntry {
  event: string;
  source: "controller" | "layout" | "manual";
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

  async stop(): Promise<ChatScrollDiagnosticEntry[]> {
    return this.page.evaluate(() => {
      if (!window.__chatScrollDiagnostics) {
        throw new Error("Expected development chat scroll diagnostics");
      }
      return window.__chatScrollDiagnostics.stop();
    });
  }
}
