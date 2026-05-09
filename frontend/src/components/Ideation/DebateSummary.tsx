/**
 * DebateSummary - Responsive debate comparison layout
 *
 * Wide (>=768px): Side-by-side grid columns for each advocate.
 * Narrow (<768px): Stacked collapsible cards (DebateAdvocateCard).
 * Winner indicator with warm orange accent at the bottom.
 */

import { useState, useEffect } from "react";
import { Star } from "lucide-react";
import { DebateAdvocateCard } from "./DebateAdvocateCard";

// ============================================================================
// Types
// ============================================================================

export interface DebateAdvocate {
  name: string;
  role: string;
  strengths: string[];
  weaknesses: string[];
  evidence: string[];
  criticChallenge: string;
  color?: string;
}

export interface DebateSummaryData {
  advocates: DebateAdvocate[];
  winner: {
    name: string;
    justification: string;
  };
}

interface DebateSummaryProps {
  data: DebateSummaryData;
  /** Force narrow (stacked) layout — used for testing */
  forceNarrow?: boolean;
}

// ============================================================================
// Wide Layout - Section Header
// ============================================================================

function SectionHeader({ label }: { label: string }) {
  return (
    <h4
      className="text-[0.6875rem] uppercase tracking-wide font-medium mb-1.5"
      style={{ color: "var(--text-muted)" }}
    >
      {label}
    </h4>
  );
}

// ============================================================================
// Wide Layout - Advocate Column
// ============================================================================

function AdvocateColumn({
  advocate,
  isWinner,
}: {
  advocate: DebateAdvocate;
  isWinner: boolean;
}) {
  return (
    <div
      data-testid={`advocate-column-${advocate.name}`}
      className="rounded-xl p-4 space-y-4 transition-all duration-200"
      style={{
        background: "var(--overlay-faint)",
        border: isWinner
          ? "1px solid var(--accent-primary)"
          : "1px solid var(--overlay-faint)",
        borderColor: isWinner ? "var(--accent-primary)" : undefined,
      }}
    >
      {/* Header */}
      <div className="flex items-center gap-2">
        <span
          className="text-[0.8125rem] font-medium tracking-[-0.01em]"
          style={{
            color: isWinner ? "var(--accent-primary)" : "var(--text-primary)",
          }}
        >
          {advocate.name}
        </span>
        <span
          className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded-md"
          style={{
            background: "var(--overlay-faint)",
            border: "1px solid var(--overlay-faint)",
            color: advocate.color ?? "var(--text-muted)",
          }}
        >
          {advocate.role}
        </span>
      </div>

      {/* Strengths */}
      <div>
        <SectionHeader label="Strengths" />
        <ul className="space-y-1">
          {advocate.strengths.map((s) => (
            <li
              key={s}
              className="text-[0.75rem] leading-relaxed pl-3 relative"
              style={{ color: "var(--text-secondary)" }}
            >
              <span
                className="absolute left-0 top-[6px] w-1 h-1 rounded-full"
                style={{ background: "var(--status-success)" }}
              />
              {s}
            </li>
          ))}
        </ul>
      </div>

      {/* Weaknesses */}
      <div>
        <SectionHeader label="Weaknesses" />
        <ul className="space-y-1">
          {advocate.weaknesses.map((w) => (
            <li
              key={w}
              className="text-[0.75rem] leading-relaxed pl-3 relative"
              style={{ color: "var(--text-secondary)" }}
            >
              <span
                className="absolute left-0 top-[6px] w-1 h-1 rounded-full"
                style={{ background: "var(--status-error)" }}
              />
              {w}
            </li>
          ))}
        </ul>
      </div>

      {/* Evidence */}
      <div>
        <SectionHeader label="Evidence" />
        <ul className="space-y-1">
          {advocate.evidence.map((e) => (
            <li
              key={e}
              className="text-[0.75rem] leading-relaxed pl-3 relative"
              style={{ color: "var(--text-secondary)" }}
            >
              <span
                className="absolute left-0 top-[6px] w-1 h-1 rounded-full"
                style={{ background: "var(--text-muted)" }}
              />
              {e}
            </li>
          ))}
        </ul>
      </div>

      {/* Critic Challenge */}
      <div>
        <SectionHeader label="Critic Challenge" />
        <p
          className="text-[0.75rem] leading-relaxed italic pl-3"
          style={{
            color: "var(--text-secondary)",
            borderLeft: "2px solid var(--overlay-faint)",
          }}
        >
          {advocate.criticChallenge}
        </p>
      </div>
    </div>
  );
}

// ============================================================================
// Winner Indicator
// ============================================================================

function WinnerIndicator({
  winner,
}: {
  winner: DebateSummaryData["winner"];
}) {
  return (
    <div
      data-testid="debate-winner"
      className="flex items-center gap-3 rounded-xl px-4 py-3 mt-3"
      style={{
        background: "var(--accent-muted)",
        border: "1px solid var(--accent-border)",
      }}
    >
      <Star
        className="w-4 h-4 flex-shrink-0"
        style={{ color: "var(--accent-primary)", fill: "var(--accent-primary)" }}
      />
      <div className="flex items-baseline gap-2 flex-wrap">
        <span
          className="text-[0.8125rem] font-semibold"
          style={{ color: "var(--accent-primary)" }}
        >
          {winner.name}
        </span>
        <span
          className="text-[0.75rem]"
          style={{ color: "var(--text-secondary)" }}
        >
          {winner.justification}
        </span>
      </div>
    </div>
  );
}

// ============================================================================
// Breakpoint Hook
// ============================================================================

const WIDE_BREAKPOINT = 768;

function useIsNarrow(skip: boolean): boolean {
  const [isNarrow, setIsNarrow] = useState(false);

  useEffect(() => {
    if (skip || typeof window.matchMedia !== "function") return;

    const mql = window.matchMedia(`(max-width: ${WIDE_BREAKPOINT - 1}px)`);
    setIsNarrow(mql.matches);

    const handler = (e: MediaQueryListEvent) => setIsNarrow(e.matches);
    mql.addEventListener("change", handler);
    return () => mql.removeEventListener("change", handler);
  }, [skip]);

  return isNarrow;
}

// ============================================================================
// Main Component
// ============================================================================

export function DebateSummary({ data, forceNarrow }: DebateSummaryProps) {
  const mediaIsNarrow = useIsNarrow(forceNarrow !== undefined);
  const isNarrow = forceNarrow ?? mediaIsNarrow;

  if (isNarrow) {
    return (
      <div className="space-y-2">
        {data.advocates.map((advocate, i) => (
          <DebateAdvocateCard
            key={advocate.name}
            advocate={advocate}
            isWinner={advocate.name === data.winner.name}
            defaultOpen={i === 0}
          />
        ))}
        <WinnerIndicator winner={data.winner} />
      </div>
    );
  }

  return (
    <div>
      <div
        className="grid gap-3"
        style={{
          gridTemplateColumns: `repeat(${data.advocates.length}, 1fr)`,
        }}
      >
        {data.advocates.map((advocate) => (
          <AdvocateColumn
            key={advocate.name}
            advocate={advocate}
            isWinner={advocate.name === data.winner.name}
          />
        ))}
      </div>
      <WinnerIndicator winner={data.winner} />
    </div>
  );
}
