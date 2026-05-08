/**
 * ConflictDiffViewer - Chunk-based conflict diff using SimpleDiffView
 *
 * Renders ours vs theirs content as a unified diff with:
 * - Deletions (red) = lines only in ours (current branch)
 * - Additions (blue) = lines only in theirs (incoming branch)
 * - Context = shared between both
 */

import type { ConflictDiff } from "@/hooks/useConflictDiff";
import { SimpleDiffView } from "./SimpleDiffView";

interface ConflictDiffViewerProps {
  /** Conflict diff data from useConflictDiff hook */
  conflictDiff: ConflictDiff;
}

/**
 * Get file extension for language badge display
 */
function getFileExtension(filePath: string): string {
  const parts = filePath.split(".");
  if (parts.length > 1) {
    return parts[parts.length - 1] ?? "";
  }
  return "";
}

export function ConflictDiffViewer({ conflictDiff }: ConflictDiffViewerProps) {
  const { filePath, oursContent, theirsContent, language } = conflictDiff;

  const displayLanguage = language || getFileExtension(filePath);

  return (
    <div className="h-full flex flex-col">
      <div
        className="font-mono text-[0.8125rem] leading-[20px]"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        {/* Header with file path and language badge */}
        <div
          className="flex items-center justify-between px-3 py-2 border-b"
          style={{ borderColor: "var(--overlay-weak)" }}
        >
          <span
            className="text-sm truncate"
            style={{ color: "var(--text-secondary)" }}
          >
            {filePath}
          </span>
          {displayLanguage && (
            <span
              className="text-[0.6875rem] px-2 py-0.5 rounded ml-2 shrink-0"
              style={{
                backgroundColor: "var(--overlay-weak)",
                color: "var(--text-muted)",
              }}
            >
              {displayLanguage}
            </span>
          )}
        </div>

        {/* Conflict legend */}
        <div
          className="flex items-center gap-4 px-3 py-1.5 text-[0.6875rem]"
          style={{
            backgroundColor: "var(--overlay-faint)",
            borderBottom: "1px solid var(--overlay-faint)",
          }}
        >
          <span className="flex items-center gap-1.5">
            <span
              className="w-3 h-3 rounded"
              style={{ backgroundColor: "var(--status-error-muted)" }}
            />
            <span style={{ color: "var(--status-error)" }}>-</span>
            <span style={{ color: "var(--text-muted)" }}>Ours (current)</span>
          </span>
          <span className="flex items-center gap-1.5">
            <span
              className="w-3 h-3 rounded"
              style={{ backgroundColor: "var(--status-info-muted)" }}
            />
            <span style={{ color: "var(--status-info)" }}>+</span>
            <span style={{ color: "var(--text-muted)" }}>Theirs (incoming)</span>
          </span>
        </div>
      </div>

      {/* Diff content via SimpleDiffView */}
      <div className="flex-1 min-h-0">
        <SimpleDiffView
          oldContent={oursContent ?? ""}
          newContent={theirsContent ?? ""}
          language={displayLanguage}
          variant="conflict"
        />
      </div>
    </div>
  );
}
