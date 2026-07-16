import { describe, expect, it } from "vitest";

import { buildGranolaSelectionContent } from "./granolaSelectionContent";

describe("buildGranolaSelectionContent", () => {
  it("builds a deterministic LF-normalized note document from summary and transcript", () => {
    expect(
      buildGranolaSelectionContent({
        noteId: "not_1234567890ABCD",
        title: "  Planning sync  ",
        summaryMarkdown: "Summary first\r\nSummary second\n",
        transcript: [
          { speaker: "Alex", text: "Ship it\r\nthis week", startMs: 10 },
          { speaker: null, text: "Unattributed decision" },
          { speaker: "Sam", text: "   " },
          "malformed transcript entry",
        ],
      }),
    ).toBe(
      "# Planning sync\n\n## Summary\n\nSummary first\nSummary second\n\n## Transcript\n\nAlex: Ship it\nthis week\nUnattributed decision",
    );
  });

  it("falls back to the note id when optional content is unavailable", () => {
    expect(
      buildGranolaSelectionContent({
        noteId: "not_1234567890ABCD",
        transcript: [],
      }),
    ).toBe("# not_1234567890ABCD");
  });
});
