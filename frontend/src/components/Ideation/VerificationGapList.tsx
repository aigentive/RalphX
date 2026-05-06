/**
 * VerificationGapList — grouped gap display with severity counts and round trend.
 *
 * Design: macOS Tahoe style, warm orange accent (#ff6b35), SF Pro, no purple/blue.
 */

import { TrendingDown, TrendingUp, Minus } from "lucide-react";
import { withAlpha } from "@/lib/theme-colors";
import type { RoundSummary, VerificationGap } from "@/types/ideation";

// ============================================================================
// Types
// ============================================================================

export interface VerificationGapListProps {
  /** Gaps from the latest/current round */
  gaps: VerificationGap[];
  /** All rounds — used for gap score trend visualization */
  rounds?: RoundSummary[];
  /** Gap score for the latest round (critical*10 + high*3 + medium*1) */
  gapScore?: number;
  /** When true, gaps show checkboxes and can be selected */
  selectable?: boolean;
  /** Set of selected gap indices into the flat `gaps` array (controlled) */
  selectedGaps?: Set<number>;
  /** Called when the selection changes */
  onSelectionChange?: (selected: Set<number>) => void;
}

// ============================================================================
// Config
// ============================================================================

const SEVERITY_CONFIG = {
  critical: {
    label: "Critical",
    color: "var(--status-error)",
    bg: "var(--status-error-muted)",
    border: "var(--status-error-border)",
    dotColor: "var(--status-error)",
    order: 0,
  },
  high: {
    label: "High",
    color: "var(--accent-primary)",
    bg: withAlpha("var(--accent-primary)", 8),
    border: "var(--accent-border)",
    dotColor: "var(--accent-primary)",
    order: 1,
  },
  medium: {
    label: "Medium",
    color: "var(--status-warning)",
    bg: "var(--status-warning-muted)",
    border: "var(--status-warning-border)",
    dotColor: "var(--status-warning)",
    order: 2,
  },
  low: {
    label: "Low",
    color: "var(--text-secondary)",
    bg: "var(--overlay-faint)",
    border: "var(--overlay-moderate)",
    dotColor: "var(--text-secondary)",
    order: 3,
  },
} as const;

type Severity = keyof typeof SEVERITY_CONFIG;

// ============================================================================
// Helpers
// ============================================================================

function groupBySeverity(gaps: VerificationGap[]) {
  const groups: Partial<Record<Severity, Array<{ gap: VerificationGap; index: number }>>> = {};
  for (let i = 0; i < gaps.length; i++) {
    const gap = gaps[i]!;
    const sev = gap.severity as Severity;
    if (!groups[sev]) groups[sev] = [];
    groups[sev]!.push({ gap, index: i });
  }
  return (Object.keys(groups) as Severity[])
    .sort((a, b) => SEVERITY_CONFIG[a].order - SEVERITY_CONFIG[b].order)
    .map((sev) => ({ severity: sev, items: groups[sev]! }));
}

// ============================================================================
// Sub-components
// ============================================================================

function GapScoreTrend({ rounds }: { rounds: RoundSummary[] }) {
  if (rounds.length < 2) return null;

  const last = rounds[rounds.length - 1]!;
  const prev = rounds[rounds.length - 2]!;
  const delta = last.gapScore - prev.gapScore;
  const improving = delta < 0;
  const stable = delta === 0;

  return (
    <div className="flex items-center gap-3 mb-3">
      <div
        className="flex items-center gap-1.5 text-[0.6875rem]"
        style={{ color: "var(--text-muted)" }}
      >
        <span>Gap score:</span>
        <span
          className="font-semibold"
          style={{ color: "var(--text-primary)" }}
        >
          {last.gapScore}
        </span>
      </div>

      <div
        className="flex items-center gap-1 text-[0.6875rem]"
        style={{
          color: stable
            ? "var(--text-secondary)"
            : improving
              ? "var(--status-success)"
              : "var(--status-error)",
        }}
      >
        {stable ? (
          <Minus className="w-3 h-3" />
        ) : improving ? (
          <TrendingDown className="w-3 h-3" />
        ) : (
          <TrendingUp className="w-3 h-3" />
        )}
        <span>
          {stable
            ? "No change"
            : improving
              ? `−${Math.abs(delta)} from round ${prev.round}`
              : `+${delta} from round ${prev.round}`}
        </span>
      </div>

      {/* Mini round history */}
      <div className="flex items-center gap-1 ml-auto">
        {rounds.slice(-5).map((r) => {
          const isLast = r.round === last.round;
          return (
            <div
              key={r.round}
              className="w-1.5 h-4 rounded-sm flex-shrink-0"
              title={`Round ${r.round}: score ${r.gapScore}`}
              style={{
                background: isLast
                  ? "var(--accent-primary)"
                  : "var(--overlay-moderate)",
                opacity: isLast ? 1 : 0.6,
              }}
            />
          );
        })}
      </div>
    </div>
  );
}

// Simple custom checkbox that matches the design system
function GapCheckbox({ checked }: { checked: boolean }) {
  return (
    <span
      className="w-3.5 h-3.5 rounded flex-shrink-0 flex items-center justify-center transition-colors"
      style={{
        background: checked ? withAlpha("var(--accent-primary)", 15) : "transparent",
        border: checked
          ? "1px solid var(--accent-primary)"
          : "1px solid var(--border-default)",
      }}
    >
      {checked && (
        <svg width="8" height="8" viewBox="0 0 8 8" fill="none">
          <path
            d="M1.5 4L3 5.5L6.5 2"
            stroke="var(--accent-primary)"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      )}
    </span>
  );
}

// ============================================================================
// Component
// ============================================================================

export function VerificationGapList({
  gaps,
  rounds,
  gapScore,
  selectable = false,
  selectedGaps,
  onSelectionChange,
}: VerificationGapListProps) {
  if (gaps.length === 0) {
    return (
      <div
        className="flex items-center justify-center py-4 text-[0.75rem]"
        style={{ color: "var(--text-muted)" }}
      >
        No gaps found
      </div>
    );
  }

  const grouped = groupBySeverity(gaps);
  const allIndices = gaps.map((_, i) => i);
  const allSelected = allIndices.every((i) => selectedGaps?.has(i));

  const handleToggle = (index: number) => {
    if (!onSelectionChange) return;
    const next = new Set(selectedGaps);
    if (next.has(index)) {
      next.delete(index);
    } else {
      next.add(index);
    }
    onSelectionChange(next);
  };

  const handleSelectAll = () => {
    if (!onSelectionChange) return;
    onSelectionChange(new Set(allIndices));
  };

  const handleDeselectAll = () => {
    if (!onSelectionChange) return;
    onSelectionChange(new Set());
  };

  return (
    <div className="space-y-3">
      {/* Score and trend row */}
      {rounds && rounds.length > 0 ? (
        <GapScoreTrend rounds={rounds} />
      ) : gapScore !== undefined ? (
        <div
          className="flex items-center gap-1.5 text-[0.6875rem] mb-3"
          style={{ color: "var(--text-muted)" }}
        >
          <span>Gap score:</span>
          <span className="font-semibold" style={{ color: "var(--text-primary)" }}>
            {gapScore}
          </span>
        </div>
      ) : null}

      {/* Severity summary row */}
      <div className="flex items-center gap-2 flex-wrap">
        {grouped.map(({ severity, items }) => {
          const cfg = SEVERITY_CONFIG[severity];
          return (
            <span
              key={severity}
              className="inline-flex items-center gap-1 text-[0.625rem] font-medium px-1.5 py-0.5 rounded-md"
              style={{
                background: cfg.bg,
                border: `1px solid ${cfg.border}`,
                color: cfg.color,
              }}
            >
              <span
                className="w-1.5 h-1.5 rounded-full flex-shrink-0"
                style={{ background: cfg.dotColor }}
              />
              {items.length} {cfg.label}
            </span>
          );
        })}

        {/* Select All / Deselect All */}
        {selectable && onSelectionChange && (
          <button
            type="button"
            className="ml-auto text-[0.625rem] transition-colors"
            style={{ color: "var(--text-muted)" }}
            onClick={allSelected ? handleDeselectAll : handleSelectAll}
            onMouseEnter={(e) => {
              e.currentTarget.style.color = "var(--text-primary)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.color = "var(--text-muted)";
            }}
          >
            {allSelected ? "Deselect All" : "Select All"}
          </button>
        )}
      </div>

      {/* Gap items grouped by severity */}
      <div>
        {grouped.map(({ severity, items }) => {
          const cfg = SEVERITY_CONFIG[severity];
          return (
            <div key={severity} className="mb-2 last:mb-0">
              <div
                className="text-[0.625rem] font-semibold uppercase tracking-wider mb-1"
                style={{ color: cfg.color, opacity: 0.7 }}
              >
                {cfg.label}
              </div>
              <div>
                {items.map(({ gap, index }, localIdx) => {
                  const isSelected = selectedGaps?.has(index) ?? false;
                  const isLast = localIdx === items.length - 1;

                  return (
                    <div
                      key={index}
                      className="flex items-start gap-2.5 py-2"
                      style={{
                        borderBottom: isLast
                          ? "none"
                          : "1px solid var(--overlay-weak)",
                        cursor: selectable ? "pointer" : "default",
                      }}
                      onClick={selectable ? () => handleToggle(index) : undefined}
                    >
                      {/* Checkbox (selectable mode only) */}
                      {selectable && <GapCheckbox checked={isSelected} />}

                      {/* 6px severity dot */}
                      <span
                        className="w-1.5 h-1.5 rounded-full flex-shrink-0 mt-1"
                        style={{ background: cfg.dotColor }}
                      />

                      {/* Content */}
                      <div className="flex-1 min-w-0">
                        <div
                          className="text-[0.75rem] leading-snug"
                          style={{ color: "var(--text-primary)" }}
                        >
                          {gap.description}
                        </div>
                        {gap.whyItMatters && (
                          <div
                            className="text-[0.6875rem] mt-0.5 leading-snug"
                            style={{ color: "var(--text-secondary)" }}
                          >
                            {gap.whyItMatters}
                          </div>
                        )}
                        <div
                          className="text-[0.625rem] mt-0.5 opacity-60"
                          style={{ color: cfg.color }}
                        >
                          {gap.category}
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
