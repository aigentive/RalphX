/**
 * CloneProgress - presentational clone progress display.
 *
 * Renders the current phase as a plain-language label, a determinate bar
 * when percent is known, an indeterminate bar otherwise (with an extra note
 * during the long, percent-less "checking out" phase), a collapsible raw
 * console fed from the tail of stderr lines, and the cancel control.
 */

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { ChevronDown, Loader2, Terminal } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ClonePhase } from "@/types/clone";

const PHASE_LABELS: Record<ClonePhase, string> = {
  connecting: "Connecting to the repository...",
  counting: "Counting objects...",
  compressing: "Compressing objects...",
  receiving: "Receiving objects...",
  resolving: "Resolving deltas...",
  checking_out: "Checking out files...",
};

export interface CloneProgressProps {
  phase: ClonePhase | null;
  percent: number | null;
  received: number | null;
  total: number | null;
  lines: string[];
  onCancel: () => void;
  isCancelling?: boolean;
}

export function CloneProgress({
  phase,
  percent,
  received,
  total,
  lines,
  onCancel,
  isCancelling = false,
}: CloneProgressProps) {
  const [showConsole, setShowConsole] = useState(false);
  const clampedPercent = percent === null ? null : Math.min(100, Math.max(0, percent));
  const isDeterminate = clampedPercent !== null;
  const isCheckingOut = phase === "checking_out";

  return (
    <div data-testid="clone-progress" className="space-y-3">
      <div className="flex items-center gap-2 text-sm text-[var(--text-primary)]">
        <Loader2 className="h-4 w-4 animate-spin text-[var(--accent-primary)]" />
        <span>{phase ? PHASE_LABELS[phase] : "Preparing to clone..."}</span>
        {isDeterminate && (
          <span className="ml-auto text-xs text-[var(--text-muted)]">{clampedPercent}%</span>
        )}
      </div>

      <div
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={isDeterminate ? clampedPercent : undefined}
        className="h-2 w-full overflow-hidden rounded-full bg-[var(--bg-base)]"
      >
        {isDeterminate ? (
          <div
            className="h-full rounded-full bg-[var(--accent-primary)] transition-[width] duration-200"
            style={{ width: `${clampedPercent}%` }}
          />
        ) : (
          <div className="h-full w-1/3 animate-pulse rounded-full bg-[var(--accent-primary)]" />
        )}
      </div>

      {received !== null && total !== null && total > 0 && (
        <p className="text-xs text-[var(--text-muted)]">
          {received.toLocaleString()} / {total.toLocaleString()} objects
        </p>
      )}

      {isCheckingOut && (
        <p className="text-xs text-[var(--text-muted)]">
          This can take a while for large repositories.
        </p>
      )}

      <Collapsible open={showConsole} onOpenChange={setShowConsole}>
        <CollapsibleTrigger
          data-testid="clone-console-trigger"
          className="flex items-center gap-2 text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors"
        >
          <Terminal className="h-3 w-3" />
          <span>Raw output</span>
          <ChevronDown className={cn("h-3 w-3 transition-transform", showConsole && "rotate-180")} />
        </CollapsibleTrigger>
        <CollapsibleContent className="mt-2">
          <pre
            data-testid="clone-console-output"
            className="max-h-40 overflow-y-auto rounded-lg bg-[var(--bg-base)] px-3 py-2 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-all"
          >
            {lines.length > 0 ? lines.join("\n") : "Waiting for output..."}
          </pre>
        </CollapsibleContent>
      </Collapsible>

      <Button
        data-testid="clone-cancel-button"
        type="button"
        onClick={onCancel}
        disabled={isCancelling}
        variant="ghost"
        className="w-full bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
      >
        {isCancelling && <Loader2 className="h-4 w-4 animate-spin" />}
        {isCancelling ? "Cancelling..." : "Cancel Clone"}
      </Button>
    </div>
  );
}
