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
    expect(findings[0]?.hunkStatus).toBe("ready");
    expect(findings[1]?.sourceLabel).toBe("CI check");
  });

  it("reports error status when the file diff query failed", () => {
    const findings = buildReviewWalkthroughFindings({
      files,
      hunkAnnotationsByPath: new Map([["src/second.ts", [hunk]]]),
      annotationsByPath: new Map(),
      diffByPath: new Map([["src/second.ts", "error"]]),
    });

    expect(findings[0]?.hunkStatus).toBe("error");
    expect(findings[0]?.hunk).toBeUndefined();
  });

  it("reports loading status while the file diff query is pending or unfetched", () => {
    const pending = buildReviewWalkthroughFindings({
      files,
      hunkAnnotationsByPath: new Map([["src/second.ts", [hunk]]]),
      annotationsByPath: new Map(),
      diffByPath: new Map([["src/second.ts", "loading"]]),
    });
    const unfetched = buildReviewWalkthroughFindings({
      files,
      hunkAnnotationsByPath: new Map([["src/second.ts", [hunk]]]),
      annotationsByPath: new Map(),
      diffByPath: new Map(),
    });

    expect(pending[0]?.hunkStatus).toBe("loading");
    expect(unfetched[0]?.hunkStatus).toBe("loading");
  });

  it("reports unavailable status when the diff loaded but no hunk matches the annotation", () => {
    const findings = buildReviewWalkthroughFindings({
      files,
      hunkAnnotationsByPath: new Map([["src/second.ts", [hunk]]]),
      annotationsByPath: new Map(),
      diffByPath: new Map([
        ["src/second.ts", {
          filePath: "src/second.ts", language: "ts", oldTotalLines: 9, newTotalLines: 9,
          isBinary: false, hunks: [{ oldStart: 8, oldLines: 1, newStart: 8, newLines: 1,
            header: "@@ -8,1 +8,1 @@", lines: [{ kind: "addition", content: "const other = 8;", oldLineNum: null, newLineNum: 8 }] }],
        }],
      ]),
    });

    expect(findings[0]?.hunkStatus).toBe("unavailable");
    expect(findings[0]?.hunk).toBeUndefined();
  });

  it("matches PR annotations on the annotated side only, not the opposite line numbering", () => {
    // A RIGHT-side annotation on new line 2 must not attach to a hunk whose only
    // line-2 match is an *old* line number in an unrelated part of the file.
    const findings = buildReviewWalkthroughFindings({
      files,
      hunkAnnotationsByPath: new Map(),
      annotationsByPath: new Map([["src/second.ts", [annotation]]]),
      diffByPath: new Map([
        ["src/second.ts", {
          filePath: "src/second.ts", language: "ts", oldTotalLines: 40, newTotalLines: 40,
          isBinary: false, hunks: [
            { oldStart: 2, oldLines: 1, newStart: 30, newLines: 1, header: "@@ -2,1 +30,1 @@",
              lines: [{ kind: "deletion", content: "const decoy = 2;", oldLineNum: 2, newLineNum: null }] },
            { oldStart: 20, oldLines: 1, newStart: 2, newLines: 1, header: "@@ -20,1 +2,1 @@",
              lines: [{ kind: "addition", content: "const value = 2;", oldLineNum: null, newLineNum: 2 }] },
          ],
        }],
      ]),
    });

    expect(findings[0]?.hunk?.header).toBe("@@ -20,1 +2,1 @@");
    expect(findings[0]?.hunkHeader).toBe("@@ -20,1 +2,1 @@");
  });

  it("matches LEFT-side PR annotations against old line numbers", () => {
    const findings = buildReviewWalkthroughFindings({
      files,
      hunkAnnotationsByPath: new Map(),
      annotationsByPath: new Map([
        ["src/second.ts", [{ ...annotation, side: "LEFT" as const }]],
      ]),
      diffByPath: new Map([
        ["src/second.ts", {
          filePath: "src/second.ts", language: "ts", oldTotalLines: 40, newTotalLines: 40,
          isBinary: false, hunks: [
            { oldStart: 20, oldLines: 1, newStart: 2, newLines: 1, header: "@@ -20,1 +2,1 @@",
              lines: [{ kind: "addition", content: "const value = 2;", oldLineNum: null, newLineNum: 2 }] },
            { oldStart: 2, oldLines: 1, newStart: 30, newLines: 1, header: "@@ -2,1 +30,1 @@",
              lines: [{ kind: "deletion", content: "const removed = 2;", oldLineNum: 2, newLineNum: null }] },
          ],
        }],
      ]),
    });

    expect(findings[0]?.hunk?.header).toBe("@@ -2,1 +30,1 @@");
  });
});
