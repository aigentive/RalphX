import { describe, expect, it } from "vitest";

import { formatQueuedMessageExcerpt } from "./queuedMessageExcerpt";

describe("formatQueuedMessageExcerpt", () => {
  it("keeps short queued messages unchanged", () => {
    expect(formatQueuedMessageExcerpt("Short queued prompt")).toBe(
      "Short queued prompt"
    );
  });

  it("compacts whitespace and excerpts long queued messages", () => {
    const content = `Start\n${"A".repeat(40)}\nEnd`;

    expect(formatQueuedMessageExcerpt(content, 24)).toBe(
      `Start ${"A".repeat(15)}...`
    );
  });
});
