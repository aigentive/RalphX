import { expect } from "@playwright/test";

import type { ChatScrollDiagnosticEntry } from "../pages/views/agents-chat-diagnostics.page";

/**
 * Bottom pins that never advance past the highest true bottom already published
 * are a write/measure fight with the virtualizer, not progress. This catches
 * both shapes seen in production traces: a repeated identical target, and a
 * rotating cycle of previously published targets.
 */
export function longestNoProgressPinRun(
  trace: ChatScrollDiagnosticEntry[],
): { run: number; highWater: number | null } {
  let longest = 0;
  let longestHighWater: number | null = null;
  let current = 0;
  let highWater: number | null = null;
  for (const { source, event, detail } of trace) {
    if (source !== "controller" || event !== "pin") continue;
    const target = typeof detail.target === "number" ? detail.target : null;
    if (target === null) continue;
    if (highWater === null || target > highWater) {
      highWater = target;
      current = 0;
      continue;
    }
    current += 1;
    if (current <= longest) continue;
    longest = current;
    longestHighWater = highWater;
  }
  return { run: longest, highWater: longestHighWater };
}

export function expectNoPinChurn(
  trace: ChatScrollDiagnosticEntry[],
  limit = 6,
): void {
  const { run, highWater } = longestNoProgressPinRun(trace);
  expect(run, `${run} bottom pins in a row never advanced past ${String(highWater)}`)
    .toBeLessThan(limit);
}
