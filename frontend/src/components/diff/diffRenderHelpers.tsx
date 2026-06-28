import { ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { withAlpha } from "@/lib/theme-colors";
import type { DiffLine, PrDiffAnnotation } from "@/api/diff";

export type DiffRenderVariant = "standard" | "conflict";
type Variant = DiffRenderVariant;
type AnnotationSide = "old" | "new";
export type AnnotationIndex = Map<string, PrDiffAnnotation[]>;
export interface RenderDiffLineOptions {
  stickyGutter?: boolean | undefined;
}

function getLineBackground(kind: DiffLine["kind"], variant: Variant): string {
  switch (kind) {
    case "addition":
      return variant === "conflict"
        ? "var(--status-info-muted)"
        : "var(--status-success-muted)";
    case "deletion":
      return "var(--status-error-muted)";
    default:
      return "transparent";
  }
}

function getLineNumColor(kind: DiffLine["kind"], variant: Variant): string {
  switch (kind) {
    case "addition":
      return variant === "conflict"
        ? withAlpha("var(--status-info)", 60)
        : withAlpha("var(--status-success)", 60);
    case "deletion":
      return withAlpha("var(--status-error)", 60);
    default:
      return "var(--text-muted)";
  }
}

function getLinePrefix(kind: DiffLine["kind"]): string {
  switch (kind) {
    case "addition":
      return "+";
    case "deletion":
      return "-";
    default:
      return " ";
  }
}

function getPrefixColor(kind: DiffLine["kind"], variant: Variant): string {
  switch (kind) {
    case "addition":
      return variant === "conflict" ? "var(--status-info)" : "var(--status-success)";
    case "deletion":
      return "var(--status-error)";
    default:
      return "transparent";
  }
}

function annotationSide(annotation: PrDiffAnnotation): AnnotationSide {
  const side = annotation.side?.toLowerCase();
  return side === "left" || side === "old" ? "old" : "new";
}

function annotationLevelColor(level: string): string {
  switch (level.toLowerCase()) {
    case "failure":
    case "error":
    case "critical":
    case "high":
      return "var(--status-error)";
    case "medium":
    case "warning":
      return "var(--status-warning)";
    case "low":
    case "notice":
      return "var(--status-info)";
    default:
      return "var(--accent-primary)";
  }
}

function annotationSourceLabel(source: string): string {
  switch (source) {
    case "code_scanning":
      return "Code scanning";
    case "check_run":
      return "Check";
    case "review_comment":
      return "Review";
    default:
      return source.replace(/_/g, " ");
  }
}

export function buildAnnotationIndex(annotations: PrDiffAnnotation[]): AnnotationIndex {
  const index: AnnotationIndex = new Map();
  for (const annotation of annotations) {
    const startLine = annotation.startLine;
    if (startLine == null) continue;
    const endLine = annotation.endLine ?? startLine;
    const side = annotationSide(annotation);
    const boundedEnd = Math.min(endLine, startLine + 50);
    for (let line = startLine; line <= boundedEnd; line += 1) {
      const key = `${side}:${line}`;
      const existing = index.get(key);
      if (existing) {
        existing.push(annotation);
      } else {
        index.set(key, [annotation]);
      }
    }
  }
  return index;
}

export function annotationsForLine(
  index: AnnotationIndex,
  line: DiffLine
): PrDiffAnnotation[] {
  const annotations: PrDiffAnnotation[] = [];
  if (line.oldLineNum != null) {
    annotations.push(...(index.get(`old:${line.oldLineNum}`) ?? []));
  }
  if (line.newLineNum != null) {
    annotations.push(...(index.get(`new:${line.newLineNum}`) ?? []));
  }
  return [...new Map(annotations.map((annotation) => [annotation.id, annotation])).values()];
}

function renderAnnotationRows(
  annotations: PrDiffAnnotation[],
  wrapLines: boolean,
  variant: Variant
) {
  if (annotations.length === 0) return null;
  return annotations.map((annotation) => {
    const label = annotationSourceLabel(annotation.source);
    const summary = annotation.title ?? annotation.message;
    const detail =
      annotation.title && annotation.message && annotation.title !== annotation.message
        ? annotation.message
        : null;
    const annotationUrl = annotation.url;
    return (
      <div
        key={annotation.id}
        className="flex"
        data-testid="diff-annotation-row"
        style={{ backgroundColor: variant === "conflict" ? "var(--status-info-muted)" : "var(--bg-subtle)" }}
      >
        <div className="w-12 shrink-0" style={{ backgroundColor: "var(--bg-surface)" }} />
        <div className="w-12 shrink-0 border-r" style={{ backgroundColor: "var(--bg-surface)", borderColor: "var(--border-subtle)" }} />
        <div className="w-6 shrink-0" style={{ backgroundColor: "var(--bg-surface)" }} />
        <div
          className={`flex-1 min-w-0 border-l-2 px-2 py-1 text-[0.6875rem] ${
            wrapLines ? "whitespace-normal break-words" : "whitespace-nowrap"
          }`}
          style={{
            borderColor: annotationLevelColor(annotation.level),
            color: "var(--text-secondary)",
          }}
        >
          <div className="flex min-w-0 items-start gap-2">
            <div className="min-w-0 flex-1">
              <span
                className="mr-1 rounded px-1 font-semibold uppercase"
                style={{
                  backgroundColor: "var(--overlay-weak)",
                  color: annotationLevelColor(annotation.level),
                }}
              >
                {label}
              </span>
              {annotation.checkName && (
                <span className="mr-1 font-medium" style={{ color: "var(--text-primary)" }}>
                  {annotation.checkName}:
                </span>
              )}
              <span>{summary}</span>
              {annotation.isOutdated && (
                <span className="ml-1" style={{ color: "var(--text-muted)" }}>
                  outdated
                </span>
              )}
              {detail && (
                <div className="mt-0.5" style={{ color: "var(--text-muted)" }}>
                  {detail}
                </div>
              )}
            </div>
            {annotationUrl && (
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      aria-label="Open annotation in GitHub"
                      className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded border"
                      style={{
                        borderColor: "var(--border-subtle)",
                        color: "var(--text-muted)",
                      }}
                      onClick={(event) => {
                        event.stopPropagation();
                        void openUrl(annotationUrl);
                      }}
                    >
                      <ExternalLink className="h-3 w-3" />
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top">Open in GitHub</TooltipContent>
                </Tooltip>
              </TooltipProvider>
            )}
          </div>
        </div>
      </div>
    );
  });
}

export function renderDiffLine(
  line: DiffLine,
  index: number,
  wrapLines: boolean,
  variant: Variant,
  annotations: PrDiffAnnotation[] = [],
  options: RenderDiffLineOptions = {},
) {
  const stickyGutter = options.stickyGutter ?? true;
  return (
    <div key={index}>
      <div
        className="flex"
        style={{
          backgroundColor: getLineBackground(line.kind, variant),
          minHeight: "20px",
        }}
      >
        <div
          className="w-12 shrink-0 text-right pr-2 select-none z-10"
          style={{
            ...(stickyGutter ? { position: "sticky" as const, left: 0 } : {}),
            color: getLineNumColor(line.kind, variant),
            backgroundColor: "var(--bg-surface)",
          }}
        >
          {line.oldLineNum ?? ""}
        </div>

        <div
          className="w-12 shrink-0 text-right pr-2 select-none border-r z-10"
          style={{
            ...(stickyGutter ? { position: "sticky" as const, left: 48 } : {}),
            color: getLineNumColor(line.kind, variant),
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
          }}
        >
          {line.newLineNum ?? ""}
        </div>

        <div
          className="w-6 shrink-0 text-center select-none font-bold z-10"
          style={{
            ...(stickyGutter ? { position: "sticky" as const, left: 96 } : {}),
            color: getPrefixColor(line.kind, variant),
            backgroundColor: "var(--bg-surface)",
          }}
        >
          {getLinePrefix(line.kind)}
        </div>

        <div
          className={`flex-1 pr-4 min-w-0 ${
            wrapLines ? "whitespace-pre-wrap break-all" : "whitespace-pre"
          }`}
          style={{
            color:
              line.kind === "deletion"
                ? "var(--text-muted)"
                : "var(--text-secondary)",
          }}
        >
          {line.content || " "}
        </div>
      </div>
      {renderAnnotationRows(annotations, wrapLines, variant)}
    </div>
  );
}

export function renderHunkHeader(header: string) {
  return (
    <div
      className="px-3 py-1 text-[0.6875rem] font-mono"
      style={{
        backgroundColor: "var(--overlay-weak)",
        color: withAlpha("var(--text-primary)", 60),
        borderTop: "1px solid var(--overlay-weak)",
        borderBottom: "1px solid var(--overlay-weak)",
      }}
    >
      {header}
    </div>
  );
}
