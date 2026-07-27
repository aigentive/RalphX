import { annotationSourceLabel } from "@/components/diff/diffRenderHelpers";
import type {
  DiffHunk,
  FileChange,
  FileDiff,
  PrDiffAnnotation,
  WorkspaceReviewHunkAnnotation,
} from "@/api/diff";
import type { ReviewWalkthroughFinding } from "./ReviewWalkthrough";

interface BuildReviewWalkthroughFindingsOptions {
  files: FileChange[];
  annotationsByPath: Map<string, PrDiffAnnotation[]>;
  hunkAnnotationsByPath: Map<string, WorkspaceReviewHunkAnnotation[]>;
  diffByPath: Map<string, FileDiff | "loading" | "error" | undefined>;
}

function loadedDiff(
  diff: FileDiff | "loading" | "error" | undefined,
): FileDiff | undefined {
  return diff !== "loading" && diff !== "error" ? diff : undefined;
}

function hunkForWorkspaceAnnotation(
  diff: FileDiff | undefined,
  annotation: WorkspaceReviewHunkAnnotation,
): DiffHunk | undefined {
  return diff?.hunks.find(
    (hunk) =>
      hunk.header === annotation.hunkHeader &&
      hunk.oldStart === annotation.oldStart &&
      hunk.oldLines === annotation.oldLines &&
      hunk.newStart === annotation.newStart &&
      hunk.newLines === annotation.newLines,
  );
}

function hunkForPrAnnotation(
  diff: FileDiff | undefined,
  annotation: PrDiffAnnotation,
): DiffHunk | undefined {
  const startLine = annotation.startLine;
  if (startLine === null) return undefined;
  const endLine = annotation.endLine ?? startLine;
  return diff?.hunks.find((hunk) =>
    hunk.lines.some(
      (line) =>
        (line.newLineNum !== null && line.newLineNum >= startLine && line.newLineNum <= endLine) ||
        (line.oldLineNum !== null && line.oldLineNum >= startLine && line.oldLineNum <= endLine),
    ),
  );
}

export function buildReviewWalkthroughFindings({
  files,
  annotationsByPath,
  hunkAnnotationsByPath,
  diffByPath,
}: BuildReviewWalkthroughFindingsOptions): ReviewWalkthroughFinding[] {
  const findings: ReviewWalkthroughFinding[] = [];
  for (const file of files) {
    const diff = loadedDiff(diffByPath.get(file.path));
    for (const annotation of hunkAnnotationsByPath.get(file.path) ?? []) {
      const hunk = hunkForWorkspaceAnnotation(diff, annotation);
      findings.push({
        id: `workspace:${annotation.id}`,
        path: annotation.path,
        hunkHeader: annotation.hunkHeader,
        title: annotation.title ?? "Workspace review finding",
        message: annotation.message,
        level: annotation.level,
        sourceLabel: "Workspace review",
        ...(hunk !== undefined && { hunk }),
      });
    }
    for (const annotation of annotationsByPath.get(file.path) ?? []) {
      const hunk = hunkForPrAnnotation(diff, annotation);
      findings.push({
        id: `pr:${annotation.id}`,
        path: file.path,
        hunkHeader: hunk?.header ?? "Referenced lines",
        title: annotation.title ?? "Pull request finding",
        message: annotation.message,
        level: annotation.level,
        sourceLabel: annotation.checkName ?? annotationSourceLabel(annotation.source),
        ...(hunk !== undefined && { hunk }),
      });
    }
  }
  return findings;
}
