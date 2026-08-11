import { useEffect, useMemo, useState } from "react";

import {
  computeDiff,
  getLineBackground,
  getLinePrefix,
  getPrefixColor,
} from "@/components/Chat/DiffToolCallView.utils";

export interface PersonaContentDiffProps {
  oldContent: string;
  newContent: string;
  ariaLabel: string;
}

/**
 * Read-only line diff between two persona content revisions. Renders a cheap
 * shell first and computes the diff after a paint boundary (perf rule).
 */
export function PersonaContentDiff({
  oldContent,
  newContent,
  ariaLabel,
}: PersonaContentDiffProps) {
  const [hydrated, setHydrated] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const frame = requestAnimationFrame(() => {
      setTimeout(() => {
        if (!cancelled) setHydrated(true);
      }, 0);
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, []);

  const lines = useMemo(
    () => (hydrated ? computeDiff(oldContent, newContent) : null),
    [hydrated, oldContent, newContent],
  );

  if (!lines) {
    return (
      <div
        aria-label={ariaLabel}
        data-testid="persona-diff-loading"
        className="min-h-24 animate-pulse rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]"
      />
    );
  }

  if (lines.every((line) => line.kind === "context")) {
    return (
      <p
        aria-label={ariaLabel}
        className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-xs text-[var(--text-muted)]"
      >
        No changes.
      </p>
    );
  }

  return (
    <div
      role="region"
      aria-label={ariaLabel}
      data-testid="persona-diff"
      className="overflow-x-auto rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] py-1 font-mono text-xs leading-relaxed"
    >
      {lines.map((line, index) => (
        <div
          key={index}
          className="flex px-2"
          style={{ backgroundColor: getLineBackground(line.kind) }}
        >
          <span
            aria-hidden="true"
            className="w-4 shrink-0 select-none"
            style={{ color: getPrefixColor(line.kind) }}
          >
            {getLinePrefix(line.kind)}
          </span>
          <span className="whitespace-pre-wrap break-all text-[var(--text-primary)]">
            {line.content}
          </span>
        </div>
      ))}
    </div>
  );
}
