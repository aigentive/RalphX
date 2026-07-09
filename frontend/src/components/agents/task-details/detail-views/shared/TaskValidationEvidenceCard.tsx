import { useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  CircleSlash,
  Clock3,
  Loader2,
  TestTube2,
  XCircle,
} from "lucide-react";
import {
  useDisplayTaskValidationSummary,
  useTaskValidationLiveState,
  type LiveValidationCommand,
} from "@/hooks/useTaskValidationEvents";
import { useTaskValidationSummary } from "@/hooks/useTaskValidationSummary";
import type {
  TaskValidationSummary,
  ValidationCommandSummary,
  ValidationRunStatus,
} from "@/hooks/useTaskValidationSummary";
import type { MergeValidationStepEvent } from "@/types/events";
import { DetailCard } from "./DetailCard";
import { SectionTitle } from "./SectionTitle";
import { StatusPill } from "./StatusPill";
import { ValidationProgress } from "./ValidationProgress";

type DisplayCommand = ValidationCommandSummary | LiveValidationCommand;

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

function validationCopy(isHistorical: boolean, usingLive: boolean) {
  if (usingLive) {
    return {
      sectionTitle: "Task Validation",
      runLabel: "Live validation run",
      subject: "Live task validation",
    };
  }
  if (isHistorical) {
    return {
      sectionTitle: "Latest Task Validation",
      runLabel: "Latest task validation run",
      subject: "Latest task validation",
    };
  }
  return {
    sectionTitle: "Task Validation",
    runLabel: "Latest validation run",
    subject: "Latest validation run",
  };
}

function passedEvidenceReason(
  latestRun: TaskValidationSummary["latest_run"],
): string | null {
  if (!latestRun) return null;
  if (latestRun.purpose === "baseline") return "baseline_only";
  if (latestRun.review_evidence_eligible === false) {
    return latestRun.ineligible_reason ?? "ineligible";
  }
  return null;
}

function passedEvidencePresentation(
  summary: TaskValidationSummary,
  subject: string,
) {
  const reason = passedEvidenceReason(summary.latest_run);
  if (!reason) {
    return {
      icon: CheckCircle2,
      label: "Passed",
      variant: "success" as const,
      animated: false,
      message: `${subject} completed successfully.`,
    };
  }

  if (reason === "baseline_only") {
    return {
      icon: AlertCircle,
      label: "Baseline Only",
      variant: "info" as const,
      animated: false,
      message: "Baseline validation passed, but final validation is still needed.",
    };
  }

  if (reason === "stale_head") {
    return {
      icon: AlertCircle,
      label: "Stale Evidence",
      variant: "warning" as const,
      animated: false,
      message:
        "Validation passed for an older commit. Final validation is still needed.",
    };
  }

  if (reason === "stale_episode") {
    return {
      icon: AlertCircle,
      label: "Stale Evidence",
      variant: "warning" as const,
      animated: false,
      message:
        "Validation passed for an older execution attempt. Final validation is still needed.",
    };
  }

  if (reason === "no_test_commands") {
    return {
      icon: AlertCircle,
      label: "No Test Evidence",
      variant: "warning" as const,
      animated: false,
      message:
        "Validation passed without test commands. Final validation is still needed.",
    };
  }

  return {
    icon: AlertCircle,
    label: "Needs Final Validation",
    variant: "warning" as const,
    animated: false,
    message: `${subject} passed, but final validation is still needed.`,
  };
}

function runPresentation(summary: TaskValidationSummary, subject: string) {
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
    return passedEvidencePresentation(summary, subject);
  }
  if (status === "running") {
    return {
      icon: Loader2,
      label: "Running",
      variant: "info" as const,
      animated: true,
      message: "Task validation is running.",
    };
  }
  if (status === "skipped") {
    return {
      icon: CircleSlash,
      label: "Skipped",
      variant: "warning" as const,
      animated: false,
      message: `${subject} did not execute commands.`,
    };
  }
  if (status === "cancelled") {
    return {
      icon: AlertCircle,
      label: "Cancelled",
      variant: "warning" as const,
      animated: false,
      message: `${subject} was cancelled.`,
    };
  }
  return {
    icon: XCircle,
    label: status === "error" ? "Error" : "Failed",
    variant: "error" as const,
    animated: false,
    message: `${subject} did not pass.`,
  };
}

function commandStatusToStepStatus(
  status: DisplayCommand["status"],
): MergeValidationStepEvent["status"] {
  if (status === "running") return "running";
  if (status === "passed") return "success";
  if (status === "cached") return "cached";
  if (status === "skipped") return "skipped";
  return "failed";
}

function commandPhase(category: string): MergeValidationStepEvent["phase"] {
  const normalized = category.toLowerCase();
  if (normalized === "setup" || normalized === "install") return normalized;
  return "validate";
}

function commandStartedAt(command: DisplayCommand): string | null {
  return "started_at" in command ? command.started_at : null;
}

function commandElapsed(command: DisplayCommand, now: number): number | undefined {
  if (command.duration_ms != null) return command.duration_ms;
  if (command.status !== "running") return undefined;
  const startedAt = commandStartedAt(command) ?? command.created_at;
  const startedTime = new Date(startedAt).getTime();
  if (Number.isNaN(startedTime)) return undefined;
  return Math.max(now - startedTime, 0);
}

function commandLabel(command: DisplayCommand): string {
  return command.label || humanize(command.category);
}

function commandToStep(
  taskId: string,
  command: DisplayCommand,
  now: number,
): MergeValidationStepEvent {
  return {
    task_id: taskId,
    phase: commandPhase(command.category),
    command: command.command,
    path: command.cwd,
    label: commandLabel(command),
    status: commandStatusToStepStatus(command.status),
    exit_code: command.exit_code,
    stdout: command.stdout_snippet ?? undefined,
    stderr: command.stderr_snippet ?? undefined,
    duration_ms: commandElapsed(command, now),
    context: "execution",
  };
}

function useValidationClock(enabled: boolean): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!enabled) return;
    setNow(Date.now());
    const interval = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(interval);
  }, [enabled]);

  return now;
}

function hasRunningCommand(summary: TaskValidationSummary | undefined): boolean {
  return Boolean(
    summary?.latest_run?.status === "running" ||
      summary?.commands.some((command) => command.status === "running"),
  );
}

export function TaskValidationSummaryCard({
  displaySummary,
  isHistorical,
  usingLive,
}: {
  displaySummary: TaskValidationSummary;
  isHistorical: boolean;
  usingLive: boolean;
}) {
  const copy = validationCopy(isHistorical, usingLive);
  const presentation = runPresentation(displaySummary, copy.subject);
  const completedAt = formatTimestamp(displaySummary.latest_run?.completed_at);
  const startedAt = formatTimestamp(displaySummary.latest_run?.started_at);
  const baseRef = displaySummary.latest_run?.base_ref;
  const head = displaySummary.latest_run?.head_short_sha;
  const totalDuration = displaySummary.commands.reduce(
    (sum, command) => sum + (command.duration_ms ?? 0),
    0,
  );
  const duration = totalDuration > 0 ? formatDuration(totalDuration) : null;

  return (
    <>
      <SectionTitle>{copy.sectionTitle}</SectionTitle>
      <DetailCard>
        <div className="space-y-3">
          <div className="flex items-start justify-between gap-3">
            <div className="flex min-w-0 items-center gap-2">
              <TestTube2 className="h-4 w-4 shrink-0 text-text-primary/45" />
              <div className="min-w-0">
                <div className="text-[0.8125rem] font-medium text-text-primary/80">
                  {copy.runLabel}
                </div>
                <div className="truncate text-[0.75rem] text-text-primary/45">
                  {head ? `HEAD ${head}` : baseRef ? `Base ${baseRef}` : "Persisted evidence"}
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

          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[0.6875rem] text-text-primary/40">
            {(completedAt || startedAt) && (
              <span className="flex items-center gap-1.5">
                <Clock3 className="h-3 w-3" />
                {completedAt ? `Completed ${completedAt}` : `Started ${startedAt}`}
              </span>
            )}
            {duration && <span>{duration} total</span>}
            {baseRef && head && <span>{baseRef}</span>}
          </div>
        </div>
      </DetailCard>
    </>
  );
}

export function TaskValidationCommandList({
  taskId,
  validationSteps,
  usingLive,
}: {
  taskId: string;
  validationSteps: MergeValidationStepEvent[];
  usingLive: boolean;
}) {
  if (validationSteps.length === 0) return null;

  return (
    <ValidationProgress
      taskId={taskId}
      steps={validationSteps}
      sourceLabel={usingLive ? "live" : null}
      title="Validation Commands"
    />
  );
}

export function TaskValidationSection({
  taskId,
  isHistorical = false,
}: {
  taskId: string;
  isHistorical?: boolean;
}) {
  const { data, isLoading, isError } = useTaskValidationSummary(taskId);
  const live = useTaskValidationLiveState(taskId, { enabled: !isHistorical });
  const displaySummary = useDisplayTaskValidationSummary(data, live);
  const usingLive = Boolean(
    live?.latest_run &&
      displaySummary?.latest_run?.id === live.latest_run.id &&
      (live.latest_run.status === "running" ||
        data?.latest_run?.id !== live.latest_run.id),
  );
  const now = useValidationClock(hasRunningCommand(displaySummary));
  const validationSteps = useMemo(
    () =>
      displaySummary
        ? displaySummary.commands.map((command) =>
            commandToStep(displaySummary.task_id, command, now),
          )
        : [],
    [displaySummary, now],
  );

  if (isLoading && !displaySummary) {
    return (
      <section data-testid="task-validation-section" className="space-y-2">
        <SectionTitle>{validationCopy(isHistorical, false).sectionTitle}</SectionTitle>
        <DetailCard>
          <div className="flex items-center gap-2 text-[0.75rem] text-text-primary/45">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            <span>Loading validation evidence</span>
          </div>
        </DetailCard>
      </section>
    );
  }

  if (isError || !displaySummary) {
    return (
      <section data-testid="task-validation-section" className="space-y-2">
        <SectionTitle>{validationCopy(isHistorical, false).sectionTitle}</SectionTitle>
        <DetailCard>
          <div className="flex items-center gap-2 text-[0.75rem] text-text-primary/55">
            <AlertCircle className="h-3.5 w-3.5 text-[var(--status-warning)]" />
            <span>Validation evidence unavailable</span>
          </div>
        </DetailCard>
      </section>
    );
  }

  return (
    <section data-testid="task-validation-section" className="space-y-3">
      <TaskValidationSummaryCard
        displaySummary={displaySummary}
        isHistorical={isHistorical}
        usingLive={usingLive}
      />
      <TaskValidationCommandList
        taskId={taskId}
        validationSteps={validationSteps}
        usingLive={usingLive}
      />
    </section>
  );
}

export function TaskValidationEvidenceCard({ taskId }: { taskId: string }) {
  return <TaskValidationSection taskId={taskId} />;
}
