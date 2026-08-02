/**
 * ExecutionControlBar - Premium execution status and controls
 *
 * Fixed bottom bar displaying running/queued/merge status counts with animated status indicator
 * and pause/stop controls. Follows the design spec from specs/design/pages/execution-control-bar.md
 *
 * Responsive breakpoints:
 * - Wide (>1200px): Full labels "Running: 2/3", "Queued: 5", "Merge: 1", "Escalated: 1"
 * - Medium (800-1200px): Abbreviated "R: 2/3", "Q: 5", "M: 1"
 * - Narrow (<800px): Counts only "2/3", "5", "1"
 */

import {
  AlertTriangle,
  Loader2,
  Pause,
  Play,
  Square,
  Terminal as TerminalIcon,
} from "lucide-react";
import { useState, useEffect, useMemo } from "react";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useAgentTerminalStore } from "@/components/agents/agentTerminalStore";
import { cn } from "@/lib/utils";
import { RunningProcessPopover } from "./RunningProcessPopover";
import { TerminalsPopover, type ExecutionBarTerminalSession } from "./TerminalsPopover";
import type {
  ExecutionCapacitySummary,
  ExecutionLaneUsage,
  RunningProcess,
  RunningIdeationSession,
  RunningWorkspaceSession,
} from "@/api/running-processes";
import { MergePipelinePopover } from "./MergePipelinePopover";
import type { MergePipelineResponse } from "@/api/merge-pipeline";
import { QueuedTasksPopover } from "./QueuedTasksPopover";
import { PausedTasksPopover } from "./PausedTasksPopover";
import { InfoTooltip } from "./InfoTooltip";
import type { ExecutionBarTaskNavigationTarget } from "./executionTaskNavigation";
import { getStatusIconConfig } from "@/types/status-icons";
import { useProjectStore } from "@/stores/projectStore";
import type { Task } from "@/types/task";
import type { ExecutionHaltMode } from "@/api/execution";
import { useUiStore, type ExecutionBarPopoverKind, type ExecutionBarRunningTab } from "@/stores/uiStore";

interface ExecutionControlBarProps {
  /** The project ID */
  projectId: string;
  /** Whether execution counts came from an authoritative status source. */
  statusKnown?: boolean;
  /** Number of currently running tasks */
  runningCount: number;
  /** Maximum concurrent tasks allowed */
  maxConcurrent: number;
  /** Number of queued (planned) tasks */
  queuedCount: number;
  /** Number of queued agent messages held by pause/capacity barriers */
  queuedMessageCount?: number;
  /** Number of tasks paused due to provider errors */
  pausedCount?: number;
  /** Tasks paused due to provider errors (for popover) */
  pausedTasks?: Task[];
  /** Number of currently generating ideation sessions (consuming slots) */
  ideationActive?: number;
  /** Per-project maximum concurrent ideation sessions */
  ideationMax?: number;
  /** Number of ideation sessions waiting for capacity (pending_initial_prompt) */
  ideationWaiting?: number;
  /** Number of merge tasks actively merging or waiting to merge */
  mergingCount: number;
  /** Number of merge tasks requiring user/operator attention */
  mergeAttentionCount?: number;
  /** Whether any merge tasks need attention (conflict/incomplete) */
  hasAttentionMerges: boolean;
  /** Merge pipeline data for popover */
  mergePipelineData: MergePipelineResponse | null;
  /** Whether execution is paused */
  isPaused: boolean;
  /** Current halt mode for global execution controls */
  haltMode?: ExecutionHaltMode;
  /** Whether a control action is in progress */
  isLoading?: boolean;
  /** Name of the currently executing task (optional) */
  currentTaskName?: string;
  /** Called when pause/resume button clicked */
  onPauseToggle: () => void;
  /** Called when stop button clicked */
  onStop: () => void;
  /** List of running processes (for popover) */
  runningProcesses?: RunningProcess[];
  /** List of running ideation sessions (for popover) */
  ideationSessions?: RunningIdeationSession[];
  /** List of running workspace conversations (for popover) */
  workspaceSessions?: RunningWorkspaceSession[];
  /** Lane-level capacity usage */
  lanes?: ExecutionLaneUsage[];
  /** Capacity summary for all lanes */
  capacity?: ExecutionCapacitySummary | null;
  /** Called when pause button clicked for a specific process */
  onPauseProcess?: (taskId: string) => void;
  /** Called when stop button clicked for a specific process */
  onStopProcess?: (taskId: string) => void;
  /** Called when settings link clicked in popover */
  onOpenSettings?: () => void;
  /** Called when an ideation session is clicked in the running processes popover */
  onNavigateToSession?: (sessionId: string) => void;
  /** Called when a workspace row is clicked in the running processes popover */
  onNavigateToWorkspace?: (
    projectId: string,
    conversationId: string,
    session?: RunningWorkspaceSession
  ) => void;
  /** Called when any execution-bar task row should open its Agent task detail */
  onNavigateToTask?: (target: ExecutionBarTaskNavigationTarget) => void;
}

/**
 * Get status indicator color based on execution state
 */
const STATUS_COLORS = {
  running: getStatusIconConfig("executing").color,
  paused: getStatusIconConfig("paused").color,
  idle: getStatusIconConfig("backlog").color,
  ready: getStatusIconConfig("ready").color,
  pendingMerge: getStatusIconConfig("pending_merge").color,
  mergeAttention: getStatusIconConfig("merge_incomplete").color,
  stop: getStatusIconConfig("stopped").color,
} as const;
const POPOVER_ALIGN_TO_SEPARATOR_DOT = -20;

function getStatusColor(
  running: number,
  paused: boolean,
  haltMode: ExecutionHaltMode
): string {
  if (haltMode === "stopped") return STATUS_COLORS.stop;
  if (paused) return STATUS_COLORS.paused;
  if (running > 0) return STATUS_COLORS.running;
  return STATUS_COLORS.idle;
}

/**
 * Get status state for data attributes and animation class
 */
function getStatusState(running: number, paused: boolean): "running" | "paused" | "idle" {
  if (paused) return "paused";
  if (running > 0) return "running";
  return "idle";
}

function StatusSeparator() {
  return (
    <span
      aria-hidden="true"
      className="h-[3px] w-[3px] rounded-full"
      style={{ backgroundColor: "var(--border-strong, var(--border-default))" }}
    />
  );
}

export function ExecutionControlBar({
  projectId,
  statusKnown = true,
  runningCount,
  maxConcurrent,
  queuedCount,
  queuedMessageCount = 0,
  pausedCount = 0,
  pausedTasks = [],
  ideationMax = 0,
  mergingCount,
  mergeAttentionCount,
  hasAttentionMerges,
  mergePipelineData,
  isPaused,
  haltMode = isPaused ? "paused" : "running",
  isLoading = false,
  currentTaskName,
  onPauseToggle,
  onStop,
  runningProcesses = [],
  ideationSessions = [],
  workspaceSessions = [],
  lanes = [],
  capacity = null,
  onPauseProcess = () => {},
  onStopProcess = () => {},
  onOpenSettings = () => {},
  onNavigateToSession,
  onNavigateToWorkspace,
  onNavigateToTask,
}: ExecutionControlBarProps) {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const laneByName = new Map(lanes.map((lane) => [lane.lane, lane]));
  const workspaceLane = laneByName.get("workspaces");
  const taskLane = laneByName.get("tasks");
  const ideationLane = laneByName.get("ideation");
  const hasLaneUsage = lanes.length > 0;
  const displayRunningCount = capacity?.totalActive ?? runningCount;
  const displayMaxConcurrent = capacity?.globalMaxConcurrent ?? maxConcurrent;
  const workspaceActive = workspaceLane?.active ?? workspaceSessions.length;
  const taskActive = taskLane?.active ?? runningCount;
  const taskMax = taskLane?.max ?? maxConcurrent;
  const displayedIdeationMax = ideationLane?.max ?? ideationMax;
  const terminalStatusByConversationId = useAgentTerminalStore((state) => state.statusByConversationId);
  const terminalMetadataByConversationId = useAgentTerminalStore(
    (state) => state.metadataByConversationId
  );
  const projectsById = useProjectStore((state) => state.projects);
  const terminalSessions = useMemo<ExecutionBarTerminalSession[]>(() => {
    return Object.entries(terminalMetadataByConversationId)
      .flatMap(([conversationId, metadata]) => {
        const status = terminalStatusByConversationId[conversationId] ?? "closed";
        if (status === "closed") {
          return [];
        }
        const projectName = projectsById[metadata.projectId]?.name ?? "Unknown project";
        return [
          {
            conversationId,
            projectId: metadata.projectId,
            title: metadata.title?.trim() || `${projectName} terminal`,
            projectName,
            branchName: metadata.branchName,
            worktreePath: metadata.worktreePath,
            status,
          },
        ];
      })
      .sort((left, right) => left.title.localeCompare(right.title));
  }, [
    projectsById,
    terminalMetadataByConversationId,
    terminalStatusByConversationId,
  ]);
  const terminalCount = terminalSessions.length;
  const canStop = statusKnown && displayRunningCount > 0 && !isLoading;
  const isStopped = haltMode === "stopped";
  const canPauseToggle = statusKnown && !isLoading;
  const statusColor = getStatusColor(displayRunningCount, isPaused, haltMode);
  const statusState = isStopped ? "stopped" : getStatusState(displayRunningCount, isPaused);
  const isRunning = displayRunningCount > 0 && !isPaused;
  const openPopover = useUiStore((s) => s.executionBarOpenPopover);
  const activeTab = useUiStore((s) => s.executionBarRunningTab);
  const setOpenPopover = useUiStore((s) => s.setExecutionBarOpenPopover);
  const setActiveTab = useUiStore((s) => s.setExecutionBarRunningTab);
  const setPopoverOpen = (popover: Exclude<ExecutionBarPopoverKind, null>, open: boolean) => {
    if (open) {
      setOpenPopover(popover);
      return;
    }
    if (useUiStore.getState().executionBarOpenPopover === popover) {
      setOpenPopover(null);
    }
  };
  const openRunningPopover = (tab: ExecutionBarRunningTab) => {
    setActiveTab(tab);
    setOpenPopover("running");
  };

  // Responsive breakpoint tracking
  const [breakpoint, setBreakpoint] = useState<"wide" | "medium" | "narrow">("wide");

  useEffect(() => {
    const updateBreakpoint = () => {
      const width = window.innerWidth;
      if (width > 1200) {
        setBreakpoint("wide");
      } else if (width >= 800) {
        setBreakpoint("medium");
      } else {
        setBreakpoint("narrow");
      }
    };

    updateBreakpoint();
    window.addEventListener("resize", updateBreakpoint);
    return () => window.removeEventListener("resize", updateBreakpoint);
  }, []);

  // Label formatting based on breakpoint
  const runningLabel = breakpoint === "wide" ? "Running: " : breakpoint === "medium" ? "R: " : "";
  const queuedLabel = breakpoint === "wide" ? "Queued: " : breakpoint === "medium" ? "Q: " : "";
  const queuedMessageLabel =
    breakpoint === "wide" ? "Msgs: " : breakpoint === "medium" ? "Msg: " : "";
  const terminalsLabel = breakpoint === "wide" ? "Terminals: " : breakpoint === "medium" ? "Term: " : "";
  const pausedLabel = breakpoint === "wide" ? "Paused: " : breakpoint === "medium" ? "P: " : "";
  const mergingLabel = breakpoint === "wide" ? "Merge: " : breakpoint === "medium" ? "M: " : "";
  const mergeAttentionLabel = breakpoint === "wide" ? "Escalated: " : breakpoint === "medium" ? "E: " : "";
  const attentionCount = mergeAttentionCount ?? (hasAttentionMerges ? 1 : 0);
  const showAttentionCount = attentionCount > 0;
  const showMergeWorkCount = mergingCount > 0;

  // Only show ideation indicator when max > 0
  const showIdeation = displayedIdeationMax > 0;

  return (
    <TooltipProvider>
      <div
        data-testid="execution-control-shell"
        className="h-9"
        style={{
          backgroundColor: "var(--kanban-toolbar-bg)",
          borderTopColor: "var(--kanban-toolbar-border, #2E2E36)",
          borderTopStyle: "solid",
          borderTopWidth: "1px",
        }}
      >
        <div
          data-testid="execution-control-bar"
          data-paused={isPaused ? "true" : "false"}
          data-running={statusKnown ? displayRunningCount : undefined}
          data-loading={isLoading ? "true" : undefined}
          data-status={statusState}
          role="region"
          aria-label="Execution controls"
          aria-live="polite"
          className="z-10 flex h-full items-center justify-between px-4 text-[0.7188rem]"
          style={{
            backgroundColor: "transparent",
            borderRadius: 0,
            border: "none",
            borderStyle: "none",
            boxShadow: "none",
          }}
        >
        {/* Status Section (Left) */}
        <div
          className="flex items-center gap-5"
          aria-label={statusKnown ? `${displayRunningCount} agents running out of ${displayMaxConcurrent}, ${workspaceActive} workspace agents, ${taskActive} task agents, ${queuedCount} queued tasks, ${queuedMessageCount} queued messages, ${terminalCount} open terminals, ${pausedCount} paused, ${mergingCount} merge tasks, ${attentionCount} escalated merge tasks` : "Execution status unavailable"}
        >
          {/* Animated Status Indicator (anchor for all popovers) */}
          <div
            data-testid="status-indicator"
            className={cn(
              "h-[7px] w-[7px] rounded-full transition-colors duration-200",
              isRunning && "status-indicator-running"
            )}
            style={{ backgroundColor: statusColor }}
          />

          {/* Running Count (Clickable - opens popover) + Info Tooltip */}
          <div className="flex items-center gap-1.5">
            <RunningProcessPopover
              processes={runningProcesses}
              ideationSessions={ideationSessions}
              workspaceSessions={workspaceSessions}
              lanes={lanes}
              capacity={capacity}
              runningCount={displayRunningCount}
              maxConcurrent={taskMax}
              ideationMax={displayedIdeationMax}
              open={openPopover === "running"}
              onOpenChange={(open) => setPopoverOpen("running", open)}
              onPauseProcess={onPauseProcess}
              onStopProcess={onStopProcess}
              onOpenSettings={onOpenSettings}
              {...(onNavigateToSession !== undefined && { onNavigateToSession })}
              {...(onNavigateToWorkspace !== undefined && { onNavigateToWorkspace })}
              {...(onNavigateToTask !== undefined && { onNavigateToTask })}
              alignOffset={POPOVER_ALIGN_TO_SEPARATOR_DOT}
              initialTab={activeTab}
              showIdeation={showIdeation}
            >
              <button
                data-testid="running-count"
                className="inline-flex cursor-pointer items-center gap-1 transition-opacity hover:opacity-80"
                style={{ color: displayRunningCount > 0 ? STATUS_COLORS.running : "var(--text-muted)" }}
                onClick={() => openRunningPopover(hasLaneUsage ? "running" : "execution")}
              >
                <span>{runningLabel}</span>
                <span
                  style={{
                      color: displayRunningCount > 0 ? STATUS_COLORS.running : "var(--text-primary)",
                    fontFamily:
                      "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                    fontWeight: 500,
                  }}
                >
                  {statusKnown ? `${displayRunningCount}/${displayMaxConcurrent}` : "—"}
                </span>
              </button>
            </RunningProcessPopover>
            <InfoTooltip
              testId="running-info-tooltip"
              content={
                <div className="space-y-2">
                  <div>
                    <strong className="block mb-1" style={{ color: "var(--text-primary)" }}>
                      Concurrent Execution
                    </strong>
                    <p style={{ color: "var(--text-secondary)" }}>
                      Active agents across capacity lanes. Current global limit is{" "}
                      <strong>{displayMaxConcurrent}</strong>.
                    </p>
                  </div>
                  <div>
                    <p style={{ color: "var(--text-secondary)" }}>
                      Priority: Workspaces, then Tasks, then Ideation.
                    </p>
                  </div>
                  <div className="pt-1 border-t" style={{ borderColor: "var(--overlay-weak)" }}>
                    <p className="text-xs" style={{ color: "var(--text-secondary)" }}>
                      Change limits → Settings
                    </p>
                  </div>
                </div>
              }
            />
          </div>

          {/* Separator */}
          <StatusSeparator />

          {/* Queued Count (Clickable Popover) + Info Tooltip */}
          <div className="flex items-center gap-1.5">
            <QueuedTasksPopover
              projectId={projectId}
              queuedCount={queuedCount}
              alignOffset={POPOVER_ALIGN_TO_SEPARATOR_DOT}
              open={openPopover === "queued"}
              onOpenChange={(open) => setPopoverOpen("queued", open)}
              {...(onNavigateToTask !== undefined && { onNavigateToTask })}
            >
              <button
                data-testid="queued-count"
                className="inline-flex cursor-pointer items-center gap-1 transition-opacity hover:opacity-80"
                style={{ color: "var(--text-muted)" }}
                aria-label="View queued tasks"
                aria-haspopup="dialog"
              >
                <span>{queuedLabel}</span>
                <span
                  style={{
                    color: queuedCount > 0 ? STATUS_COLORS.ready : "var(--text-primary)",
                    fontFamily:
                      "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                    fontWeight: 500,
                  }}
                >
                  {statusKnown ? queuedCount : "—"}
                </span>
              </button>
            </QueuedTasksPopover>
            <InfoTooltip
              testId="queued-info-tooltip"
              content={
                <div className="space-y-2">
                  <div>
                    <strong className="block mb-1" style={{ color: "var(--text-primary)" }}>
                      Task Queue
                    </strong>
                    <p style={{ color: "var(--text-secondary)" }}>
                      Tasks in "ready" status waiting for an open execution slot.
                      Processed by priority then age (oldest first).
                    </p>
                  </div>
                  <div>
                    <p style={{ color: "var(--text-secondary)" }}>
                      Blocked tasks are NOT counted here.
                    </p>
                  </div>
                  {queuedMessageCount > 0 && (
                    <div>
                      <strong className="block mb-1" style={{ color: "var(--text-primary)" }}>
                        Pending Agent Messages
                      </strong>
                      <p style={{ color: "var(--text-secondary)" }}>
                        {queuedMessageCount} prompt{queuedMessageCount === 1 ? "" : "s"} currently held by
                        pause/capacity barriers. They relaunch automatically on resume or when
                        capacity opens.
                      </p>
                    </div>
                  )}
                </div>
              }
            />
          </div>

          {queuedMessageCount > 0 && (
            <>
              <StatusSeparator />
              <div
                data-testid="queued-message-count"
                className="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[0.7188rem]"
                style={{
                  color: "var(--status-warning)",
                  backgroundColor: "var(--status-warning-muted)",
                  borderColor: "var(--status-warning-border)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                }}
                aria-label={`${queuedMessageCount} queued agent messages held by pause or capacity barriers`}
                title="Queued agent messages held by pause/capacity barriers"
              >
                <AlertTriangle className="h-3.5 w-3.5" />
                <span>
                  {queuedMessageLabel}
                  {queuedMessageCount}
                </span>
              </div>
            </>
          )}

          {/* Paused Count (Clickable Popover) - only visible when > 0 */}
          {pausedCount > 0 && (
            <>
              <StatusSeparator />
              <PausedTasksPopover
                pausedTasks={pausedTasks}
                alignOffset={POPOVER_ALIGN_TO_SEPARATOR_DOT}
                open={openPopover === "paused"}
                onOpenChange={(open) => setPopoverOpen("paused", open)}
                {...(onNavigateToTask !== undefined && { onNavigateToTask })}
              >
                <button
                  data-testid="paused-count"
                  className="inline-flex cursor-pointer items-center gap-1 transition-opacity hover:opacity-80"
                  style={{ color: "var(--text-muted)" }}
                  aria-label="View paused tasks"
                  aria-haspopup="dialog"
                >
                  <span>{pausedLabel}</span>
                  <span
                    style={{
                      color: STATUS_COLORS.paused,
                      fontFamily:
                        "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                      fontWeight: 500,
                    }}
                  >
                    {pausedCount}
                  </span>
                </button>
              </PausedTasksPopover>
            </>
          )}

          {/* Merging Count with Popover */}
          {(showMergeWorkCount || showAttentionCount) && (
            <>
              <StatusSeparator />
              {mergePipelineData ? (
                <MergePipelinePopover
                  active={mergePipelineData.active}
                  waiting={mergePipelineData.waiting}
                  needsAttention={mergePipelineData.needsAttention}
                  runningCount={displayRunningCount}
                  alignOffset={POPOVER_ALIGN_TO_SEPARATOR_DOT}
                  open={openPopover === "merge"}
                  onOpenChange={(open) => setPopoverOpen("merge", open)}
                  {...(onNavigateToTask !== undefined && { onNavigateToTask })}
                >
                  <button
                    data-testid="merging-count"
                    className="flex cursor-pointer items-center gap-1.5 transition-opacity hover:opacity-80"
                    style={{ color: "var(--text-muted)" }}
                  >
                    {showMergeWorkCount && (
                      <span className="inline-flex items-center gap-1">
                        <span>{mergingLabel}</span>
                        <span
                          style={{
                            color: STATUS_COLORS.pendingMerge,
                            fontFamily:
                              "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                            fontWeight: 500,
                          }}
                        >
                          {mergingCount}
                        </span>
                      </span>
                    )}
                    {showAttentionCount && (
                      <span
                        data-testid="merge-attention-count"
                        className="inline-flex items-center gap-1"
                        style={{ color: STATUS_COLORS.mergeAttention }}
                      >
                        <AlertTriangle className="h-3 w-3" />
                        <span>{mergeAttentionLabel}</span>
                        <span
                          style={{
                            fontFamily:
                              "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                            fontWeight: 500,
                          }}
                        >
                          {attentionCount}
                        </span>
                      </span>
                    )}
                  </button>
                </MergePipelinePopover>
              ) : (
                <span
                  data-testid="merging-count"
                  className="flex items-center gap-1.5"
                  style={{ color: "var(--text-muted)" }}
                >
                  {showMergeWorkCount && (
                    <span className="inline-flex items-center gap-1">
                      <span>{mergingLabel}</span>
                      <span
                        style={{
                          color: STATUS_COLORS.pendingMerge,
                          fontFamily:
                            "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                          fontWeight: 500,
                        }}
                      >
                        {mergingCount}
                      </span>
                    </span>
                  )}
                  {showAttentionCount && (
                    <span
                      data-testid="merge-attention-count"
                      className="inline-flex items-center gap-1"
                      style={{ color: STATUS_COLORS.mergeAttention }}
                    >
                      <AlertTriangle className="h-3 w-3" />
                      <span>{mergeAttentionLabel}</span>
                      <span
                        style={{
                          fontFamily:
                            "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                          fontWeight: 500,
                        }}
                      >
                        {attentionCount}
                      </span>
                    </span>
                  )}
                </span>
              )}
            </>
          )}

          {/* Terminal sessions are host-local (2.6-a) — no remote entry point. */}
          {terminalCount > 0 && !isRemoteEnvironment && (
            <>
              <StatusSeparator />
              <TerminalsPopover
                sessions={terminalSessions}
                open={openPopover === "terminals"}
                onOpenChange={(open) => setPopoverOpen("terminals", open)}
                {...(onNavigateToWorkspace !== undefined && { onNavigateToWorkspace })}
                alignOffset={POPOVER_ALIGN_TO_SEPARATOR_DOT}
              >
                <button
                  data-testid="terminals-count"
                  className="inline-flex cursor-pointer items-center gap-1 transition-opacity hover:opacity-80"
                  style={{ color: "var(--text-muted)" }}
                  aria-label={`View ${terminalCount} open agent terminal${terminalCount === 1 ? "" : "s"}`}
                  aria-haspopup="dialog"
                >
                  <TerminalIcon
                    className="h-3.5 w-3.5"
                    style={{ color: "var(--accent-primary)" }}
                  />
                  <span>{terminalsLabel}</span>
                  <span
                    style={{
                      color: "var(--text-primary)",
                      fontFamily:
                        "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                      fontWeight: 500,
                    }}
                  >
                    {terminalCount}
                  </span>
                </button>
              </TerminalsPopover>
            </>
          )}
        </div>

        {/* Progress Section (Center) - Conditional */}
        {isRunning && currentTaskName && (
          <div
            data-testid="current-task"
            className="flex items-center gap-2 max-w-[40%] task-name-enter"
          >
            <Loader2
              className="w-4 h-4 animate-spin shrink-0"
              style={{ color: STATUS_COLORS.running }}
            />
            <span
              className="text-[0.8125rem] truncate"
              style={{ color: "var(--text-secondary)" }}
            >
              {currentTaskName}
            </span>
          </div>
        )}

        {/* Control Section (Right) */}
        <div className="flex items-center gap-1.5">
          {/* Pause/Resume Button */}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                data-testid="pause-toggle-button"
                variant="ghost"
                size="default"
                onClick={onPauseToggle}
                disabled={!canPauseToggle}
                aria-label={isStopped ? "Start execution" : isPaused ? "Resume execution" : "Pause execution"}
                aria-pressed={isPaused && !isStopped}
                className="h-6 gap-1.5 rounded-[6px] px-2.5 text-[0.7188rem] font-medium transition-all duration-150 active:scale-[0.98]"
                style={{
                  backgroundColor: "var(--bg-elevated)",
                  borderColor: "var(--border-default)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                  color: isStopped
                    ? "var(--accent-primary)"
                    : isPaused
                      ? STATUS_COLORS.paused
                      : "var(--text-primary)",
                  opacity: canPauseToggle ? 1 : 0.55,
                }}
              >
                {isLoading ? (
                  <Loader2 className="h-[11px] w-[11px] animate-spin" />
                ) : isStopped ? (
                  <Play className="h-[11px] w-[11px]" />
                ) : isPaused ? (
                  <Play className="h-[11px] w-[11px]" />
                ) : (
                  <Pause className="h-[11px] w-[11px]" />
                )}
                <span className="hidden sm:inline">
                  {isStopped ? "Start" : isPaused ? "Resume" : "Pause"}
                </span>
              </Button>
            </TooltipTrigger>
            <TooltipContent side="top">
              <p>
                {isStopped
                  ? "Start execution again. Stopped tasks remain stopped until you restart them."
                  : isPaused
                  ? "Resume paused tasks and queue ⌘P"
                  : "Pause execution (running tasks will pause) ⌘P"}
              </p>
            </TooltipContent>
          </Tooltip>

          {/* Stop Button */}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                data-testid="stop-button"
                variant="ghost"
                size="default"
                onClick={onStop}
                disabled={!canStop}
                aria-label={isStopped ? "Execution already stopped" : "Stop all running tasks"}
                aria-disabled={!canStop}
                className="h-6 gap-1.5 rounded-[6px] px-2.5 text-[0.7188rem] font-medium transition-all duration-150 active:scale-[0.98]"
                style={{
                  backgroundColor: "var(--bg-elevated)",
                  borderColor: "var(--border-default)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                  color: canStop ? STATUS_COLORS.stop : "var(--text-muted)",
                  opacity: canStop ? 1 : 0.55,
                }}
              >
                <Square className="h-[11px] w-[11px] fill-current" />
                <span className="hidden sm:inline">Stop</span>
              </Button>
            </TooltipTrigger>
            <TooltipContent side="top">
              <p>
                {isStopped
                  ? "Execution is halted. Press Start or restart a task to run ready work."
                  : canStop
                  ? "Stop all running tasks (manual restart required) ⌘⇧S"
                  : "No tasks currently running"}
              </p>
            </TooltipContent>
          </Tooltip>
        </div>
        </div>
      </div>
    </TooltipProvider>
  );
}
