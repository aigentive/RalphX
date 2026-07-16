import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircle2,
  Circle,
  CirclePause,
  CircleStop,
  Loader2,
  Play,
  RotateCcw,
  Workflow,
  XCircle,
} from "lucide-react";
import React, { useContext, useEffect, useMemo, useRef, useState } from "react";

import {
  agentWorkflowApi,
  isAgentWorkflowTerminal,
  type AgentWorkflowProgress,
} from "@/api/agent-workflows";
import { ChildSessionNavigationContext } from "./ChildSessionNavigationContext";
import { Badge, WidgetCard, WidgetHeader } from "./shared";
import {
  colors,
  parseMcpToolResult,
  type BadgeVariant,
  type ToolCallWidgetProps,
} from "./shared.constants";

interface WorkflowScriptResult {
  id: string;
  source: string;
  script_hash: string;
  permission_hash: string;
  permission_summary_json: string;
  estimated_fanout: number;
  meta: {
    name: string;
    description?: string;
    phases?: string[];
    maxConcurrency?: number;
    maxInvocations?: number;
  };
}

function isWorkflowScriptResult(value: unknown): value is WorkflowScriptResult {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.id === "string" &&
    typeof record.source === "string" &&
    typeof record.script_hash === "string" &&
    typeof record.permission_hash === "string" &&
    typeof record.meta === "object" &&
    record.meta !== null
  );
}

function statusVariant(status: string): BadgeVariant {
  if (status === "completed") return "success";
  if (status === "failed" || status === "cancelled") return "error";
  if (status === "paused" || status === "pause_requested") return "warning";
  if (status === "running" || status === "recovering") return "blue";
  return "muted";
}

function statusIcon(status: string) {
  if (status === "completed") return <CheckCircle2 size={12} />;
  if (status === "failed" || status === "cancelled") return <XCircle size={12} />;
  if (status === "running" || status === "recovering") {
    return <Loader2 size={12} className="animate-spin" />;
  }
  if (status === "paused" || status === "pause_requested") {
    return <CirclePause size={12} />;
  }
  return <Circle size={12} />;
}

const actionClassName =
  "inline-flex h-7 items-center gap-1.5 rounded-md border border-[var(--border-subtle)] px-2.5 text-[0.6875rem] font-medium text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] disabled:cursor-not-allowed disabled:opacity-50";

function WorkflowProgressBody({
  progress,
  busy,
  onAction,
}: {
  progress: AgentWorkflowProgress;
  busy: boolean;
  onAction: (action: "pause" | "resume" | "cancel") => void;
}) {
  const navigateToChild = useContext(ChildSessionNavigationContext);
  const status = progress.run.status;
  const isReadOnly = status === "disabled";
  const isActivelyElapsed = !isReadOnly && !isAgentWorkflowTerminal(status);
  const [currentTimeMs, setCurrentTimeMs] = useState(() => Date.now());
  useEffect(() => {
    if (!isActivelyElapsed) return;
    setCurrentTimeMs(Date.now());
    const interval = window.setInterval(() => setCurrentTimeMs(Date.now()), 1_000);
    return () => window.clearInterval(interval);
  }, [isActivelyElapsed]);
  const totalTokens =
    progress.usage.inputTokens +
    progress.usage.outputTokens +
    progress.usage.cacheCreationTokens +
    progress.usage.cacheReadTokens;
  const elapsedEndMs = progress.run.completedAt
    ? new Date(progress.run.completedAt).getTime()
    : isActivelyElapsed
      ? currentTimeMs
      : new Date(progress.run.updatedAt).getTime();
  const elapsedMs = elapsedEndMs - new Date(progress.run.createdAt).getTime();

  return (
    <div className="space-y-3" data-testid="agent-workflow-progress">
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-[0.6875rem] text-[var(--text-muted)]">
        <span>Elapsed: {Math.max(0, Math.round(elapsedMs / 1_000))}s</span>
        <span>Tokens: {totalTokens.toLocaleString()}</span>
        <span>Cost: ${progress.usage.estimatedUsd.toFixed(4)}</span>
      </div>
      {progress.phases.length > 0 && (
        <div className="space-y-1.5">
          <p className="text-[0.6875rem] font-medium text-[var(--text-muted)]">Phases</p>
          {progress.phases.map((phase) => (
            <div key={phase.id} className="flex items-center gap-2 text-xs">
              <span style={{ color: colors.textMuted }}>{statusIcon(phase.status)}</span>
              <span className="min-w-0 flex-1 truncate text-[var(--text-secondary)]">
                {phase.name}
              </span>
              <Badge variant={statusVariant(phase.status)} compact>
                {phase.status.replace(/_/g, " ")}
              </Badge>
            </div>
          ))}
        </div>
      )}

      {progress.invocations.length > 0 && (
        <div className="space-y-1.5">
          <p className="text-[0.6875rem] font-medium text-[var(--text-muted)]">Agents</p>
          {progress.invocations.map((invocation) => (
            <div key={invocation.id} className="flex items-center gap-2 text-xs">
              <span style={{ color: colors.textMuted }}>{statusIcon(invocation.status)}</span>
              <span className="min-w-0 flex-1 truncate text-[var(--text-secondary)]">
                {invocation.agentName}
              </span>
              {invocation.childConversationId && (
                <button
                  type="button"
                  className="text-[0.6875rem] text-[var(--accent-primary)] hover:underline"
                  onClick={() => navigateToChild(invocation.childConversationId!)}
                >
                  Open transcript
                </button>
              )}
              <Badge variant={statusVariant(invocation.status)} compact>
                {invocation.status}
              </Badge>
            </div>
          ))}
        </div>
      )}

      {progress.logs.length > 0 && (
        <div className="rounded-md bg-[var(--bg-base)] px-2.5 py-2 text-[0.6875rem] text-[var(--text-muted)]">
          {progress.logs.slice(-3).map((entry) => (
            <div key={entry.sequence} className="truncate">
              {entry.message}
            </div>
          ))}
        </div>
      )}

      {progress.run.error && (
        <p className="text-xs text-[var(--status-error)]">{progress.run.error}</p>
      )}

      {!isReadOnly && !isAgentWorkflowTerminal(status) && (
        <div className="flex flex-wrap gap-2">
          {status === "paused" ? (
            <button
              type="button"
              className={actionClassName}
              disabled={busy}
              onClick={() => onAction("resume")}
            >
              <RotateCcw size={12} /> Resume
            </button>
          ) : (
            <button
              type="button"
              className={actionClassName}
              disabled={busy || status === "pause_requested"}
              onClick={() => onAction("pause")}
            >
              <CirclePause size={12} /> Pause
            </button>
          )}
          <button
            type="button"
            className={actionClassName}
            disabled={busy}
            onClick={() => onAction("cancel")}
          >
            <CircleStop size={12} /> Cancel
          </button>
        </div>
      )}
    </div>
  );
}

export const AgentWorkflowWidget = React.memo(function AgentWorkflowWidget({
  toolCall,
  compact = false,
}: ToolCallWidgetProps) {
  const queryClient = useQueryClient();
  const parsed = parseMcpToolResult(toolCall.result);
  const script = isWorkflowScriptResult(parsed) ? parsed : null;
  const resultRunId =
    typeof parsed.id === "string" && typeof parsed.status === "string" ? parsed.id : null;
  const nestedRun =
    parsed.run && typeof parsed.run === "object"
      ? (parsed.run as Record<string, unknown>)
      : null;
  const progressRunId = typeof nestedRun?.id === "string" ? nestedRun.id : null;
  const [launchedRunId, setLaunchedRunId] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [showSource, setShowSource] = useState(false);
  const launchIdRef = useRef(globalThis.crypto.randomUUID());
  const latestRunQuery = useQuery({
    queryKey: ["agent-workflow-latest-run", script?.id],
    queryFn: () => agentWorkflowApi.getLatestRun(script!.id),
    enabled: Boolean(script),
  });
  const runId =
    launchedRunId ?? resultRunId ?? progressRunId ?? latestRunQuery.data?.id;
  const progressQuery = useQuery({
    queryKey: ["agent-workflow-progress", runId],
    queryFn: () => agentWorkflowApi.getProgress(runId!),
    enabled: Boolean(runId),
    refetchInterval: (query) => {
      const status = query.state.data?.run.status;
      return status && (isAgentWorkflowTerminal(status) || status === "disabled")
        ? false
        : 1_000;
    },
  });
  const startMutation = useMutation({
    mutationFn: () =>
      agentWorkflowApi.approveAndStart({
        scriptId: script!.id,
        scriptHash: script!.script_hash,
        permissionHash: script!.permission_hash,
        launchId: launchIdRef.current,
      }),
    onSuccess: (run) => {
      setLaunchedRunId(run.id);
      queryClient.setQueryData(["agent-workflow-latest-run", script?.id], run);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({
        queryKey: ["agent-workflow-latest-run", script?.id],
      });
    },
  });
  const actionMutation = useMutation({
    mutationFn: async (action: "pause" | "resume" | "cancel") => {
      if (!runId) return;
      await agentWorkflowApi[action](runId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agent-workflow-progress", runId] });
    },
  });
  const permissionSummary = useMemo(() => {
    if (!script?.permission_summary_json) return "No elevated permissions declared";
    try {
      return JSON.stringify(JSON.parse(script.permission_summary_json));
    } catch {
      return script.permission_summary_json;
    }
  }, [script]);
  const progress = progressQuery.data;
  const visibleStatus = progress?.run.status ?? (runId ? "queued" : "awaiting_approval");

  return (
    <WidgetCard
      compact={compact}
      defaultExpanded
      header={
        <WidgetHeader
          icon={<Workflow size={12} />}
          title={script?.meta.name ?? "Agent Workflow"}
          badge={
            <Badge variant={statusVariant(visibleStatus)} compact={compact}>
              {visibleStatus.replace(/_/g, " ")}
            </Badge>
          }
          compact={compact}
        />
      }
    >
      {script && latestRunQuery.isLoading && !runId && (
        <div className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
          <Loader2 size={12} className="animate-spin" /> Checking durable run state…
        </div>
      )}

      {script && latestRunQuery.isFetched && !runId && !dismissed && (
        <div className="space-y-3" data-testid="agent-workflow-approval">
          {script.meta.description && (
            <p className="text-xs text-[var(--text-secondary)]">{script.meta.description}</p>
          )}
          <div className="grid grid-cols-2 gap-2 text-[0.6875rem] text-[var(--text-muted)]">
            <span>{script.meta.phases?.length ?? 0} phases</span>
            <span>Estimated fanout: {script.estimated_fanout}</span>
            <span>Concurrency: {script.meta.maxConcurrency ?? "—"}</span>
            <span>Invocation cap: {script.meta.maxInvocations ?? "—"}</span>
          </div>
          <div className="rounded-md bg-[var(--bg-base)] px-2.5 py-2 text-[0.6875rem] text-[var(--text-muted)]">
            Permission envelope: {permissionSummary}
          </div>
          {showSource && (
            <pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded-md bg-[var(--bg-base)] p-2.5 text-[0.6875rem] text-[var(--text-secondary)]">
              {script.source}
            </pre>
          )}
          {(startMutation.error || latestRunQuery.error || progressQuery.error) && (
            <p className="text-xs text-[var(--status-error)]">
              {(startMutation.error ?? latestRunQuery.error ?? progressQuery.error)?.message}
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className={actionClassName}
              disabled={startMutation.isPending}
              onClick={() => startMutation.mutate()}
            >
              {startMutation.isPending ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <Play size={12} />
              )}
              Run once
            </button>
            <button
              type="button"
              className={actionClassName}
              onClick={() => setShowSource((value) => !value)}
            >
              {showSource ? "Hide script" : "View script"}
            </button>
            <button
              type="button"
              className={actionClassName}
              onClick={() => setDismissed(true)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {dismissed && !runId && (
        <p className="text-xs text-[var(--text-muted)]">Workflow was not approved or started.</p>
      )}

      {runId && progressQuery.isLoading && (
        <div className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
          <Loader2 size={12} className="animate-spin" /> Loading durable progress…
        </div>
      )}

      {progress && (
        <WorkflowProgressBody
          progress={progress}
          busy={actionMutation.isPending}
          onAction={(action) => actionMutation.mutate(action)}
        />
      )}

      {runId && progressQuery.error && !script && (
        <p className="text-xs text-[var(--status-error)]">{progressQuery.error.message}</p>
      )}
    </WidgetCard>
  );
});
