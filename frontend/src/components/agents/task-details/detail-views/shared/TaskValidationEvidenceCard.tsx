import {
  AlertCircle,
  CheckCircle2,
  CircleSlash,
  Clock3,
  Loader2,
  TestTube2,
  XCircle,
} from "lucide-react";
import { useTaskValidationSummary } from "@/hooks/useTaskValidationSummary";
import type {
  TaskValidationSummary,
  ValidationCommandSummary,
  ValidationRunStatus,
} from "@/hooks/useTaskValidationSummary";
import { DetailCard } from "./DetailCard";
import { StatusPill } from "./StatusPill";

function formatTimestamp(value: string | null | undefined): string | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

function formatDuration(value: number | null): string | null {
  if (value == null) return null;
  if (value < 1_000) return `${value} ms`;
  return `${(value / 1_000).toFixed(value < 10_000 ? 1 : 0)} s`;
}

function humanize(value: string): string {
  return value
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function truncateMiddle(value: string, max = 96): string {
  if (value.length <= max) return value;
  const edge = Math.floor((max - 3) / 2);
  return `${value.slice(0, edge)}...${value.slice(value.length - edge)}`;
}

function runPresentation(summary: TaskValidationSummary) {
  if (!summary.policy_enabled) {
    return {
      icon: CircleSlash,
      label: "Disabled",
      variant: "neutral" as const,
      animated: false,
      message:
        summary.disabled_reason ??
        "Task validation runners are disabled in Review Policy.",
    };
  }

  if (!summary.latest_run) {
    const legacyHint = summary.legacy_validation_cache?.hint_message;
    return {
      icon: AlertCircle,
      label: legacyHint ? "Legacy Evidence" : "No Evidence",
      variant: legacyHint ? ("info" as const) : ("warning" as const),
      animated: false,
      message:
        legacyHint ?? "No backend-managed validation evidence has been recorded.",
    };
  }

  const status = summary.latest_run.status as ValidationRunStatus;
  if (status === "passed") {
    return {
      icon: CheckCircle2,
      label: "Passed",
      variant: "success" as const,
      animated: false,
      message: "Latest task validation completed successfully.",
    };
  }
  if (status === "running") {
    return {
      icon: Loader2,
      label: "Running",
      variant: "info" as const,
      animated: true,
      message: "A task validation run is still in progress.",
    };
  }
  if (status === "skipped") {
    return {
      icon: CircleSlash,
      label: "Skipped",
      variant: "warning" as const,
      animated: false,
      message: "The latest validation run did not execute commands.",
    };
  }
  if (status === "cancelled") {
    return {
      icon: AlertCircle,
      label: "Cancelled",
      variant: "warning" as const,
      animated: false,
      message: "The latest validation run was cancelled.",
    };
  }
  return {
    icon: XCircle,
    label: status === "error" ? "Error" : "Failed",
    variant: "error" as const,
    animated: false,
    message: "The latest validation run did not pass.",
  };
}

function commandTone(command: ValidationCommandSummary) {
  if (command.status === "passed" && command.cache_decision === "cached") {
    return "cached";
  }
  if (command.status === "passed") return command.cache_decision;
  return command.status;
}

function CommandRow({ command }: { command: ValidationCommandSummary }) {
  const duration = formatDuration(command.duration_ms);
  const label = command.label || humanize(command.category);
  const tone = commandTone(command);

  return (
    <div className="space-y-1 rounded-lg bg-[var(--bg-elevated)] px-2.5 py-2">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-[0.75rem] font-medium text-text-primary/75">
            {label}
          </div>
          <div className="truncate font-mono text-[0.6875rem] text-text-primary/45">
            {truncateMiddle(command.command)}
          </div>
        </div>
        <span className="shrink-0 rounded-full bg-[var(--bg-surface)] px-2 py-0.5 text-[0.625rem] font-medium text-text-primary/55">
          {humanize(tone)}
        </span>
      </div>
      {(command.reason || duration) && (
        <div className="flex items-center justify-between gap-2 text-[0.6875rem] text-text-primary/40">
          {command.reason && <span className="truncate">{command.reason}</span>}
          {duration && <span className="shrink-0">{duration}</span>}
        </div>
      )}
    </div>
  );
}

export function TaskValidationEvidenceCard({ taskId }: { taskId: string }) {
  const { data, isLoading, isError } = useTaskValidationSummary(taskId);

  if (isLoading) {
    return (
      <DetailCard>
        <div className="flex items-center gap-2 text-[0.75rem] text-text-primary/45">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          <span>Loading validation evidence</span>
        </div>
      </DetailCard>
    );
  }

  if (isError || !data) {
    return (
      <DetailCard variant="warning">
        <div className="flex items-center gap-2 text-[0.75rem] text-text-primary/55">
          <AlertCircle className="h-3.5 w-3.5 text-[var(--status-warning)]" />
          <span>Validation evidence unavailable</span>
        </div>
      </DetailCard>
    );
  }

  const presentation = runPresentation(data);
  const completedAt = formatTimestamp(data.latest_run?.completed_at);
  const startedAt = formatTimestamp(data.latest_run?.started_at);
  const visibleCommands = data.commands.slice(0, 4);
  const extraCommandCount = Math.max(data.commands.length - visibleCommands.length, 0);
  const cardVariant =
    presentation.variant === "neutral" ? "default" : presentation.variant;

  return (
    <DetailCard variant={cardVariant}>
      <div className="space-y-3">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2">
            <TestTube2 className="h-4 w-4 shrink-0 text-text-primary/45" />
            <div className="min-w-0">
              <div className="text-[0.8125rem] font-medium text-text-primary/80">
                Task Validation
              </div>
              <div className="truncate text-[0.75rem] text-text-primary/45">
                {data.latest_run?.head_short_sha
                  ? `HEAD ${data.latest_run.head_short_sha}`
                  : "Persisted evidence"}
              </div>
            </div>
          </div>
          <StatusPill
            icon={presentation.icon}
            label={presentation.label}
            variant={presentation.variant}
            animated={presentation.animated}
          />
        </div>

        <p className="text-[0.75rem] leading-relaxed text-text-primary/50">
          {presentation.message}
        </p>

        {(completedAt || startedAt) && (
          <div className="flex items-center gap-1.5 text-[0.6875rem] text-text-primary/40">
            <Clock3 className="h-3 w-3" />
            <span>{completedAt ? `Completed ${completedAt}` : `Started ${startedAt}`}</span>
          </div>
        )}

        {visibleCommands.length > 0 && (
          <div className="space-y-2">
            {visibleCommands.map((command) => (
              <CommandRow key={command.id} command={command} />
            ))}
            {extraCommandCount > 0 && (
              <div className="text-[0.6875rem] text-text-primary/40">
                {extraCommandCount} more command{extraCommandCount === 1 ? "" : "s"}
              </div>
            )}
          </div>
        )}
      </div>
    </DetailCard>
  );
}
