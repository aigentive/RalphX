/**
 * MergePhaseTimeline - Shows high-level merge progress through phases
 *
 * Renders a linear timeline of merge phases with status indicators:
 * - started → spinner (in progress)
 * - passed → green check (success)
 * - failed → red X (error)
 * - pending → gray circle (not started)
 *
 * Supports dynamic phase lists derived from project analysis.
 * Falls back to a hardcoded default if no dynamic list is provided.
 */

import {
  Loader2,
  CheckCircle2,
  XCircle,
  SkipForward,
} from "lucide-react";
import { SectionTitle } from "./shared";
import { withAlpha } from "@/lib/theme-colors";
import type { MergeProgressEvent, MergePhaseInfo } from "@/types/events";

/** Default phase config — used as fallback when no dynamic phase list is received */
const DEFAULT_PHASE_CONFIG: MergePhaseInfo[] = [
  { id: "merge_preparation", label: "Preparation", description: "Loading task context and resolving branches" },
  { id: "precondition_check", label: "Preconditions", description: "Validating merge prerequisites and dependencies" },
  { id: "branch_freshness", label: "Branch Freshness", description: "Checking source branch is up-to-date with target" },
  { id: "merge_cleanup", label: "Cleanup", description: "Removing stale worktrees and stopping old agents" },
  { id: "worktree_setup", label: "Worktree Setup", description: "Creating isolated worktree for validation" },
  { id: "programmatic_merge", label: "Merge", description: "Running git merge/rebase operation" },
  { id: "npm_run_typecheck", label: "Type Check", command: "npm run typecheck" },
  { id: "npm_run_lint", label: "Lint", command: "npm run lint" },
  { id: "cargo_clippy", label: "Clippy", command: "cargo clippy" },
  { id: "cargo_test", label: "Test", command: "cargo test" },
  { id: "finalize", label: "Finalize", description: "Publishing merge commit and cleaning up" },
];

function PhaseIcon({ status }: { status: "started" | "passed" | "failed" | "skipped" | "pending" }) {
  if (status === "started") {
    return (
      <div className="relative">
        <Loader2 className="w-4 h-4 animate-spin" style={{ color: "var(--status-info)" }} />
      </div>
    );
  }
  if (status === "passed") {
    return <CheckCircle2 className="w-4 h-4" style={{ color: "var(--status-success)" }} />;
  }
  if (status === "failed") {
    return <XCircle className="w-4 h-4" style={{ color: "var(--status-error)" }} />;
  }
  if (status === "skipped") {
    return <SkipForward className="w-4 h-4 text-text-primary/30" />;
  }
  return (
    <div
      className="w-4 h-4 rounded-full border-2 border-text-primary/15"
    />
  );
}

function phaseTextColor(status: "started" | "passed" | "failed" | "skipped" | "pending"): string {
  switch (status) {
    case "started":
      return "var(--status-info)";
    case "passed":
      return withAlpha("var(--text-primary)", 60);
    case "failed":
      return "var(--status-error)";
    case "skipped":
      return withAlpha("var(--text-primary)", 30);
    default:
      return withAlpha("var(--text-primary)", 25);
  }
}

interface MergePhaseTimelineProps {
  phases: MergeProgressEvent[];
  /** Dynamic phase list from project analysis. Falls back to DEFAULT_PHASE_CONFIG if null. */
  phaseList?: MergePhaseInfo[] | null;
}

export function MergePhaseTimeline({ phases, phaseList }: MergePhaseTimelineProps) {
  if (phases.length === 0) return null;

  const phaseConfig = phaseList ?? DEFAULT_PHASE_CONFIG;

  // Build a lookup of received phase events
  const phaseMap = new Map(phases.map((p) => [p.phase, p]));

  // Determine which phases to display: only phases we've received events for,
  // plus phases from config up to and including the last received one
  const receivedPhases = new Set(phases.map((p) => p.phase));
  let lastReceivedIdx = -1;
  for (let i = phaseConfig.length - 1; i >= 0; i--) {
    const cfg = phaseConfig[i];
    if (cfg && receivedPhases.has(cfg.id)) {
      lastReceivedIdx = i;
      break;
    }
  }

  // Show all phases up to the last received one, plus any beyond if received
  const visiblePhases = phaseConfig.filter((cfg, i) => {
    return i <= lastReceivedIdx || receivedPhases.has(cfg.id);
  });

  return (
    <section data-testid="merge-phase-timeline">
      <SectionTitle>Merge Progress</SectionTitle>
      <div>
        {visiblePhases.map((config, index) => {
          const event = phaseMap.get(config.id);
          const status = event?.status ?? "pending";

          return (
            <div
              key={config.id}
              className="flex items-center gap-2.5 px-3 py-3"
              style={
                index < visiblePhases.length - 1
                  ? { borderBottom: "1px solid var(--border-subtle)" }
                  : undefined
              }
            >
              <PhaseIcon status={status} />
              <div className="flex-1 min-w-0">
                <span
                  className="text-[0.8125rem] font-medium block"
                  style={{ color: phaseTextColor(status) }}
                >
                  {config.label}
                </span>
                {config.command && (
                  <span className="text-[0.625rem] font-mono text-text-primary/25 truncate block max-w-[200px]">
                    $ {config.command}
                  </span>
                )}
                {!config.command && config.description && (
                  <span className="text-[0.625rem] text-text-primary/25 truncate block max-w-[280px]">
                    {config.description}
                  </span>
                )}
              </div>
              {event?.message && status === "started" && (
                <span className="text-[0.6875rem] text-text-primary/40 truncate max-w-[200px]">
                  {event.message}
                </span>
              )}
              {status === "failed" && event?.message && (
                <span className="text-[0.6875rem] truncate max-w-[200px]" style={{ color: "var(--status-error)" }}>
                  {event.message}
                </span>
              )}
              {status === "skipped" && (
                <span className="text-[0.6875rem] text-text-primary/25 truncate max-w-[200px]">
                  skipped
                </span>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}
