import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, HelpCircle } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useMetricsConfig, useSaveMetricsConfig } from "@/hooks/useMetricsConfig";
import { DEFAULT_METRICS_CONFIG } from "@/types/project-stats";
import type { MetricsConfig } from "@/types/project-stats";

const ACCENT = "var(--accent-primary)";
const HOURS_PER_MONTH_FTE = 160;

type ExperienceLevel = "junior" | "mid" | "senior" | "staff" | "custom";

interface ExperiencePreset {
  label: string;
  description: string;
  simpleBaseHours: number;
  mediumBaseHours: number;
  complexBaseHours: number;
  calendarFactor: number;
}

const EXPERIENCE_PRESETS: Record<Exclude<ExperienceLevel, "custom">, ExperiencePreset> = {
  junior: { label: "Junior", description: "1-2 yrs exp", simpleBaseHours: 4, mediumBaseHours: 10, complexBaseHours: 20, calendarFactor: 2.0 },
  mid: { label: "Mid", description: "3-5 yrs exp", simpleBaseHours: 2, mediumBaseHours: 4, complexBaseHours: 8, calendarFactor: 1.5 },
  senior: { label: "Senior", description: "5-8 yrs exp", simpleBaseHours: 1, mediumBaseHours: 2, complexBaseHours: 4, calendarFactor: 1.3 },
  staff: { label: "Staff+", description: "8+ yrs exp", simpleBaseHours: 0.5, mediumBaseHours: 1, complexBaseHours: 2, calendarFactor: 1.2 },
};

const FIELD_TOOLTIPS: Record<string, string> = {
  simpleBaseHours: "Tasks with 3 or fewer steps. Typically small bug fixes, config changes, or single-file edits that a developer would handle quickly.",
  mediumBaseHours: "Tasks with 4-7 steps. Multi-file changes, feature additions, or moderate refactors requiring some planning and testing.",
  complexBaseHours: "Tasks with 8+ steps. Large features, cross-module changes, or architectural work requiring significant design and coordination.",
  calendarFactor: "Accounts for meetings, context switching, code review, and other overhead that stretches pure coding time. 1.0 = no overhead, 2.0 = half the day is non-coding.",
};

function detectExperienceLevel(config: MetricsConfig): ExperienceLevel {
  for (const [key, preset] of Object.entries(EXPERIENCE_PRESETS)) {
    if (
      config.simpleBaseHours === preset.simpleBaseHours &&
      config.mediumBaseHours === preset.mediumBaseHours &&
      config.complexBaseHours === preset.complexBaseHours &&
      config.calendarFactor === preset.calendarFactor
    ) {
      return key as Exclude<ExperienceLevel, "custom">;
    }
  }
  return "custom";
}

interface EffortEstimationPanelProps {
  lowHours: number;
  highHours: number;
  taskCount: number;
  earliestTaskDate: string | null;
  latestTaskDate: string | null;
  projectId: string;
}

const CALIBRATION_FIELDS = [
  { field: "simpleBaseHours" as const, label: "Simple", sub: "\u22643 steps" },
  { field: "mediumBaseHours" as const, label: "Medium", sub: "4-7 steps" },
  { field: "complexBaseHours" as const, label: "Complex", sub: "8+ steps" },
  { field: "calendarFactor" as const, label: "Overhead", sub: "multiplier" },
] as const;

function formatEstimate(hours: number): string {
  return Math.round(hours).toLocaleString();
}

function formatDateRange(earliest: string | null, latest: string | null): string | null {
  if (!earliest || !latest) return null;
  const fmt = (d: string) => {
    const date = new Date(d + "T00:00:00");
    return date.toLocaleDateString("en-US", { month: "short", year: "numeric" });
  };
  const e = fmt(earliest);
  const l = fmt(latest);
  return e === l ? e : `${e} \u2014 ${l}`;
}

function computeCalendarWeeks(earliest: string | null, latest: string | null): number | null {
  if (!earliest || !latest) return null;
  const start = new Date(earliest + "T00:00:00");
  const end = new Date(latest + "T00:00:00");
  const diffMs = end.getTime() - start.getTime();
  const diffDays = Math.max(1, Math.round(diffMs / (1000 * 60 * 60 * 24)));
  return Math.max(1, Math.round(diffDays / 7));
}

export function EffortEstimationPanel({ lowHours, highHours, taskCount, earliestTaskDate, latestTaskDate, projectId }: EffortEstimationPanelProps) {
  const { data: config } = useMetricsConfig(projectId);
  const { mutate: saveConfig } = useSaveMetricsConfig(projectId);
  const [methodologyOpen, setMethodologyOpen] = useState(false);

  const currentConfig = config ?? DEFAULT_METRICS_CONFIG;
  const isDefault =
    currentConfig.simpleBaseHours === DEFAULT_METRICS_CONFIG.simpleBaseHours &&
    currentConfig.mediumBaseHours === DEFAULT_METRICS_CONFIG.mediumBaseHours &&
    currentConfig.complexBaseHours === DEFAULT_METRICS_CONFIG.complexBaseHours &&
    currentConfig.calendarFactor === DEFAULT_METRICS_CONFIG.calendarFactor &&
    currentConfig.workingDaysPerWeek === DEFAULT_METRICS_CONFIG.workingDaysPerWeek;

  const currentLevel = useMemo(() => detectExperienceLevel(currentConfig), [currentConfig]);

  const midpoint = (lowHours + highHours) / 2;
  const fteMonths = midpoint / HOURS_PER_MONTH_FTE;
  const showFteMonths = midpoint >= HOURS_PER_MONTH_FTE;

  // Compression ratio: estimated work weeks vs actual calendar weeks
  const calendarWeeks = computeCalendarWeeks(earliestTaskDate, latestTaskDate);
  const estimatedWorkWeeks = midpoint / 40; // 40h work week
  const compressionRatio = calendarWeeks != null && calendarWeeks > 0
    ? Math.round(estimatedWorkWeeks / calendarWeeks)
    : null;
  const showCompression = compressionRatio != null && compressionRatio >= 2;

  // Range bar: position of low (coding-only) relative to high (with overhead)
  const rangeBarFillPct = highHours > 0 ? Math.round((lowHours / highHours) * 100) : 0;

  function handleFieldBlur(field: keyof MetricsConfig, value: string) {
    const num = parseFloat(value);
    if (isNaN(num)) return;
    if (field === "workingDaysPerWeek") {
      const clamped = Math.max(1, Math.min(7, Math.round(num)));
      saveConfig({ ...currentConfig, [field]: clamped });
    } else {
      saveConfig({ ...currentConfig, [field]: num });
    }
  }

  function handlePresetChange(level: ExperienceLevel) {
    if (level === "custom") return;
    const preset = EXPERIENCE_PRESETS[level];
    saveConfig({
      ...currentConfig,
      simpleBaseHours: preset.simpleBaseHours,
      mediumBaseHours: preset.mediumBaseHours,
      complexBaseHours: preset.complexBaseHours,
      calendarFactor: preset.calendarFactor,
    });
  }

  function handleReset() {
    saveConfig(DEFAULT_METRICS_CONFIG);
  }

  const dateRange = formatDateRange(earliestTaskDate, latestTaskDate);

  // Tooltip content for hero number hover
  const heroTooltipContent = [
    `Range: ${formatEstimate(lowHours)} \u2013 ${formatEstimate(highHours)} hours`,
    `Low = pure coding time per task`,
    `High = coding + overhead (${currentConfig.calendarFactor}\u00d7)`,
    ``,
    `Simple: ${currentConfig.simpleBaseHours}h \u00d7 ${currentConfig.calendarFactor}`,
    `Medium: ${currentConfig.mediumBaseHours}h \u00d7 ${currentConfig.calendarFactor}`,
    `Complex: ${currentConfig.complexBaseHours}h \u00d7 ${currentConfig.calendarFactor}`,
  ].join("\n");

  // Tooltip for FTE-months context line
  const fteTooltipContent = showFteMonths
    ? `${formatEstimate(midpoint)} hours \u00f7 ${HOURS_PER_MONTH_FTE}h/month = ~${fteMonths.toFixed(1)} FTE-months\nBased on standard 160h full-time month`
    : `${formatEstimate(midpoint)} midpoint hours`;

  return (
    <div
      className="@container rounded-xl"
      style={{
        backgroundColor: "var(--overlay-faint)",
        border: "1px solid var(--overlay-weak)",
      }}
    >
      <div className="flex flex-col p-4 gap-3">
        {/* Header: title + customized badge */}
        <div className="flex items-center gap-2">
          <span
            className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-muted"
            style={{ letterSpacing: "0.08em" }}
          >
            Equivalent Developer Effort
          </span>
          {!isDefault && (
            <span
              className="text-[0.625rem] px-1.5 py-0.5 rounded"
              style={{ backgroundColor: "var(--accent-muted)", color: ACCENT }}
            >
              customized
            </span>
          )}
        </div>

        {/* Hero number: midpoint with tooltip showing full breakdown */}
        <TooltipProvider delayDuration={200}>
          <div className="flex flex-col gap-1.5">
            <Tooltip>
              <TooltipTrigger asChild>
                <div className="flex items-baseline gap-1.5 cursor-default w-fit">
                  <span
                    className="text-[2rem] font-semibold tabular-nums"
                    style={{ color: ACCENT, fontFamily: "system-ui", lineHeight: 1.1 }}
                  >
                    ~{formatEstimate(midpoint)}
                  </span>
                  <span
                    className="text-[0.875rem] font-medium"
                    style={{ color: "var(--text-secondary)" }}
                  >
                    developer hours
                  </span>
                </div>
              </TooltipTrigger>
              <TooltipContent
                side="bottom"
                className="max-w-[280px] text-[0.6875rem] whitespace-pre-line"
              >
                {heroTooltipContent}
              </TooltipContent>
            </Tooltip>

            {/* Range bar */}
            <div className="flex flex-col gap-1">
              <div
                className="w-full rounded-full overflow-hidden"
                style={{ height: "6px", backgroundColor: "var(--overlay-weak)" }}
              >
                <div
                  className="h-full rounded-full"
                  style={{
                    width: "100%",
                    background: `linear-gradient(to right, ${ACCENT} ${rangeBarFillPct}%, var(--accent-muted) ${rangeBarFillPct}%)`,
                  }}
                />
              </div>
              <div
                className="flex justify-between text-[0.625rem] tabular-nums text-text-muted"
              >
                <span>{formatEstimate(lowHours)}h coding only</span>
                <span>{formatEstimate(highHours)}h with overhead</span>
              </div>
            </div>

            {/* Context line: FTE-months, tasks, date range, compression */}
            <Tooltip>
              <TooltipTrigger asChild>
                <div
                  className="flex items-center flex-wrap gap-x-1.5 text-[0.75rem] cursor-default w-fit text-text-muted"
                >
                  {showFteMonths && (
                    <span>~{fteMonths.toFixed(1)} FTE-months</span>
                  )}
                  {showFteMonths && <span className="text-text-muted">&middot;</span>}
                  <span>{taskCount} task{taskCount !== 1 ? "s" : ""}</span>
                  {dateRange != null && (
                    <>
                      <span className="text-text-muted">&middot;</span>
                      <span>{dateRange}</span>
                    </>
                  )}
                  {showCompression && (
                    <span
                      className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded-full ml-1"
                      style={{ backgroundColor: "var(--accent-muted)", color: ACCENT }}
                    >
                      ~{compressionRatio}x compression
                    </span>
                  )}
                </div>
              </TooltipTrigger>
              <TooltipContent
                side="bottom"
                className="max-w-[260px] text-[0.6875rem] whitespace-pre-line"
              >
                {fteTooltipContent}
              </TooltipContent>
            </Tooltip>
          </div>

          {/* Team Level selector — always visible */}
          <div className="flex flex-col gap-1.5 pt-1">
            <span
              className="text-[0.625rem] uppercase tracking-wide text-text-muted"
              style={{ letterSpacing: "0.06em" }}
            >
              Team Level
            </span>
            <div className="flex gap-1" data-testid="experience-level-select">
              {Object.entries(EXPERIENCE_PRESETS).map(([key, preset]) => {
                const isActive = currentLevel === key;
                return (
                  <button
                    key={key}
                    onClick={() => handlePresetChange(key as ExperienceLevel)}
                    className="flex-1 flex flex-col items-center rounded-md px-2 py-1.5 transition-colors"
                    style={{
                      backgroundColor: isActive
                        ? "var(--accent-muted)"
                        : "var(--overlay-faint)",
                      color: isActive ? ACCENT : "var(--text-secondary)",
                    }}
                  >
                    <span className="text-[0.6875rem] font-medium">{preset.label}</span>
                    <span
                      className="text-[0.625rem]"
                      style={{
                        color: isActive ? ACCENT : "var(--text-muted)",
                        opacity: isActive ? 0.7 : 1,
                      }}
                    >
                      {preset.description}
                    </span>
                  </button>
                );
              })}
            </div>
            {currentLevel === "custom" && (
              <span className="text-[0.625rem]" style={{ color: ACCENT, opacity: 0.7 }}>
                Custom values
              </span>
            )}
          </div>

          {/* Methodology & calibration — collapsible */}
          <div>
            <button
              onClick={() => setMethodologyOpen(!methodologyOpen)}
              className="flex items-center gap-1 text-[0.6875rem] transition-colors text-text-muted"
            >
              {methodologyOpen ? (
                <ChevronDown className="w-3.5 h-3.5" />
              ) : (
                <ChevronRight className="w-3.5 h-3.5" />
              )}
              Methodology & calibration
            </button>

            {methodologyOpen && (
              <div className="flex flex-col gap-3 pt-3">
                {/* Range explanation */}
                <div className="flex flex-col gap-0.5">
                  <span className="text-[0.6875rem] text-text-secondary">
                    Range: {formatEstimate(lowHours)} &ndash; {formatEstimate(highHours)} hours
                  </span>
                  <span className="text-[0.625rem] text-text-muted">
                    Coding only (floor) &rarr; with overhead (typical)
                  </span>
                </div>

                {/* Calibration inputs */}
                <div className="grid grid-cols-2 gap-x-3 gap-y-2">
                  {CALIBRATION_FIELDS.map(({ field, label, sub }) => (
                    <div key={field} className="flex items-center justify-between gap-2">
                      <div className="flex items-center gap-1">
                        <div className="flex flex-col">
                          <span className="text-[0.6875rem] text-text-secondary">
                            {label}
                          </span>
                          <span className="text-[0.625rem] text-text-muted">
                            {sub}
                          </span>
                        </div>
                        {FIELD_TOOLTIPS[field] !== undefined && (
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <HelpCircle className="w-3 h-3 shrink-0 text-muted-foreground" />
                            </TooltipTrigger>
                            <TooltipContent
                              side="top"
                              className="max-w-[220px] text-[0.6875rem]"
                            >
                              {FIELD_TOOLTIPS[field]}
                            </TooltipContent>
                          </Tooltip>
                        )}
                      </div>
                      <input
                        id={`insights-calibrate-${field}`}
                        type="number"
                        min={field === "calendarFactor" ? 1 : 0.5}
                        max={field === "calendarFactor" ? 3 : 40}
                        step={0.5}
                        defaultValue={currentConfig[field]}
                        key={currentConfig[field]}
                        onBlur={(e) => handleFieldBlur(field, e.target.value)}
                        className="w-14 rounded px-1.5 py-0.5 text-[0.75rem] text-right tabular-nums outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0 text-text-secondary"
                        style={{
                          backgroundColor: "var(--overlay-weak)",
                          boxShadow: "none",
                          outline: "none",
                        }}
                        data-testid={`calibrate-${field}`}
                        aria-label={label}
                      />
                    </div>
                  ))}

                  {/* Working days per week */}
                  <div className="flex items-center justify-between gap-2 col-span-2 pt-1">
                    <div className="flex items-center gap-1">
                      <div className="flex flex-col">
                        <span className="text-[0.6875rem] text-text-secondary">
                          Work days/week
                        </span>
                        <span className="text-[0.625rem] text-text-muted">
                          for time conversion
                        </span>
                      </div>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <HelpCircle className="w-3 h-3 shrink-0 text-muted-foreground" />
                        </TooltipTrigger>
                        <TooltipContent
                          side="top"
                          className="max-w-[220px] text-[0.6875rem]"
                        >
                          Number of working days per week. Used to convert hours into work weeks and days (8h/day).
                        </TooltipContent>
                      </Tooltip>
                    </div>
                    <input
                      id="insights-calibrate-workingDaysPerWeek"
                      type="number"
                      min={1}
                      max={7}
                      step={1}
                      defaultValue={currentConfig.workingDaysPerWeek}
                      key={currentConfig.workingDaysPerWeek}
                      onBlur={(e) => handleFieldBlur("workingDaysPerWeek", e.target.value)}
                      className="w-14 rounded px-1.5 py-0.5 text-[0.75rem] text-right tabular-nums outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0 text-text-secondary"
                      style={{
                        backgroundColor: "var(--overlay-weak)",
                        boxShadow: "none",
                        outline: "none",
                      }}
                      data-testid="calibrate-workingDaysPerWeek"
                      aria-label="Working days per week"
                    />
                  </div>
                </div>

                {/* What this captures / doesn't capture */}
                <div className="flex flex-col gap-1.5 pt-1">
                  <div className="flex flex-col gap-0.5">
                    <span className="text-[0.625rem] font-medium text-text-muted">
                      What this captures
                    </span>
                    <span className="text-[0.625rem] text-text-muted">
                      Coding, review, and context switching time
                    </span>
                  </div>
                  <div className="flex flex-col gap-0.5">
                    <span className="text-[0.625rem] font-medium text-text-muted">
                      What this does NOT capture
                    </span>
                    <span className="text-[0.625rem] text-text-muted">
                      Requirements, deployment, cross-team coordination
                    </span>
                  </div>
                  <span className="text-[0.625rem] italic text-text-muted">
                    These estimates are conservative by design.
                  </span>
                </div>

                {/* Reset */}
                {!isDefault && (
                  <button
                    onClick={handleReset}
                    className="text-[0.6875rem] transition-colors self-start text-text-muted"
                    data-testid="calibration-reset"
                  >
                    Reset to Senior defaults
                  </button>
                )}
              </div>
            )}
          </div>

          {/* Sovereignty footer */}
          <span className="text-[0.625rem] text-text-muted">
            Computed locally. Your metrics never leave your machine.
          </span>
        </TooltipProvider>
      </div>
    </div>
  );
}
