import type { CSSProperties, ElementType } from "react";
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  CircleCheck,
  Clock,
  Info,
  Loader2,
  PauseCircle,
  RotateCcw,
  Zap,
} from "lucide-react";

import { STATUS_TOKEN_REFS, withAlpha } from "@/lib/theme-colors";
import type { StatusCounts } from "@/types/status";

export type PlanLifecycleState = "needs_approval" | "approved" | "accepted";

export interface PlanLifecycleAction {
  key: string;
  label: string;
  onClick: () => void;
  icon?: ElementType;
  disabled?: boolean;
  disabledReason?: string | null;
  loading?: boolean;
  primary?: boolean;
  tone?: "default" | "success" | "danger";
  testId?: string;
}

export interface PlanLifecycleRuntimeCounts {
  running: number;
  paused: number;
}

interface PlanLifecycleBannerProps {
  state: PlanLifecycleState;
  title: string;
  description: string;
  actions: readonly PlanLifecycleAction[];
  acceptedFooterActions?: readonly PlanLifecycleAction[] | undefined;
  acceptedRuntimeCounts?: PlanLifecycleRuntimeCounts | undefined;
  counts?: StatusCounts | undefined;
  acceptedAt?: string | null | undefined;
  onViewWork?: (() => void) | undefined;
  onRestartImplementation?: (() => void) | undefined;
  canRestartImplementation?: boolean | undefined;
  isRestartingImplementation?: boolean | undefined;
}

const LIFECYCLE_CONFIG: Record<
  PlanLifecycleState,
  { accent: string; icon: ElementType }
> = {
  needs_approval: {
    accent: STATUS_TOKEN_REFS.warning,
    icon: AlertTriangle,
  },
  approved: {
    accent: STATUS_TOKEN_REFS.info,
    icon: Info,
  },
  accepted: {
    accent: STATUS_TOKEN_REFS.success,
    icon: CheckCircle2,
  },
};

function formatTimestamp(iso: string): string {
  try {
    const date = new Date(iso);
    return date.toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

function actionButtonStyle(
  action: PlanLifecycleAction,
): CSSProperties {
  if (action.primary) {
    return {
      backgroundColor: "var(--accent-primary)",
      borderColor: "var(--accent-border)",
      borderStyle: "solid",
      borderWidth: "1px",
      color: "var(--text-inverse)",
      boxShadow: `0 1px 4px ${withAlpha("var(--accent-primary)", 25)}`,
    };
  }

  if (action.tone === "danger") {
    return {
      backgroundColor: withAlpha("var(--status-error)", 8),
      borderColor: withAlpha("var(--status-error)", 35),
      borderStyle: "solid",
      borderWidth: "1px",
      color: "var(--status-error)",
    };
  }

  if (action.tone === "success") {
    return {
      backgroundColor: "var(--status-success-muted)",
      borderColor: "var(--status-success-border)",
      borderStyle: "solid",
      borderWidth: "1px",
      color: "var(--status-success)",
    };
  }

  return {
    backgroundColor: "transparent",
    borderColor: "var(--border-subtle)",
    borderStyle: "solid",
    borderWidth: "1px",
    color: "var(--text-secondary)",
  };
}

export function PlanLifecycleBanner({
  state,
  title,
  description,
  actions,
  acceptedFooterActions = [],
  acceptedRuntimeCounts,
  counts,
  acceptedAt = null,
  onViewWork,
  onRestartImplementation,
  canRestartImplementation = false,
  isRestartingImplementation = false,
}: PlanLifecycleBannerProps) {
  const config = LIFECYCLE_CONFIG[state];
  const Icon = config.icon;
  const runningCount = acceptedRuntimeCounts?.running ?? counts?.active ?? 0;
  const pausedCount = acceptedRuntimeCounts?.paused ?? 0;
  const rootStyle = {
    "--plan-lifecycle-accent": config.accent,
    backgroundColor: withAlpha(config.accent, 8),
    borderColor: withAlpha(config.accent, 35),
    borderStyle: "solid",
    borderWidth: "1px",
    boxShadow: `0 0 32px ${withAlpha(config.accent, 8)}, inset 0 1px 0 ${withAlpha(config.accent, 15)}`,
  } as CSSProperties;

  return (
    <div
      data-testid="plan-lifecycle-banner"
      data-lifecycle-state={state}
      className="mb-4 overflow-hidden rounded-xl"
      style={rootStyle}
    >
      <div className="px-5 py-4">
        <div className="mb-3 flex items-start justify-between gap-4">
          <div className="flex min-w-0 items-start gap-2.5">
            <div
              className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full"
              style={{
                backgroundColor: withAlpha(config.accent, 18),
                borderColor: withAlpha(config.accent, 40),
                borderStyle: "solid",
                borderWidth: "1px",
              }}
            >
              <Icon className="h-4 w-4" style={{ color: config.accent }} />
            </div>
            <div className="flex min-w-0 flex-col gap-1">
              <div className="flex min-w-0 flex-col leading-tight">
                <span
                  className="text-[0.9375rem] font-semibold"
                  style={{ color: "var(--text-primary)" }}
                >
                  {title}
                </span>
                {acceptedAt ? (
                  <span
                    className="text-[0.6875rem]"
                    style={{ color: "var(--text-muted)" }}
                  >
                    {formatTimestamp(acceptedAt)}
                  </span>
                ) : null}
              </div>
              {description ? (
                <p
                  className="max-w-[48rem] text-[0.75rem] leading-snug"
                  style={{ color: "var(--text-secondary)" }}
                >
                  {description}
                </p>
              ) : null}
            </div>
          </div>

          <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
            {onRestartImplementation && canRestartImplementation ? (
              <button
                type="button"
                data-testid="restart-implementation-button"
                onClick={onRestartImplementation}
                disabled={isRestartingImplementation}
                className="flex items-center gap-1.5 rounded-lg px-3 py-2 text-[0.75rem] font-semibold transition-all duration-150 disabled:cursor-not-allowed disabled:opacity-60"
                style={{
                  backgroundColor: withAlpha("var(--status-error)", 8),
                  borderColor: withAlpha("var(--status-error)", 35),
                  borderStyle: "solid",
                  borderWidth: "1px",
                  color: "var(--status-error)",
                }}
              >
                <RotateCcw
                  className={
                    isRestartingImplementation
                      ? "h-3.5 w-3.5 animate-spin"
                      : "h-3.5 w-3.5"
                  }
                />
                {isRestartingImplementation
                  ? "Restarting..."
                  : "Restart Implementation"}
              </button>
            ) : null}

            {actions.map((action) => {
              const ActionIcon = action.loading ? Loader2 : action.icon;

              return (
                <button
                  key={action.key}
                  type="button"
                  data-testid={action.testId}
                  onClick={action.onClick}
                  disabled={action.disabled || action.loading}
                  className="flex items-center gap-1.5 rounded-lg px-3 py-2 text-[0.75rem] font-semibold transition-all duration-150 disabled:cursor-not-allowed disabled:opacity-60"
                  style={actionButtonStyle(action)}
                >
                  {ActionIcon ? (
                    <ActionIcon
                      className={
                        action.loading
                          ? "h-3.5 w-3.5 animate-spin"
                          : "h-3.5 w-3.5"
                      }
                    />
                  ) : null}
                  {action.label}
                </button>
              );
            })}

            {state === "accepted" && onViewWork ? (
              <button
                type="button"
                data-testid="view-work-button"
                onClick={onViewWork}
                className="flex items-center gap-1.5 rounded-lg px-4 py-2 text-[0.8125rem] font-semibold transition-all duration-150"
                style={{
                  backgroundColor: STATUS_TOKEN_REFS.success,
                  color: "var(--text-inverse)",
                  boxShadow: `0 1px 4px ${withAlpha(STATUS_TOKEN_REFS.success, 30)}`,
                }}
              >
                View Work
                <ArrowRight className="h-3.5 w-3.5" />
              </button>
            ) : null}
          </div>
        </div>

        {state === "accepted" && (counts || acceptedFooterActions.length > 0) ? (
          <div
            className="flex flex-wrap items-center justify-between gap-3 pt-3"
            style={{
              borderTopColor: withAlpha(config.accent, 15),
              borderTopStyle: "solid",
              borderTopWidth: "1px",
            }}
          >
            {counts ? (
              <div className="flex flex-wrap items-center gap-4">
                <span
                  className="text-[0.8125rem] font-medium"
                  style={{ color: "var(--text-secondary)" }}
                >
                  {counts.total} {counts.total === 1 ? "task" : "tasks"}
                </span>

                {runningCount > 0 ? (
                  <div className="flex items-center gap-1.5">
                    <Zap
                      className="h-3.5 w-3.5"
                      style={{ color: "var(--accent-primary)" }}
                    />
                    <span
                      className="text-[0.75rem] font-medium"
                      style={{ color: "var(--accent-primary)" }}
                    >
                      {runningCount} in progress
                    </span>
                  </div>
                ) : null}

                {pausedCount > 0 ? (
                  <div className="flex items-center gap-1.5">
                    <PauseCircle
                      className="h-3.5 w-3.5"
                      style={{ color: "var(--text-muted)" }}
                    />
                    <span
                      className="text-[0.75rem]"
                      style={{ color: "var(--text-muted)" }}
                    >
                      {pausedCount} paused
                    </span>
                  </div>
                ) : null}

                {counts.done > 0 ? (
                  <div className="flex items-center gap-1.5">
                    <CircleCheck
                      className="h-3.5 w-3.5"
                      style={{ color: STATUS_TOKEN_REFS.success }}
                    />
                    <span
                      className="text-[0.75rem] font-medium"
                      style={{ color: STATUS_TOKEN_REFS.success }}
                    >
                      {counts.done} completed
                    </span>
                  </div>
                ) : null}

                {counts.idle > 0 && runningCount === 0 && counts.done === 0 ? (
                  <div className="flex items-center gap-1.5">
                    <Clock
                      className="h-3.5 w-3.5"
                      style={{ color: "var(--text-muted)" }}
                    />
                    <span
                      className="text-[0.75rem]"
                      style={{ color: "var(--text-muted)" }}
                    >
                      {counts.idle} queued
                    </span>
                  </div>
                ) : null}
              </div>
            ) : null}

            {acceptedFooterActions.length > 0 ? (
              <div className="ml-auto flex flex-wrap items-center justify-end gap-2">
                {acceptedFooterActions.map((action) => {
                  const ActionIcon = action.loading ? Loader2 : action.icon;

                  return (
                    <button
                      key={action.key}
                      type="button"
                      data-testid={action.testId}
                      onClick={action.onClick}
                      disabled={action.disabled || action.loading}
                      title={action.disabled ? (action.disabledReason ?? undefined) : undefined}
                      className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[0.75rem] font-semibold transition-all duration-150 disabled:cursor-not-allowed disabled:opacity-60"
                      style={actionButtonStyle(action)}
                    >
                      {ActionIcon ? (
                        <ActionIcon
                          className={
                            action.loading
                              ? "h-3.5 w-3.5 animate-spin"
                              : "h-3.5 w-3.5"
                          }
                        />
                      ) : null}
                      {action.label}
                    </button>
                  );
                })}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}
