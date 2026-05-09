/**
 * ValidationProgress - Shared components for displaying validation step progress
 *
 * Used by MergingTaskDetail, MergedTaskDetail, and MergeIncompleteTaskDetail
 * to show real-time or historical validation command execution.
 */

import { useState, useMemo } from "react";
import {
  Loader2,
  XCircle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Clock,
  Archive,
  SkipForward,
} from "lucide-react";
import { SectionTitle } from "./SectionTitle";
import type { MergeValidationStepEvent } from "@/types/events";

function formatDuration(ms: number): string {
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}

const NEUTRAL_BADGE_STYLE = {
  backgroundColor: "var(--overlay-weak)",
  color: "var(--text-muted)",
};

function getValidationStatusColor(status: MergeValidationStepEvent["status"]): string {
  switch (status) {
    case "running":
      return "color-mix(in srgb, var(--accent-primary) 82%, var(--text-muted))";
    case "failed":
      return "color-mix(in srgb, var(--status-error) 78%, var(--text-muted))";
    case "success":
    case "cached":
      return "color-mix(in srgb, var(--status-success) 78%, var(--text-muted))";
    case "skipped":
      return "var(--text-muted)";
  }
}

/**
 * ValidationStepRow - Single validation command row with collapsible output
 */
export function ValidationStepRow({ step }: { step: MergeValidationStepEvent }) {
  const isRunning = step.status === "running";
  const isFailed = step.status === "failed";
  const isCached = step.status === "cached";
  const isSkipped = step.status === "skipped";
  const hasOutput = (step.stdout && step.stdout.trim().length > 0) ||
    (step.stderr && step.stderr.trim().length > 0);
  const [expanded, setExpanded] = useState(isRunning || isFailed);
  const iconColor = getValidationStatusColor(step.status);

  const statusIcon = isRunning ? (
    <Loader2 className="w-4 h-4 animate-spin" style={{ color: iconColor }} />
  ) : isFailed ? (
    <XCircle className="w-4 h-4" style={{ color: iconColor }} />
  ) : isSkipped ? (
    <SkipForward className="w-4 h-4 text-text-primary/30" />
  ) : isCached ? (
    <Archive className="w-4 h-4" style={{ color: iconColor }} />
  ) : (
    <CheckCircle2 className="w-4 h-4" style={{ color: iconColor }} />
  );

  return (
    <div>
      <button
        type="button"
        className="w-full flex items-center gap-2.5 px-3 py-3 text-left"
        onClick={() => hasOutput && setExpanded(!expanded)}
        style={{ cursor: hasOutput ? "pointer" : "default" }}
      >
        {statusIcon}
        <span
          className="text-[10px] uppercase font-semibold tracking-wider px-1.5 py-0.5 rounded"
          style={NEUTRAL_BADGE_STYLE}
        >
          validate
        </span>
        <span className="text-[0.75rem] text-text-primary/80 font-mono truncate flex-1" title={step.label}>
          {step.label}
        </span>
        {isCached && (
          <span
            className="text-[9px] uppercase font-semibold tracking-wider px-1.5 py-0.5 rounded shrink-0"
            style={NEUTRAL_BADGE_STYLE}
          >
            Cached
          </span>
        )}
        {step.duration_ms != null && (
          <span className="flex items-center gap-1 text-[0.6875rem] text-text-primary/40 shrink-0">
            <Clock className="w-3 h-3" />
            {formatDuration(step.duration_ms)}
          </span>
        )}
        {hasOutput && (
          expanded
            ? <ChevronDown className="w-3.5 h-3.5 text-text-primary/30 shrink-0" />
            : <ChevronRight className="w-3.5 h-3.5 text-text-primary/30 shrink-0" />
        )}
      </button>
      {expanded && hasOutput && (
        <div
          className="px-3 pb-3 max-h-[200px] overflow-y-auto"
          style={{ scrollbarWidth: "thin" }}
        >
          {step.stdout && step.stdout.trim() && (
            <pre className="text-[0.6875rem] font-mono text-text-primary/50 whitespace-pre-wrap break-all leading-relaxed">
              {step.stdout}
            </pre>
          )}
          {step.stderr && step.stderr.trim() && (
            <pre className="text-[11px] font-mono text-text-primary/50 whitespace-pre-wrap break-all leading-relaxed">
              {step.stderr}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * StepsGroup - Collapsible group of steps with summary header.
 * Used for setup steps and passed validation checks.
 */
export function StepsGroup({ steps, phase, label }: {
  steps: MergeValidationStepEvent[];
  phase: string;
  label: string;
}) {
  const anyFailed = steps.some((s) => s.status === "failed");
  const anyRunning = steps.some((s) => s.status === "running");
  const [expanded, setExpanded] = useState(anyFailed);

  const totalMs = steps.reduce((sum, s) => sum + (s.duration_ms ?? 0), 0);
  const allSkipped = steps.every((s) => s.status === "skipped");
  const groupStatus: MergeValidationStepEvent["status"] =
    anyRunning ? "running" : anyFailed ? "failed" : allSkipped ? "skipped" : "success";
  const groupIconColor = getValidationStatusColor(groupStatus);
  const statusIcon = anyRunning ? (
    <Loader2 className="w-4 h-4 animate-spin" style={{ color: groupIconColor }} />
  ) : anyFailed ? (
    <XCircle className="w-4 h-4" style={{ color: groupIconColor }} />
  ) : allSkipped ? (
    <SkipForward className="w-4 h-4 text-text-primary/30" />
  ) : (
    <CheckCircle2 className="w-4 h-4" style={{ color: groupIconColor }} />
  );

  return (
    <div>
      <button
        type="button"
        className="w-full flex items-center gap-2.5 px-3 py-3 text-left cursor-pointer"
        onClick={() => setExpanded(!expanded)}
      >
        {statusIcon}
        <span
          className="text-[10px] uppercase font-semibold tracking-wider px-1.5 py-0.5 rounded"
          style={NEUTRAL_BADGE_STYLE}
        >
          {phase}
        </span>
        <span className="text-[0.75rem] text-text-primary/80 flex-1">
          {label}
        </span>
        {totalMs > 0 && (
          <span className="flex items-center gap-1 text-[0.6875rem] text-text-primary/40 shrink-0">
            <Clock className="w-3 h-3" />
            {formatDuration(totalMs)}
          </span>
        )}
        {expanded
          ? <ChevronDown className="w-3.5 h-3.5 text-text-primary/30 shrink-0" />
          : <ChevronRight className="w-3.5 h-3.5 text-text-primary/30 shrink-0" />}
      </button>
      {expanded && (
        <div>
          {steps.map((step, i) => {
            const isFailed = step.status === "failed";
            const isRunning = step.status === "running";
            const isSkipped = step.status === "skipped";
            const hasOutput = (step.stdout && step.stdout.trim().length > 0) ||
              (step.stderr && step.stderr.trim().length > 0);
            const stepIconColor = getValidationStatusColor(step.status);
            return (
              <div
                key={`${phase}-${step.command}-${i}`}
                className="px-3 py-3 space-y-1"
                style={
                  i < steps.length - 1
                    ? { borderBottom: "1px solid var(--border-subtle)" }
                    : undefined
                }
              >
                <div className="flex items-center gap-2">
                  {isRunning ? (
                    <Loader2 className="w-3 h-3 animate-spin" style={{ color: stepIconColor }} />
                  ) : isFailed ? (
                    <XCircle className="w-3 h-3" style={{ color: stepIconColor }} />
                  ) : isSkipped ? (
                    <SkipForward className="w-3 h-3 text-text-primary/30" />
                  ) : (
                    <CheckCircle2 className="w-3 h-3" style={{ color: stepIconColor }} />
                  )}
                  <span className="text-[0.6875rem] text-text-primary/60 truncate flex-1" title={step.label}>
                    {step.label}
                  </span>
                  {step.duration_ms != null && (
                    <span className="text-[0.625rem] text-text-primary/30 shrink-0">
                      {formatDuration(step.duration_ms)}
                    </span>
                  )}
                </div>
                <code className="block text-[0.625rem] font-mono text-text-primary/40 pl-5 truncate" title={step.command}>
                  $ {step.command}
                </code>
                {hasOutput && (
                  <div
                    className="pl-5 max-h-[120px] overflow-y-auto"
                    style={{ scrollbarWidth: "thin" }}
                  >
                    {step.stdout && step.stdout.trim() && (
                      <pre className="text-[0.625rem] font-mono text-text-primary/40 whitespace-pre-wrap break-all leading-relaxed">
                        {step.stdout}
                      </pre>
                    )}
                    {step.stderr && step.stderr.trim() && (
                      <pre className="text-[10px] font-mono text-text-primary/40 whitespace-pre-wrap break-all leading-relaxed">
                        {step.stderr}
                      </pre>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/**
 * Parse validation log entries from task metadata (historical data).
 * Returns entries matching MergeValidationStepEvent shape.
 */
function parseMetadataValidationLog(
  metadata: string | Record<string, unknown> | null | undefined,
  logKey = "validation_log",
): MergeValidationStepEvent[] {
  if (!metadata) return [];
  try {
    const parsed = typeof metadata === "string" ? JSON.parse(metadata) : metadata;
    const log = parsed?.[logKey];
    if (!Array.isArray(log)) return [];
    return log.map((entry: Record<string, unknown>) => ({
      task_id: String(entry.task_id ?? ""),
      phase: (entry.phase === "setup" || entry.phase === "validate" || entry.phase === "install") ? entry.phase : "validate",
      command: String(entry.command ?? ""),
      path: String(entry.path ?? ""),
      label: String(entry.label ?? entry.command ?? ""),
      status: (entry.status === "running" || entry.status === "success" || entry.status === "failed" || entry.status === "cached" || entry.status === "skipped")
        ? entry.status
        : "success",
      exit_code: typeof entry.exit_code === "number" ? entry.exit_code : null,
      stdout: typeof entry.stdout === "string" ? entry.stdout : undefined,
      stderr: typeof entry.stderr === "string" ? entry.stderr : undefined,
      duration_ms: typeof entry.duration_ms === "number" ? entry.duration_ms : undefined,
    })) as MergeValidationStepEvent[];
  } catch {
    return [];
  }
}

/**
 * ValidationProgress - Shows real-time or historical validation command progress
 *
 * Data sources:
 * - Live: useValidationEvents(task.id, context) — real-time events during execution/merge
 * - Historical: task.metadata.validation_log — stored after merge attempt
 * - Merge: prefer live events; fall back to metadata if live is empty
 */
export function ValidationProgress({
  taskId,
  metadata,
  liveSteps,
  title = "Merge Validation",
  metadataLogKey = "validation_log",
}: {
  taskId: string;
  metadata?: string | Record<string, unknown> | null | undefined;
  liveSteps?: MergeValidationStepEvent[] | undefined;
  title?: string;
  metadataLogKey?: string;
}) {
  const metadataSteps = useMemo(() => parseMetadataValidationLog(metadata, metadataLogKey), [metadata, metadataLogKey]);
  const steps = liveSteps && liveSteps.length > 0 ? liveSteps : metadataSteps;

  if (steps.length === 0) return null;

  const source = liveSteps && liveSteps.length > 0 ? "live" : "historical";
  const setupSteps = steps.filter((s) => s.phase === "setup");
  const installSteps = steps.filter((s) => s.phase === "install");
  const validateSteps = steps.filter((s) => s.phase !== "setup" && s.phase !== "install");
  const passedValidateSteps = validateSteps.filter((s) => s.status === "success" || s.status === "cached");
  const skippedValidateSteps = validateSteps.filter((s) => s.status === "skipped");
  const activeValidateSteps = validateSteps.filter((s) => s.status !== "success" && s.status !== "cached" && s.status !== "skipped");

  return (
    <section data-testid={`validation-progress-${taskId}`}>
      <SectionTitle>
        {title}
        {source === "live" && (
          <span className="ml-2 text-[0.625rem] font-normal text-text-primary/30">(live)</span>
        )}
      </SectionTitle>
      <div>
        {[
          setupSteps.length > 0 ? (
            <StepsGroup
              key="setup"
              steps={setupSteps}
              phase="setup"
              label={`${setupSteps.length} command${setupSteps.length !== 1 ? "s" : ""}`}
            />
          ) : null,
          installSteps.length > 0 ? (
            <StepsGroup
              key="install"
              steps={installSteps}
              phase="install"
              label={`${installSteps.length} command${installSteps.length !== 1 ? "s" : ""}`}
            />
          ) : null,
          passedValidateSteps.length > 0 ? (
            <StepsGroup
              key="passed"
              steps={passedValidateSteps}
              phase="passed"
              label={`${passedValidateSteps.length} check${passedValidateSteps.length !== 1 ? "s" : ""} passed`}
            />
          ) : null,
          ...activeValidateSteps.map((step, index) => (
            <ValidationStepRow key={`${step.phase}-${step.command}-${index}`} step={step} />
          )),
          skippedValidateSteps.length > 0 ? (
            <StepsGroup
              key="skipped"
              steps={skippedValidateSteps}
              phase="skipped"
              label={`${skippedValidateSteps.length} check${skippedValidateSteps.length !== 1 ? "s" : ""} skipped`}
            />
          ) : null,
        ].filter(Boolean).map((item, index, list) => (
          <div
            key={index}
            style={
              index < list.length - 1
                ? { borderBottom: "1px solid var(--border-subtle)" }
                : undefined
            }
          >
            {item}
          </div>
        ))}
      </div>
    </section>
  );
}
