import {
  annotationSourceLabel,
  annotationsForLine,
  buildAnnotationIndex,
} from "@/components/diff/diffRenderHelpers";
import type {
  DiffHunk,
  FileChange,
  FileDiff,
  PrDiffAnnotation,
  WorkspaceReviewHunkAnnotation,
} from "@/api/diff";
import { requiresExplicitDiffHydration } from "./inlineDiffGuards";
import type {
  ReviewWalkthroughFinding,
  ReviewWalkthroughHunkStatus,
} from "./ReviewWalkthrough";

type DiffState = FileDiff | "loading" | "error" | undefined;

interface BuildReviewWalkthroughFindingsOptions {
  files: FileChange[];
  annotationsByPath: Map<string, PrDiffAnnotation[]>;
  hunkAnnotationsByPath: Map<string, WorkspaceReviewHunkAnnotation[]>;
  diffByPath: Map<string, DiffState>;
  /** Paths the user explicitly opted into loading despite the generated-file gate. */
  showAnywayPaths?: Set<string>;
}

function loadedDiff(diff: DiffState): FileDiff | undefined {
  return diff !== "loading" && diff !== "error" ? diff : undefined;
}

/**
 * Distinguishes a failed diff fetch from a pending one, and a successfully
 * loaded diff whose hunks no longer contain the annotated range from either.
 * Collapsing these into "no hunk" strands the walkthrough on loading copy.
 */
function hunkStatusFor(
  diff: DiffState,
  hunk: DiffHunk | undefined,
  isHydrationBlocked: boolean,
): ReviewWalkthroughHunkStatus {
  if (diff === "error") return "error";
  if (hunk !== undefined) return "ready";
  if (isHydrationBlocked) return "blocked";
  if (diff === "loading" || diff === undefined) return "loading";
  return "unavailable";
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

/**
 * Reuses the same side-keyed index the inline diff views use, so the walkthrough
 * attaches exactly the hunk that would carry this annotation inline. Matching on
 * either line numbering would attach the wrong hunk when old/new numbering diverges.
 */
function hunkForPrAnnotation(
  diff: FileDiff | undefined,
  annotation: PrDiffAnnotation,
): DiffHunk | undefined {
  if (annotation.startLine === null) return undefined;
  const index = buildAnnotationIndex([annotation]);
  return diff?.hunks.find((hunk) =>
    hunk.lines.some((line) => annotationsForLine(index, line).length > 0),
  );
}

export function buildReviewWalkthroughFindings({
  files,
  annotationsByPath,
  hunkAnnotationsByPath,
  diffByPath,
  showAnywayPaths,
}: BuildReviewWalkthroughFindingsOptions): ReviewWalkthroughFinding[] {
  const findings: ReviewWalkthroughFinding[] = [];
  for (const file of files) {
    const diffState = diffByPath.get(file.path);
    const diff = loadedDiff(diffState);
    const isHydrationBlocked =
      requiresExplicitDiffHydration(file) &&
      !(showAnywayPaths?.has(file.path) ?? false);
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
        hunkStatus: hunkStatusFor(diffState, hunk, isHydrationBlocked),
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
        hunkStatus: hunkStatusFor(diffState, hunk, isHydrationBlocked),
        ...(hunk !== undefined && { hunk }),
      });
    }
  }
  return findings;
}
