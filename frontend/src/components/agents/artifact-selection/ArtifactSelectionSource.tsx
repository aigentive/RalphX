import { useEffect, useMemo, useState } from "react";
import { Check, ListTree, X } from "lucide-react";

import type { ComposerSelectionSnapshot } from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  selectArtifactSelection,
  useArtifactSelectionStore,
} from "@/stores/artifactSelectionStore";
import { cn } from "@/lib/utils";

const MAX_SELECTION_BYTES = 64 * 1024;

export type ArtifactSelectionSourceDescriptor = Omit<
  ComposerSelectionSnapshot,
  "startLine" | "endLine" | "content"
>;

interface ArtifactSelectionSourceProps {
  conversationId: string | null;
  source: ArtifactSelectionSourceDescriptor;
  content: string;
  className?: string;
}

export function ArtifactSelectionSource({
  conversationId,
  source,
  content,
  className,
}: ArtifactSelectionSourceProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [viewerReady, setViewerReady] = useState(false);
  const [anchorLine, setAnchorLine] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const selection = useArtifactSelectionStore(
    selectArtifactSelection(conversationId),
  );
  const setSelection = useArtifactSelectionStore((state) => state.setSelection);
  const clearSelection = useArtifactSelectionStore(
    (state) => state.clearSelection,
  );
  const normalizedContent = useMemo(
    () => content.replace(/\r\n?/g, "\n").replace(/\n+$/, ""),
    [content],
  );
  const lines = useMemo(() => normalizedContent.split("\n"), [normalizedContent]);
  const selectionMatchesSource = Boolean(
    selection &&
      selection.sourceType === source.sourceType &&
      selection.sourceKind === source.sourceKind &&
      selection.sourceId === source.sourceId &&
      selection.artifactVersion === source.artifactVersion &&
      selection.sourceRevision === source.sourceRevision,
  );

  useEffect(() => {
    if (!isOpen) {
      setViewerReady(false);
      return;
    }
    const frame = window.requestAnimationFrame(() => setViewerReady(true));
    return () => window.cancelAnimationFrame(frame);
  }, [isOpen]);

  const commitLine = (lineNumber: number, extend: boolean) => {
    if (!conversationId) return;
    const baseLine =
      extend && anchorLine && selectionMatchesSource ? anchorLine : lineNumber;
    const startLine = Math.min(baseLine, lineNumber);
    const endLine = Math.max(baseLine, lineNumber);
    const selectedContent = lines.slice(startLine - 1, endLine).join("\n");
    if (new TextEncoder().encode(selectedContent).byteLength > MAX_SELECTION_BYTES) {
      setError("Select a smaller range (64 KiB maximum).");
      return;
    }
    setError(null);
    setAnchorLine(baseLine);
    setSelection(conversationId, {
      ...source,
      startLine,
      endLine,
      content: selectedContent,
    });
  };

  const sourceLabel = source.sourceType === "artifact" ? "plan" : "ticket";
  const selectedLineLabel =
    selectionMatchesSource && selection
      ? selection.startLine === selection.endLine
        ? `L${selection.startLine}`
        : `L${selection.startLine}–${selection.endLine}`
      : null;

  return (
    <section
      className={cn("border-b", className)}
      style={{
        backgroundColor: "var(--bg-surface)",
        borderBottomColor: "var(--border-subtle)",
        borderBottomStyle: "solid",
        borderBottomWidth: 1,
      }}
      data-testid="artifact-selection-source"
    >
      <div className="flex min-h-11 items-center gap-2 px-4 py-2">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="gap-2"
          aria-expanded={isOpen}
          onClick={() => setIsOpen((open) => !open)}
          disabled={!conversationId}
        >
          <ListTree className="h-4 w-4" />
          {isOpen ? "Close line selection" : `Select ${sourceLabel} lines`}
        </Button>
        {selectedLineLabel ? (
          <span
            className="ml-auto inline-flex items-center gap-1 text-xs font-medium"
            style={{ color: "var(--accent-primary)" }}
          >
            <Check className="h-3.5 w-3.5" />
            Selected {selectedLineLabel}
          </span>
        ) : null}
        {selectionMatchesSource && conversationId ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="gap-1.5"
            onClick={() => clearSelection(conversationId)}
          >
            <X className="h-3.5 w-3.5" />
            Clear
          </Button>
        ) : null}
      </div>

      {isOpen ? (
        <div
          className="max-h-[42vh] overflow-auto border-t px-2 py-2"
          style={{
            backgroundColor: "var(--bg-base)",
            borderTopColor: "var(--border-subtle)",
            borderTopStyle: "solid",
            borderTopWidth: 1,
          }}
        >
          {!viewerReady ? (
            <div
              className="px-3 py-4 text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              Preparing line selection…
            </div>
          ) : (
            <div aria-label={`${sourceLabel} lines`}>
              {lines.map((line, index) => {
                const lineNumber = index + 1;
                const selected = Boolean(
                  selectionMatchesSource &&
                    selection &&
                    lineNumber >= selection.startLine &&
                    lineNumber <= selection.endLine,
                );
                return (
                  <button
                    key={lineNumber}
                    type="button"
                    aria-label={`Line ${lineNumber}: ${line || "Blank line"}`}
                    className="group flex w-full min-w-max items-start rounded px-1 py-0.5 text-left font-mono text-xs leading-5"
                    style={{
                      backgroundColor: selected
                        ? "var(--accent-muted)"
                        : "var(--bg-base)",
                      color: "var(--text-primary)",
                    }}
                    onClick={(event) => commitLine(lineNumber, event.shiftKey)}
                  >
                    <span
                      className="mr-3 w-10 shrink-0 select-none text-right tabular-nums"
                      style={{ color: "var(--text-muted)" }}
                    >
                      {lineNumber}
                    </span>
                    <span className="whitespace-pre">{line || " "}</span>
                  </button>
                );
              })}
            </div>
          )}
          <p className="px-2 pt-2 text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
            Click a line, then Shift-click another line to select a range.
          </p>
          {error ? (
            <p className="px-2 pt-1 text-xs" role="alert" style={{ color: "var(--status-error)" }}>
              {error}
            </p>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
