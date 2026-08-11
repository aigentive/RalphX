import { describe, expect, it } from "vitest";

import {
  composerExcerptReferenceKey,
  normalizeComposerExcerptReferences,
} from "./artifactSelection.types";

describe("artifact excerpt references", () => {
  it("keeps exact internal whitespace while trimming selection edges", () => {
    expect(
      normalizeComposerExcerptReferences([
        {
          sourceKind: "plan",
          sourceId: "artifact-1",
          sourceLabel: "Plan",
          title: "Release plan",
          excerpt: "  first line\n\n  second line  ",
          version: 4,
        },
      ]),
    ).toEqual([
      {
        sourceKind: "plan",
        sourceId: "artifact-1",
        sourceLabel: "Plan",
        title: "Release plan",
        excerpt: "first line\n\n  second line",
        version: 4,
      },
    ]);
  });

  it("deduplicates the same source, revision, locator, and excerpt", () => {
    const reference = {
      sourceKind: "workspace_diff" as const,
      sourceId: "conversation-1",
      sourceLabel: "Diff",
      excerpt: "const answer = 42;",
      filePath: "src/app.ts",
      revision: "abc123",
      locator: "@@ -1,1 +1,1 @@",
    };

    expect(normalizeComposerExcerptReferences([reference, reference])).toEqual([
      reference,
    ]);
    expect(composerExcerptReferenceKey(reference)).toContain("workspace_diff");
  });

  it("rejects empty and individually oversized excerpts", () => {
    expect(
      normalizeComposerExcerptReferences([
        {
          sourceKind: "issue",
          sourceId: "issue-1",
          sourceLabel: "Issue",
          excerpt: "   ",
        },
        {
          sourceKind: "issue",
          sourceId: "issue-2",
          sourceLabel: "Issue",
          excerpt: "x".repeat(20_000),
        },
      ]),
    ).toEqual([]);
  });
});
