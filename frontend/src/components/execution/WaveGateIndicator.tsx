/**
 * WaveGateIndicator - Visual indicator for wave-based validation gates
 *
 * Shows current wave progress, which teammates have passed the gate,
 * and which are still working. Uses macOS Tahoe glass-morphism styling.
 */

import { CheckCircle2, Loader2 } from "lucide-react";
import type { TeammateSummary } from "@/api/running-processes";

interface WaveGateIndicatorProps {
  currentWave: number;
  totalWaves: number;
  teammates: TeammateSummary[];
}

/** Determine if a teammate has passed the current wave gate */
function hasPassedGate(teammate: TeammateSummary, currentWave: number): boolean {
  const doneStatuses = new Set(["completed", "done"]);
  if (doneStatuses.has(teammate.status.toLowerCase())) return true;
  if (teammate.wave !== undefined && teammate.wave > currentWave) return true;
  return false;
}

export function WaveGateIndicator({
  currentWave,
  totalWaves,
  teammates,
}: WaveGateIndicatorProps) {
  const passed = teammates.filter((t) => hasPassedGate(t, currentWave));
  const working = teammates.filter((t) => !hasPassedGate(t, currentWave));
  const allPassed = working.length === 0;

  return (
    <div
      className="rounded-md px-2.5 py-2"
      style={{
        backgroundColor: "var(--overlay-faint)",
        backdropFilter: "blur(12px)",
        WebkitBackdropFilter: "blur(12px)",
        border: "1px solid var(--overlay-weak)",
      }}
    >
      {/* Wave header */}
      <div className="flex items-center justify-between mb-1.5">
        <div className="flex items-center gap-1.5">
          <span
            className="text-[0.625rem] font-semibold uppercase tracking-wider"
            style={{ color: "var(--text-muted)" }}
          >
            Wave {currentWave}/{totalWaves}
          </span>
          {allPassed && (
            <span
              className="text-[0.5625rem] font-medium px-1.5 py-0.5 rounded"
              style={{
                color: "var(--status-success)",
                backgroundColor: "var(--status-success-muted)",
              }}
            >
              Gate Passed
            </span>
          )}
        </div>

        {/* Wave dots */}
        <div className="flex items-center gap-1">
          {Array.from({ length: totalWaves }, (_, i) => {
            const waveNum = i + 1;
            const isComplete = waveNum < currentWave;
            const isCurrent = waveNum === currentWave;
            return (
              <div
                key={waveNum}
                className="w-1.5 h-1.5 rounded-full transition-colors"
                style={{
                  backgroundColor: isComplete
                    ? "var(--accent-primary)"
                    : isCurrent
                      ? "var(--accent-strong)"
                      : "var(--overlay-moderate)",
                }}
              />
            );
          })}
        </div>
      </div>

      {/* Teammate gate status */}
      <div className="flex flex-wrap gap-x-3 gap-y-0.5">
        {passed.map((t) => (
          <div key={t.name} className="flex items-center gap-1">
            <CheckCircle2
              className="w-2.5 h-2.5"
              style={{ color: "var(--status-success)" }}
            />
            <span
              className="text-[0.625rem]"
              style={{ color: "var(--text-muted)" }}
            >
              {t.name}
            </span>
          </div>
        ))}
        {working.map((t) => (
          <div key={t.name} className="flex items-center gap-1">
            <Loader2
              className="w-2.5 h-2.5 animate-spin"
              style={{ color: "var(--accent-primary)" }}
            />
            <span
              className="text-[0.625rem]"
              style={{ color: "var(--text-secondary)" }}
            >
              {t.name}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
