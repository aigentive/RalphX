/**
 * CollapsibleEstimates - Collapsible EME estimate section with calibration UI
 *
 * Shows estimated manual effort range and allows per-project calibration
 * of the complexity thresholds used in the EME formula.
 */

import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useMetricsConfig, useSaveMetricsConfig } from "@/hooks/useMetricsConfig";
import { DEFAULT_METRICS_CONFIG } from "@/types/project-stats";
import type { EmeEstimate, MetricsConfig } from "@/types/project-stats";

interface CollapsibleEstimatesProps {
  eme: EmeEstimate;
  projectId: string;
}

export function CollapsibleEstimates({ eme, projectId }: CollapsibleEstimatesProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [showFormula, setShowFormula] = useState(false);

  const { data: config } = useMetricsConfig(projectId);
  const { mutate: saveConfig, isPending: isSaving } = useSaveMetricsConfig(projectId);

  const currentConfig = config ?? DEFAULT_METRICS_CONFIG;
  const isDefault =
    currentConfig.simpleBaseHours === DEFAULT_METRICS_CONFIG.simpleBaseHours &&
    currentConfig.mediumBaseHours === DEFAULT_METRICS_CONFIG.mediumBaseHours &&
    currentConfig.complexBaseHours === DEFAULT_METRICS_CONFIG.complexBaseHours &&
    currentConfig.calendarFactor === DEFAULT_METRICS_CONFIG.calendarFactor;

  function handleFieldBlur(field: keyof MetricsConfig, value: string) {
    const num = parseFloat(value);
    if (isNaN(num)) return;
    saveConfig({ ...currentConfig, [field]: num });
  }

  function handleReset() {
    saveConfig(DEFAULT_METRICS_CONFIG);
  }

  return (
    <div data-testid="estimates-section">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-1.5 w-full text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors"
        data-testid="estimates-toggle"
        aria-expanded={isOpen}
      >
        {isOpen ? (
          <ChevronDown className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
        ) : (
          <ChevronRight className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
        )}
        <span className="uppercase tracking-wide">Estimates</span>
        {!isDefault && (
          <span
            className="ml-auto text-[0.625rem] px-1 rounded"
            style={{ backgroundColor: "var(--accent-muted)", color: "var(--accent-primary)" }}
          >
            calibrated
          </span>
        )}
      </button>

      {isOpen && (
        <div className="mt-2 space-y-2 pl-5">
          <div className="flex items-baseline gap-1.5">
            <span
              className="text-lg font-semibold tabular-nums"
              style={{ color: "var(--accent-primary)" }}
              data-testid="eme-value"
            >
              ~{eme.lowHours}–{eme.highHours}h
            </span>
            <span className="text-xs text-[var(--text-muted)]">estimated manual effort</span>
          </div>
          <p className="text-xs text-[var(--text-muted)]">
            Based on task complexity analysis. Ranges are conservative estimates.
          </p>
          <button
            onClick={() => setShowFormula(!showFormula)}
            className="text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] underline transition-colors"
            data-testid="formula-toggle"
          >
            {showFormula ? "Hide methodology" : "Show methodology"}
          </button>
          {showFormula && (
            <div
              className="rounded-lg p-2.5 space-y-3 text-xs text-[var(--text-muted)]"
              style={{ backgroundColor: "var(--overlay-faint)" }}
              data-testid="formula-content"
            >
              {/* Formula display */}
              <div className="space-y-1">
                <div>Simple (≤3 steps, 0 reviews) = {currentConfig.simpleBaseHours}h</div>
                <div>Medium (4–7 steps or 1 review) = {currentConfig.mediumBaseHours}h</div>
                <div>Complex (≥8 steps or ≥2 reviews) = {currentConfig.complexBaseHours}h</div>
                <div className="pt-1 opacity-70">
                  ×{currentConfig.calendarFactor} calendar factor applied
                </div>
              </div>

              {/* Calibration inputs */}
              <div
                className="space-y-2 pt-2"
                style={{ borderTop: "1px solid var(--overlay-weak)" }}
                data-testid="calibration-section"
              >
                <div className="text-[0.625rem] uppercase tracking-wide opacity-60">Calibrate</div>
                {(
                  [
                    { field: "simpleBaseHours" as const, label: "Simple base hours" },
                    { field: "mediumBaseHours" as const, label: "Medium base hours" },
                    { field: "complexBaseHours" as const, label: "Complex base hours" },
                    { field: "calendarFactor" as const, label: "Calendar factor" },
                  ] as const
                ).map(({ field, label }) => (
                  <div key={field} className="flex items-center justify-between gap-2">
                    <label
                      htmlFor={`calibrate-${field}`}
                      className="text-xs text-[var(--text-muted)] shrink-0"
                    >
                      {label}
                    </label>
                    <input
                      id={`calibrate-${field}`}
                      type="number"
                      min={field === "calendarFactor" ? 1 : 0.5}
                      max={field === "calendarFactor" ? 3 : 40}
                      step={0.5}
                      defaultValue={currentConfig[field]}
                      key={currentConfig[field]}
                      onBlur={(e) => handleFieldBlur(field, e.target.value)}
                      className="w-16 rounded px-1.5 py-0.5 text-xs text-right tabular-nums outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
                      style={{
                        backgroundColor: "var(--overlay-weak)",
                        color: "var(--text-secondary)",
                        boxShadow: "none",
                        outline: "none",
                      }}
                      data-testid={`calibrate-${field}`}
                      aria-label={label}
                    />
                  </div>
                ))}

                {!isDefault && (
                  <button
                    onClick={handleReset}
                    disabled={isSaving}
                    className="text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] underline transition-colors"
                    data-testid="calibration-reset"
                  >
                    Reset to defaults
                  </button>
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
