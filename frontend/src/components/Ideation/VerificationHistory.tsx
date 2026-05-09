/**
 * VerificationHistory — round-by-round gap evolution display.
 *
 * Shows gap score trend across verification rounds and lists
 * the current/final gaps with severity breakdown.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  TrendingDown,
  TrendingUp,
  Minus,
  ChevronRight,
} from "lucide-react";
import { withAlpha } from "@/lib/theme-colors";
import type {
  RoundSummary,
  VerificationGap,
  VerificationRoundDetail,
} from "@/types/ideation";

// ============================================================================
// Types
// ============================================================================

export interface VerificationHistoryProps {
  /** Round summaries for the score trend timeline */
  rounds: RoundSummary[];
  /** Full round detail snapshots when the backend provides them */
  roundDetails?: VerificationRoundDetail[];
  /** Final/current gaps for the last round */
  currentGaps?: VerificationGap[];
}

// ============================================================================
// Helpers
// ============================================================================

const SEVERITY_CONFIG = {
  critical: { color: "var(--status-error)", bg: "var(--status-error-muted)", label: "Critical" },
  high: { color: "var(--accent-primary)", bg: withAlpha("var(--accent-primary)", 10), label: "High" },
  medium: { color: "var(--status-warning)", bg: "var(--status-warning-muted)", label: "Medium" },
  low: { color: "var(--text-secondary)", bg: "var(--overlay-weak)", label: "Low" },
} as const;

const SEVERITY_ORDER = ["critical", "high", "medium", "low"] as const;

function groupGapsBySeverity(gaps: VerificationGap[]): Record<string, VerificationGap[]> {
  const grouped: Record<string, VerificationGap[]> = {};
  for (const gap of gaps) {
    if (!grouped[gap.severity]) grouped[gap.severity] = [];
    grouped[gap.severity]!.push(gap);
  }
  return grouped;
}

function gapKey(gap: VerificationGap): string {
  return `${gap.severity}::${gap.category}::${gap.description}`;
}

type RoundLineageEntry = {
  round: VerificationRoundDetail;
  previousRound: number | undefined;
  resolved: VerificationGap[];
  isLatest: boolean;
};

function buildRoundLineageEntries(roundDetails: VerificationRoundDetail[]): RoundLineageEntry[] {
  const chronological = [...roundDetails].sort((left, right) => left.round - right.round);

  return chronological
    .map((round, index) => {
      const previous = index > 0 ? chronological[index - 1] : undefined;
      const currentKeys = new Set(round.gaps.map(gapKey));
      const resolved = previous
        ? previous.gaps.filter((gap) => !currentKeys.has(gapKey(gap)))
        : [];

      return {
        round,
        previousRound: previous?.round,
        resolved,
        isLatest: index === chronological.length - 1,
      };
    })
    .reverse();
}

// ============================================================================
// Sub-components
// ============================================================================

function RoundTimeline({
  rounds,
  selectedRound,
  onSelectRound,
}: {
  rounds: RoundSummary[];
  selectedRound: number | null;
  onSelectRound: (round: number) => void;
}) {
  if (rounds.length === 0) return null;

  const maxScore = Math.max(...rounds.map((r) => r.gapScore), 1);

  return (
    <div className="mb-5">
      <div
        className="text-[0.6875rem] font-semibold uppercase tracking-wider mb-3"
        style={{ color: "var(--text-muted)" }}
      >
        Gap Score by Round
      </div>
      <div className="flex items-end gap-2">
        {rounds.map((round, idx) => {
          const prevScore = idx > 0 ? rounds[idx - 1]!.gapScore : round.gapScore;
          const delta = round.gapScore - prevScore;
          const barHeight = Math.max(4, Math.round((round.gapScore / maxScore) * 48));
          const isSelected = selectedRound === round.round;

          // Select semantic token per state. Light variants for normal, heavy for the selected bar.
          let barToken = "var(--accent-primary)";
          if (round.gapScore === 0) barToken = "var(--status-success)";
          else if (delta < 0) barToken = "var(--status-success)";
          else if (delta > 0) barToken = "var(--status-error)";

          const barBg = withAlpha(barToken, isSelected ? 80 : 40);
          const barBorder = isSelected
            ? `1px solid ${withAlpha(barToken, 90)}`
            : "1px solid transparent";

          return (
            <button
              key={round.round}
              type="button"
              onClick={() => onSelectRound(round.round)}
              aria-pressed={isSelected}
              aria-label={`Round ${round.round} — gap score ${round.gapScore}`}
              data-testid={`verification-round-bar-${round.round}`}
              className="flex flex-col items-center gap-1.5 flex-1 min-w-0 rounded-md p-1 -mx-1 transition-colors hover:bg-[var(--overlay-faint)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]"
            >
              {/* Trend arrow */}
              <div className="h-4 flex items-center">
                {idx === 0 || delta === 0 ? (
                  <Minus className="w-3 h-3" style={{ color: "var(--text-muted)" }} />
                ) : delta < 0 ? (
                  <TrendingDown className="w-3 h-3" style={{ color: "var(--status-success)" }} />
                ) : (
                  <TrendingUp className="w-3 h-3" style={{ color: "var(--status-error)" }} />
                )}
              </div>
              {/* Bar */}
              <div className="w-full flex flex-col items-center gap-1">
                <span
                  className="text-[0.625rem] font-medium tabular-nums"
                  style={{ color: isSelected ? "var(--text-primary)" : "var(--text-muted)" }}
                >
                  {round.gapScore}
                </span>
                <div
                  className="w-full rounded-sm transition-all duration-300"
                  style={{
                    height: `${barHeight}px`,
                    background: barBg,
                    border: barBorder,
                  }}
                />
              </div>
              {/* Round label */}
              <span
                className="text-[0.625rem]"
                style={{ color: isSelected ? "var(--text-secondary)" : "var(--text-muted)" }}
              >
                R{round.round}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function GapBreakdown({ gaps }: { gaps: VerificationGap[] }) {
  const grouped = groupGapsBySeverity(gaps);

  return (
    <div>
      <div
        className="text-[0.6875rem] font-semibold uppercase tracking-wider mb-3"
        style={{ color: "var(--text-muted)" }}
      >
        Final Gaps ({gaps.length})
      </div>
      <div className="space-y-3">
        {SEVERITY_ORDER.filter((sev) => grouped[sev]?.length).map((severity) => {
          const severityGaps = grouped[severity]!;
          const config = SEVERITY_CONFIG[severity];
          return (
            <div key={severity}>
              <div className="flex items-center gap-2 mb-1.5">
                <span
                  className="w-1.5 h-1.5 rounded-full flex-shrink-0"
                  style={{ background: config.color }}
                />
                <span
                  className="text-[0.6875rem] font-medium"
                  style={{ color: config.color }}
                >
                  {config.label} ({severityGaps.length})
                </span>
              </div>
              <div className="space-y-1.5 pl-3.5">
                {severityGaps.map((gap, idx) => (
                  <div
                    key={idx}
                    className="rounded-md px-2.5 py-2"
                    style={{ background: config.bg }}
                  >
                    <div className="text-[0.75rem]" style={{ color: "var(--text-primary)" }}>
                      {gap.description}
                    </div>
                    {gap.whyItMatters && (
                      <div
                        className="text-[0.6875rem] mt-0.5"
                        style={{ color: "var(--text-secondary)" }}
                      >
                        {gap.whyItMatters}
                      </div>
                    )}
                    <div
                      className="text-[0.625rem] mt-1 font-medium uppercase tracking-wide"
                      style={{ color: "var(--text-muted)" }}
                    >
                      {gap.category}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function AddressedGaps({
  roundNumber,
  previousRound,
  resolved,
}: {
  roundNumber: number;
  previousRound: number;
  resolved: VerificationGap[];
}) {
  const [showResolved, setShowResolved] = useState(false);

  return (
    <div>
      <button
        type="button"
        onClick={() => setShowResolved((v) => !v)}
        className="flex items-center gap-1.5 text-[0.6875rem] font-medium transition-colors"
        style={{ color: "var(--status-success)" }}
        aria-expanded={showResolved}
        aria-controls={`verification-round-${roundNumber}-resolved`}
      >
        <ChevronRight
          className="h-3 w-3 transition-transform duration-150"
          style={{ transform: showResolved ? "rotate(90deg)" : "rotate(0deg)" }}
        />
        {resolved.length} addressed since round {previousRound}
      </button>
      {showResolved && (
        <div id={`verification-round-${roundNumber}-resolved`} className="mt-1">
          {resolved.map((gap, gapIndex) => {
            const sev = SEVERITY_CONFIG[gap.severity as keyof typeof SEVERITY_CONFIG];
            const isLast = gapIndex === resolved.length - 1;
            return (
              <div
                key={`${roundNumber}-resolved-${gapIndex}-${gapKey(gap)}`}
                className="flex items-start gap-2.5 py-2"
                style={{ borderBottom: isLast ? "none" : "1px solid var(--overlay-weak)" }}
              >
                <span
                  className="w-1.5 h-1.5 rounded-full flex-shrink-0 mt-1"
                  style={{ background: "var(--status-success)" }}
                />
                <div className="flex-1 min-w-0">
                  <div className="text-[0.75rem] leading-snug" style={{ color: "var(--text-primary)" }}>
                    {gap.description}
                  </div>
                  <div
                    className="text-[0.625rem] mt-0.5 opacity-60"
                    style={{ color: "var(--status-success)" }}
                  >
                    {sev?.label ?? gap.severity} · {gap.category}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function RoundLineage({
  roundDetails,
  selectedRound,
}: {
  roundDetails: VerificationRoundDetail[];
  selectedRound: number | null;
}) {
  const entries = useMemo(() => buildRoundLineageEntries(roundDetails), [roundDetails]);
  const visibleEntries = useMemo(
    () =>
      selectedRound == null
        ? entries
        : entries.filter((entry) => entry.round.round === selectedRound),
    [entries, selectedRound],
  );

  return (
    <div>
      <div
        className="text-[0.6875rem] font-semibold uppercase tracking-wider mb-3"
        style={{ color: "var(--text-muted)" }}
      >
        Round Lineage
      </div>
      <div className="relative">
        {visibleEntries.map(({ round, previousRound, resolved, isLatest }, entryIdx) => {
          const fullIdx = entries.findIndex((e) => e.round.round === round.round);
          const nextFullEntry =
            fullIdx >= 0 && fullIdx < entries.length - 1 ? entries[fullIdx + 1] : undefined;
          const prevScore = nextFullEntry ? nextFullEntry.round.gapScore : round.gapScore;
          const delta = round.gapScore - prevScore;
          const hasDelta = fullIdx >= 0 && fullIdx < entries.length - 1;
          const isOldest = entryIdx === visibleEntries.length - 1;

          let dotColor = "var(--text-muted)";
          if (isLatest && round.gapScore === 0) dotColor = "var(--status-success)";
          else if (isLatest) dotColor = "var(--accent-primary)";
          else if (delta < 0) dotColor = "var(--status-success)";
          else if (delta > 0) dotColor = "var(--status-error)";

          return (
            <div key={round.round} className="relative pl-6" style={{ paddingBottom: isOldest ? 0 : 4 }}>
              {/* Timeline dot */}
              <div
                className="absolute rounded-full"
                style={{
                  left: isLatest ? 0 : 1,
                  top: 8,
                  width: isLatest ? 11 : 9,
                  height: isLatest ? 11 : 9,
                  background: dotColor,
                  boxShadow: isLatest ? `0 0 6px ${withAlpha(dotColor, 30)}` : "none",
                  border: isLatest ? `2px solid ${withAlpha(dotColor, 20)}` : "none",
                }}
              />

              {/* Round header — non-interactive label; round selection
                  happens by clicking the chart bar above. */}
              <div className="flex items-center gap-2 py-1">
                <span
                  className="text-[0.75rem] font-semibold"
                  style={{ color: isLatest ? "var(--text-primary)" : "var(--text-secondary)" }}
                >
                  R{round.round}
                </span>
                <span
                  className="text-[0.6875rem] tabular-nums font-medium"
                  style={{ color: "var(--text-muted)" }}
                >
                  {round.gapScore}
                </span>
                {hasDelta && delta !== 0 && (
                  <span
                    className="text-[0.625rem] font-semibold tabular-nums"
                    style={{ color: delta < 0 ? "var(--status-success)" : "var(--status-error)" }}
                  >
                    {delta > 0 ? "+" : ""}{delta}
                  </span>
                )}
                <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
                  {round.gapCount} gaps
                </span>
                {resolved.length > 0 && (
                  <span className="text-[0.625rem]" style={{ color: "var(--status-success)" }}>
                    {resolved.length} fixed
                  </span>
                )}
                {isLatest && (
                  <span
                    className="text-[0.5625rem] font-semibold uppercase tracking-wider px-1.5 py-px rounded"
                    style={{ background: withAlpha(dotColor, 12), color: dotColor }}
                  >
                    Latest
                  </span>
                )}
              </div>

              {/* Always-expanded round detail (only the selected round renders). */}
              <div
                id={`verification-round-panel-${round.round}`}
                className="pt-1 pb-2 space-y-2"
              >
                  {round.gaps.length > 0 ? (
                    <div>
                      {round.gaps.map((gap, gapIndex) => {
                        const sev = SEVERITY_CONFIG[gap.severity as keyof typeof SEVERITY_CONFIG];
                        const isLast = gapIndex === round.gaps.length - 1;
                        return (
                          <div
                            key={`${round.round}-${gapIndex}-${gapKey(gap)}`}
                            className="flex items-start gap-2 py-1.5"
                            style={{ borderBottom: isLast ? "none" : "1px solid var(--overlay-faint)" }}
                          >
                            <span
                              className="w-1.5 h-1.5 rounded-full flex-shrink-0 mt-[5px]"
                              style={{ background: sev?.color ?? "var(--text-muted)" }}
                            />
                            <div className="flex-1 min-w-0">
                              <div
                                className="text-[0.6875rem] leading-snug"
                                style={{ color: "var(--text-primary)" }}
                              >
                                {gap.description}
                              </div>
                              <div
                                className="text-[0.625rem] mt-0.5 opacity-50"
                                style={{ color: sev?.color ?? "var(--text-muted)" }}
                              >
                                {sev?.label ?? gap.severity} · {gap.category}
                              </div>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  ) : (
                    <div
                      className="text-[0.6875rem] py-1"
                      style={{ color: "var(--text-muted)" }}
                    >
                      No gaps remained after this round.
                    </div>
                  )}

                  {resolved.length > 0 && previousRound !== undefined && (
                    <AddressedGaps
                      roundNumber={round.round}
                      previousRound={previousRound}
                      resolved={resolved}
                    />
                  )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ============================================================================
// Main component
// ============================================================================

export function VerificationHistory({
  rounds,
  roundDetails,
  currentGaps,
}: VerificationHistoryProps) {
  const hasGaps = currentGaps !== undefined && currentGaps.length > 0;
  const hasRoundDetails = roundDetails !== undefined && roundDetails.length > 0;
  // Prefer the latest round that has detail data — that's the round whose
  // lineage we can actually render. Fall back to the rounds summary if no
  // details are available yet.
  const latestRoundNumber = useMemo(() => {
    if (roundDetails && roundDetails.length > 0) {
      return roundDetails.reduce(
        (max, r) => (r.round > max ? r.round : max),
        roundDetails[0]!.round,
      );
    }
    if (rounds.length > 0) {
      return rounds.reduce(
        (max, r) => (r.round > max ? r.round : max),
        rounds[0]!.round,
      );
    }
    return null;
  }, [rounds, roundDetails]);
  const [selectedRound, setSelectedRound] = useState<number | null>(latestRoundNumber);
  // Re-anchor to the latest round when the data set changes (new run).
  useEffect(() => {
    setSelectedRound(latestRoundNumber);
  }, [latestRoundNumber]);
  const handleSelectRound = useCallback((round: number) => {
    setSelectedRound(round);
  }, []);

  return (
    <div className="py-2">
      {/* Round timeline */}
      {rounds.length > 0 && (
        <RoundTimeline
          rounds={rounds}
          selectedRound={selectedRound}
          onSelectRound={handleSelectRound}
        />
      )}

      {/* No rounds yet */}
      {rounds.length === 0 && !hasGaps && (
        <p
          className="text-[0.75rem] py-4 text-center"
          style={{ color: "var(--text-muted)" }}
        >
          No verification rounds recorded.
        </p>
      )}

      {hasRoundDetails && (
        <RoundLineage roundDetails={roundDetails!} selectedRound={selectedRound} />
      )}

      {/* Final gaps */}
      {hasGaps && !hasRoundDetails && <GapBreakdown gaps={currentGaps!} />}
    </div>
  );
}
