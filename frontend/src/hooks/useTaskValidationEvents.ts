import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useEventBus } from "@/providers/EventProvider";
import {
  taskValidationKeys,
  type TaskValidationSummary,
  type ValidationCacheDecision,
  type ValidationCommandStatus,
  type ValidationRunStatus,
} from "@/hooks/useTaskValidationSummary";
import {
  TaskValidationEventSchema,
  type TaskValidationEvent,
} from "@/types/events";

const MAX_LIVE_STREAM_CHARS = 4_000;
const INVALIDATING_EVENTS = new Set<TaskValidationEvent["type"]>([
  "run_started",
  "command_started",
  "command_completed",
  "run_completed",
]);

export interface LiveValidationCommand {
  id: string;
  command_source: string;
  command_ref: string | null;
  command: string;
  cwd: string;
  label: string | null;
  category: string;
  reason: string | null;
  related_files: string[];
  cache_decision: ValidationCacheDecision;
  status: ValidationCommandStatus;
  exit_code: number | null;
  duration_ms: number | null;
  stdout_snippet: string | null;
  stderr_snippet: string | null;
  stdout_log_path: string | null;
  stderr_log_path: string | null;
  created_at: string;
  started_at: string | null;
}

export interface LiveTaskValidationSummary
  extends Omit<TaskValidationSummary, "commands"> {
  commands: LiveValidationCommand[];
}

function appendBounded(existing: string | null, delta: string | undefined): string | null {
  if (!delta) return existing;
  const next = `${existing ?? ""}${delta}`;
  return next.length > MAX_LIVE_STREAM_CHARS
    ? next.slice(next.length - MAX_LIVE_STREAM_CHARS)
    : next;
}

function runFromEvent(
  event: TaskValidationEvent,
  status: ValidationRunStatus = event.status as ValidationRunStatus,
) {
  return {
    id: event.run_id,
    purpose: event.purpose,
    context_type: event.context_type,
    requested_by_agent: event.requested_by_agent ?? null,
    status,
    mode: event.mode,
    policy_enabled: event.policy_enabled,
    head_sha: event.head_sha ?? null,
    head_short_sha: event.head_short_sha ?? null,
    base_ref: event.base_ref ?? null,
    started_at: event.run_started_at,
    completed_at: event.run_completed_at ?? null,
    ...(event.current_for_head !== undefined && {
      current_for_head: event.current_for_head,
    }),
    ...(event.current_for_execution_episode !== undefined && {
      current_for_execution_episode: event.current_for_execution_episode,
    }),
    ...(event.review_evidence_eligible !== undefined && {
      review_evidence_eligible: event.review_evidence_eligible,
    }),
    ...(event.ineligible_reason !== undefined && {
      ineligible_reason: event.ineligible_reason,
    }),
  };
}

function commandFromEvent(event: TaskValidationEvent): LiveValidationCommand | null {
  if (!event.command_id) return null;
  return {
    id: event.command_id,
    command_source: event.command_source ?? "agent_selected",
    command_ref: event.command_ref ?? null,
    command: event.command ?? "",
    cwd: event.cwd ?? "",
    label: event.label ?? null,
    category: event.category ?? "test",
    reason: event.reason ?? null,
    related_files: [],
    cache_decision: event.cache_decision ?? "ran",
    status: event.status as ValidationCommandStatus,
    exit_code: event.exit_code ?? null,
    duration_ms: event.duration_ms ?? null,
    stdout_snippet: event.stdout_snippet ?? event.stdout_delta ?? null,
    stderr_snippet: event.stderr_snippet ?? event.stderr_delta ?? null,
    stdout_log_path: event.stdout_log_path ?? null,
    stderr_log_path: event.stderr_log_path ?? null,
    created_at: event.command_completed_at ?? event.command_started_at ?? event.emitted_at,
    started_at: event.command_started_at ?? null,
  };
}

function mergeCommandEvent(
  existing: LiveValidationCommand | undefined,
  event: TaskValidationEvent,
): LiveValidationCommand | null {
  const next = commandFromEvent(event);
  if (!next) return existing ?? null;

  if (event.type === "command_output") {
    return {
      ...next,
      ...existing,
      stdout_snippet: appendBounded(existing?.stdout_snippet ?? null, event.stdout_delta),
      stderr_snippet: appendBounded(existing?.stderr_snippet ?? null, event.stderr_delta),
      status: "running",
      duration_ms: existing?.duration_ms ?? null,
      started_at: existing?.started_at ?? next.started_at,
    };
  }

  return {
    ...existing,
    ...next,
    stdout_snippet: next.stdout_snippet ?? existing?.stdout_snippet ?? null,
    stderr_snippet: next.stderr_snippet ?? existing?.stderr_snippet ?? null,
    started_at: existing?.started_at ?? next.started_at,
  };
}

function updateLiveState(
  previous: LiveTaskValidationSummary | null,
  event: TaskValidationEvent,
): LiveTaskValidationSummary {
  const sameRun = previous?.latest_run?.id === event.run_id;
  const base: LiveTaskValidationSummary =
    sameRun && previous
      ? previous
      : {
          task_id: event.task_id,
          project_id: event.project_id,
          policy_enabled: event.policy_enabled,
          latest_run: runFromEvent(event),
          commands: [],
          legacy_validation_cache: null,
          disabled_reason: null,
        };

  if (event.type === "run_started") {
    return {
      ...base,
      latest_run: runFromEvent(event),
      commands: [],
    };
  }

  if (event.type === "run_completed") {
    return {
      ...base,
      latest_run: runFromEvent(event),
    };
  }

  const command = mergeCommandEvent(
    base.commands.find((candidate) => candidate.id === event.command_id),
    event,
  );
  if (!command) {
    return {
      ...base,
      latest_run: runFromEvent(event, "running"),
    };
  }

  const existingIndex = base.commands.findIndex(
    (candidate) => candidate.id === command.id,
  );
  const commands =
    existingIndex >= 0
      ? base.commands.map((candidate, index) =>
          index === existingIndex ? command : candidate,
        )
      : [...base.commands, command];

  return {
    ...base,
    latest_run: runFromEvent(event, "running"),
    commands,
  };
}

export function useTaskValidationLiveState(
  taskId: string,
  options?: { enabled?: boolean },
): LiveTaskValidationSummary | null {
  const enabled = options?.enabled ?? true;
  const bus = useEventBus();
  const [liveState, setLiveState] = useState<LiveTaskValidationSummary | null>(null);

  useEffect(() => {
    if (!enabled || !taskId) {
      setLiveState(null);
      return;
    }

    const unsubscribe = bus.subscribe<unknown>("task_validation:event", (payload) => {
      const parsed = TaskValidationEventSchema.safeParse(payload);
      if (!parsed.success || parsed.data.task_id !== taskId) return;
      setLiveState((previous) => updateLiveState(previous, parsed.data));
    });

    return () => {
      unsubscribe();
    };
  }, [bus, enabled, taskId]);

  return liveState;
}

export function useTaskValidationEventInvalidation() {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(() => {
    const unsubscribe = bus.subscribe<unknown>("task_validation:event", (payload) => {
      const parsed = TaskValidationEventSchema.safeParse(payload);
      if (!parsed.success || !INVALIDATING_EVENTS.has(parsed.data.type)) return;
      queryClient.invalidateQueries({
        queryKey: taskValidationKeys.summary(parsed.data.task_id),
      });
    });

    return () => {
      unsubscribe();
    };
  }, [bus, queryClient]);
}

export function useDisplayTaskValidationSummary(
  persisted: TaskValidationSummary | undefined,
  live: LiveTaskValidationSummary | null,
): TaskValidationSummary | LiveTaskValidationSummary | undefined {
  return useMemo(() => {
    if (!live?.latest_run) return persisted;
    const persistedRunId = persisted?.latest_run?.id;
    if (live.latest_run.status === "running") return live;
    if (!persistedRunId || persistedRunId !== live.latest_run.id) return live;
    return persisted;
  }, [persisted, live]);
}
