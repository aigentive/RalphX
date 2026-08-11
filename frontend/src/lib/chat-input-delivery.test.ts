import { describe, expect, it } from "vitest";
import { resolveChatInputDelivery } from "./chat-input-delivery";

describe("resolveChatInputDelivery", () => {
  it.each([
    ["claude", "interactive"],
    ["codex", "queued"],
    [null, "unknown"],
    ["future-harness", "unknown"],
  ] as const)("resolves %s as %s", (harness, expectedDelivery) => {
    expect(resolveChatInputDelivery(harness)).toBe(expectedDelivery);
  });
});
