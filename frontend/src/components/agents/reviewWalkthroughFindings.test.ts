import { describe, expect, it } from "vitest";
import type { FileChange, PrDiffAnnotation, WorkspaceReviewHunkAnnotation } from "@/api/diff";
import { buildReviewWalkthroughFindings } from "./reviewWalkthroughFindings";

const files: FileChange[] = [
  { path: "src/second.ts", status: "modified", additions: 1, deletions: 1, isGenerated: false },
  { path: "src/first.ts", status: "modified", additions: 1, deletions: 1, isGenerated: false },
];

const hunk: WorkspaceReviewHunkAnnotation = {
  id: "hunk-1", conversationId: "c", projectId: "p", artifactId: "a", artifactVersion: 1,
  targetScope: "workspace_delta", headSha: null, diffFingerprint: "f", path: "src/second.ts",
  diffSource: "selected_source", hunkHeader: "@@ -2,1 +2,1 @@", oldStart: 2, oldLines: 1,
  newStart: 2, newLines: 1, title: "Hunk finding", message: "Attached to the hunk.",
  level: "warning", createdByRunId: null, createdAt: "2026-01-01T00:00:00Z",
};

const annotation: PrDiffAnnotation = {
  id: "pr-1", source: "check_run", path: "src/second.ts", side: "RIGHT", startLine: 2,
  endLine: 2, startColumn: null, endColumn: null, level: "failure", status: null,
  title: "PR finding", message: "Attached by line.", author: null, checkName: "CI check", url: null,
  isOutdated: false, createdAt: null,
};

describe("buildReviewWalkthroughFindings", () => {
  it("uses file order, puts hunk findings before PR findings within a file, and attaches matching hunks", () => {
    const findings = buildReviewWalkthroughFindings({
      files,
      hunkAnnotationsByPath: new Map([["src/second.ts", [hunk]]]),
      annotationsByPath: new Map([["src/second.ts", [annotation]]]),
      diffByPath: new Map([
        ["src/second.ts", {
          filePath: "src/second.ts", language: "ts", oldTotalLines: 2, newTotalLines: 2,
          isBinary: false, hunks: [{ oldStart: 2, oldLines: 1, newStart: 2, newLines: 1,
            header: "@@ -2,1 +2,1 @@", lines: [{ kind: "addition", content: "const value = 2;", oldLineNum: null, newLineNum: 2 }] }],
        }],
      ]),
    });

    expect(findings.map((finding) => finding.id)).toEqual(["workspace:hunk-1", "pr:pr-1"]);
    expect(findings[0]?.hunk?.header).toBe("@@ -2,1 +2,1 @@");
    expect(findings[1]?.sourceLabel).toBe("CI check");
  });
});
