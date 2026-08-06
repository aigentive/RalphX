import { describe, expect, it, vi } from "vitest";

import { createChatScrollDiagnostics } from "./diagnostics";

function createElement(): HTMLElement {
  const element = document.createElement("div");
  Object.defineProperties(element, {
    clientHeight: { configurable: true, value: 400 },
    scrollHeight: { configurable: true, value: 1_000 },
    scrollTop: { configurable: true, value: 525 },
  });
  return element;
}

describe("chat scroll diagnostics", () => {
  it("installs the development API when the module is evaluated", async () => {
    delete window.__chatScrollDiagnostics;
    vi.resetModules();

    await import("./diagnostics");

    expect(window.__chatScrollDiagnostics).toEqual(expect.objectContaining({
      start: expect.any(Function),
      read: expect.any(Function),
    }));
  });

  it("records bounded geometry only while an eligible trace is active", () => {
    let now = 10;
    const diagnostics = createChatScrollDiagnostics({
      copyText: vi.fn(),
      maxEvents: 2,
      now: () => now++,
    });
    const element = createElement();

    diagnostics.record({
      conversationId: "conversation-1",
      source: "controller",
      event: "ignored-before-start",
      element,
    });
    diagnostics.start("conversation-1");
    diagnostics.record({
      conversationId: "other-conversation",
      source: "controller",
      event: "ignored-by-filter",
      element,
    });
    diagnostics.record({
      conversationId: "conversation-1",
      source: "controller",
      event: "first",
      state: "free",
      element,
      detail: { reason: "wheel" },
    });
    diagnostics.mark("runtime-list-expanded");
    diagnostics.record({
      conversationId: "conversation-1",
      source: "layout",
      event: "latest",
      element,
    });

    expect(diagnostics.read()).toEqual([
      expect.objectContaining({
        sequence: 2,
        event: "mark:runtime-list-expanded",
        conversationId: "conversation-1",
      }),
      expect.objectContaining({
        sequence: 3,
        timestampMs: 12,
        event: "latest",
        state: null,
        geometry: {
          scrollTop: 525,
          scrollHeight: 1_000,
          clientHeight: 400,
          trueBottom: 600,
          distanceToBottom: 75,
        },
      }),
    ]);
  });

  it("copies a stable snapshot and stops accepting events after stop", async () => {
    const copyText = vi.fn().mockResolvedValue(undefined);
    const diagnostics = createChatScrollDiagnostics({
      copyText,
      now: () => 42,
    });
    diagnostics.start();
    diagnostics.record({
      conversationId: "conversation-1",
      source: "controller",
      event: "pin",
      element: null,
    });

    await expect(diagnostics.copy()).resolves.toBe(1);
    expect(JSON.parse(copyText.mock.calls[0]?.[0] as string)).toEqual(
      diagnostics.read(),
    );
    expect(diagnostics.stop()).toHaveLength(1);
    diagnostics.record({
      conversationId: "conversation-1",
      source: "controller",
      event: "ignored-after-stop",
      element: null,
    });
    expect(diagnostics.read()).toHaveLength(1);
  });
});
