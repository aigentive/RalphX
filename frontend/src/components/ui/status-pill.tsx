import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/**
 * Design-system status pill: the single pill surface for status/stage/judge
 * badges. Converges the previous ad-hoc pill implementations (detail-shared
 * `Pill`, run-header `StatusPill`, `RunStatusChip`, `PhaseStatusBadge`,
 * `AutomationRunPhaseChip`) on one tone-based component.
 *
 * WKWebView rules: paint/border use explicit longhand style props with
 * one-hop tokens + literal fallbacks — never shorthand `border`/`background`.
 */

export type StatusPillTone = "neutral" | "success" | "warning" | "error" | "accent";

export type StatusPillSize = "sm" | "md";

export type StatusPillVariant = "default" | "tinted";

interface StatusPillToneStyle {
  color: string;
  backgroundColor: string;
  borderColor: string;
}

const TONE_STYLES: Record<StatusPillTone, StatusPillToneStyle> = {
  neutral: {
    color: "var(--text-secondary, #c7c7cc)",
    backgroundColor: "var(--bg-surface, #212127)",
    borderColor: "var(--border-default, #33333b)",
  },
  success: {
    color: "var(--status-success, #2eb867)",
    backgroundColor: "var(--bg-hover, #2a2a31)",
    borderColor: "var(--border-default, #33333b)",
  },
  warning: {
    color: "var(--status-warning, #e8a33d)",
    backgroundColor: "var(--bg-hover, #2a2a31)",
    borderColor: "var(--border-default, #33333b)",
  },
  error: {
    color: "var(--status-error, #e5484d)",
    backgroundColor: "var(--bg-hover, #2a2a31)",
    borderColor: "var(--border-default, #33333b)",
  },
  accent: {
    color: "var(--accent-primary)",
    backgroundColor: "var(--accent-muted, #3a2a22)",
    borderColor: "var(--accent-border, #59392a)",
  },
};

const TINTED_TONE_STYLES: Record<StatusPillTone, StatusPillToneStyle> = {
  neutral: TONE_STYLES.neutral,
  success: {
    color: "var(--status-success, #2eb867)",
    backgroundColor: "var(--status-success-muted, #173c29)",
    borderColor: "var(--status-success-border, #286a45)",
  },
  warning: {
    color: "var(--status-warning, #e8a33d)",
    backgroundColor: "var(--status-warning-muted, #402f18)",
    borderColor: "var(--status-warning-border, #6b4d25)",
  },
  error: {
    color: "var(--status-error, #e5484d)",
    backgroundColor: "var(--status-error-muted, #421f25)",
    borderColor: "var(--status-error-border, #70313b)",
  },
  accent: {
    color: "var(--accent-primary)",
    backgroundColor: "var(--accent-muted, #3a2a22)",
    borderColor: "var(--accent-border, #59392a)",
  },
};

export interface StatusPillProps {
  label: ReactNode;
  /** Semantic tone; `accent` marks live/active state, `success` settled health. */
  tone?: StatusPillTone;
  size?: StatusPillSize;
  /** Additive translucent status treatment for dense summary/list surfaces. */
  variant?: StatusPillVariant;
  /** Optional leading icon slot; callers own icon sizing/spin. */
  icon?: ReactNode;
  /** Pulsing live dot for in-flight state (implies attention, not success). */
  live?: boolean;
  /** Accessible name when compact visible content does not carry the full label. */
  ariaLabel?: string;
  className?: string;
  testId?: string;
}

export function StatusPill({
  label,
  tone = "neutral",
  size = "sm",
  variant = "default",
  icon,
  live = false,
  ariaLabel,
  className,
  testId,
}: StatusPillProps) {
  const style = (variant === "tinted" ? TINTED_TONE_STYLES : TONE_STYLES)[tone];
  return (
    <span
      className={cn(
        "inline-flex w-fit min-w-0 max-w-full items-center gap-1.5 rounded-full px-2 py-0.5 font-semibold",
        size === "sm" ? "text-[0.6875rem]" : "text-xs",
        className,
      )}
      style={{
        color: style.color,
        backgroundColor: style.backgroundColor,
        borderColor: style.borderColor,
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-tone={tone}
      data-variant={variant}
      {...(ariaLabel ? { role: "img" } : {})}
      {...(ariaLabel ? { "aria-label": ariaLabel } : {})}
      {...(testId ? { "data-testid": testId } : {})}
    >
      {live ? (
        <span
          className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full"
          style={{ backgroundColor: style.color }}
          aria-hidden="true"
          data-testid={testId ? `${testId}-live-dot` : undefined}
        />
      ) : null}
      {icon}
      <span className="min-w-0 truncate">{label}</span>
    </span>
  );
}
