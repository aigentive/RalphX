/**
 * RunningProcessPopover - Compact running processes list with tabbed view
 *
 * Dense row-based layout matching macOS Activity Monitor style.
 * Tabs: Execution (processes) | Ideation (ideation sessions)
 * Controlled mode: uses PopoverAnchor (not PopoverTrigger) for external open control.
 */

import { useEffect, useState } from "react";
import {
  Popover,
  PopoverContent,
  PopoverAnchor,
} from "@/components/ui/popover";
import { Loader2, MessageSquare, Settings } from "lucide-react";
import { ProcessCard } from "./ProcessCard";
import { IdeationSessionCard } from "./IdeationSessionCard";
import type {
  ExecutionCapacitySummary,
  ExecutionLaneUsage,
  ExecutionLaneName,
  RunningProcess,
  RunningIdeationSession,
  RunningWorkspaceSession,
} from "@/api/running-processes";
import { cn } from "@/lib/utils";
import { useElapsedTimer } from "@/hooks/useElapsedTimer";
import { formatElapsedTime } from "@/lib/formatters";
import { shouldPreserveExecutionPopoverForTarget } from "./executionPopoverDismissal";
import {
  runningProcessTaskTarget,
  type ExecutionBarTaskNavigationTarget,
} from "./executionTaskNavigation";

type TabType = "running" | "workspaces" | "execution" | "ideation";

interface RunningProcessPopoverProps {
  /** List of currently running processes */
  processes: RunningProcess[];
  /** List of running ideation sessions */
  ideationSessions?: RunningIdeationSession[];
  /** List of running workspace conversations */
  workspaceSessions?: RunningWorkspaceSession[];
  /** Lane-level usage from the backend */
  lanes?: ExecutionLaneUsage[];
  /** Capacity summary from the backend */
  capacity?: ExecutionCapacitySummary | null;
  /** Global running count from execution status (source of truth for capacity) */
  runningCount?: number;
  /** Current max concurrent tasks */
  maxConcurrent: number;
  /** Maximum concurrent ideation sessions */
  ideationMax?: number;
  /** Whether popover is open (controlled) */
  open: boolean;
  /** Called when open state changes */
  onOpenChange: (open: boolean) => void;
  /** Called when pause button clicked for a process */
  onPauseProcess: (taskId: string) => void;
  /** Called when stop button clicked for a process */
  onStopProcess: (taskId: string) => void;
  /** Called when settings link clicked */
  onOpenSettings: () => void;
  /** Called when an ideation session is clicked to navigate to it */
  onNavigateToSession?: (sessionId: string) => void;
  /** Called when a workspace session is clicked to navigate to its agent conversation */
  onNavigateToWorkspace?: (
    projectId: string,
    conversationId: string,
    session?: RunningWorkspaceSession
  ) => void;
  /** Called when a task row should open its Agent conversation task detail */
  onNavigateToTask?: (target: ExecutionBarTaskNavigationTarget) => void;
  /** Children (anchor element — NOT a trigger, controlled externally) */
  children: React.ReactNode;
  /** Optional horizontal alignment offset for popover content */
  alignOffset?: number;
  /** Initial tab to show — synced on every change to allow pre-selection and external switching */
  initialTab?: TabType;
  /** Whether to show the Ideation tab (false hides it entirely when ideationMax=0) */
  showIdeation?: boolean;
}

const LANE_LABELS: Record<ExecutionLaneName, string> = {
  workspaces: "Workspaces",
  tasks: "Tasks",
  ideation: "Ideation",
};

const LANE_TO_TAB: Record<ExecutionLaneName, TabType> = {
  workspaces: "workspaces",
  tasks: "execution",
  ideation: "ideation",
};

function WorkspaceSessionRow({
  session,
  onClick,
}: {
  session: RunningWorkspaceSession;
  onClick?: () => void;
}) {
  const elapsedTime = useElapsedTimer(session.elapsedSeconds, session.conversationId);
  const className =
    "w-full px-2 py-1.5 rounded-md transition-colors hover:bg-[var(--overlay-faint)]";
  const content = (
    <>
      <div className="flex items-center gap-2">
        <Loader2
          className="w-3.5 h-3.5 animate-spin shrink-0"
          style={{ color: "var(--accent-primary)" }}
        />
        <span
          className="min-w-0 flex-1 truncate text-left text-xs font-medium"
          style={{ color: "var(--text-primary)" }}
          title={session.title}
        >
          {session.title}
        </span>
        <span
          className="shrink-0 rounded px-1.5 py-0.5 text-[0.625rem] font-medium"
          style={{
            color: "var(--accent-primary)",
            backgroundColor: "var(--accent-muted)",
          }}
        >
          Workspace
        </span>
      </div>
      <div
        className="mt-0.5 flex min-w-0 items-center gap-1.5 pl-[22px] text-[0.6875rem]"
        style={{ color: "var(--text-muted)" }}
      >
        <MessageSquare className="h-3 w-3 shrink-0" style={{ color: "var(--text-muted)" }} />
        <span className="shrink-0 tabular-nums">{formatElapsedTime(elapsedTime)}</span>
        {session.model && (
          <>
            <span className="shrink-0" style={{ color: "var(--text-muted)" }}>
              ·
            </span>
            <span className="min-w-0 truncate">{session.model}</span>
          </>
        )}
      </div>
    </>
  );

  if (onClick) {
    return (
      <button
        type="button"
        data-testid={`workspace-card-${session.conversationId}`}
        className={cn(className, "text-left")}
        onClick={onClick}
      >
        {content}
      </button>
    );
  }

  return (
    <div
      data-testid={`workspace-card-${session.conversationId}`}
      className={className}
    >
      {content}
    </div>
  );
}

export function RunningProcessPopover({
  processes,
  ideationSessions = [],
  workspaceSessions = [],
  lanes = [],
  capacity = null,
  runningCount,
  maxConcurrent,
  ideationMax = 0,
  open,
  onOpenChange,
  onPauseProcess,
  onStopProcess,
  onOpenSettings,
  onNavigateToSession,
  onNavigateToWorkspace,
  onNavigateToTask,
  children,
  alignOffset = -24,
  initialTab = "execution",
  showIdeation = false,
}: RunningProcessPopoverProps) {
  const [activeTab, setActiveTab] = useState<TabType>(initialTab);

  // Sync tab whenever initialTab changes — handles external switching while popover is open
  useEffect(() => {
    setActiveTab(initialTab);
  }, [initialTab]);

  const activeIdeationCount = ideationSessions.filter((s) => s.isGenerating).length;
  const laneByName = new Map(lanes.map((lane) => [lane.lane, lane]));
  const workspaceLane = laneByName.get("workspaces");
  const taskLane = laneByName.get("tasks");
  const ideationLane = laneByName.get("ideation");
  const hasLaneUsage = lanes.length > 0;
  const effectiveRunningCount = capacity?.totalActive ?? runningCount ?? processes.length;
  const effectiveMaxConcurrent = capacity?.globalMaxConcurrent ?? maxConcurrent;

  const handleNavigate = (taskId: string) => {
    const process = processes.find((item) => item.taskId === taskId);
    onOpenChange(false);
    if (process) {
      onNavigateToTask?.(runningProcessTaskTarget(process));
    }
  };

  const handleNavigateToSession = (sessionId: string) => {
    onOpenChange(false);
    onNavigateToSession?.(sessionId);
  };

  const handleNavigateToWorkspace = (session: RunningWorkspaceSession) => {
    onOpenChange(false);
    onNavigateToWorkspace?.(session.projectId, session.conversationId, session);
  };

  // Tab-aware header title
  const headerTitle =
    activeTab === "running" && hasLaneUsage
      ? `Running (${effectiveRunningCount}/${effectiveMaxConcurrent})`
      : activeTab === "workspaces" && hasLaneUsage
        ? `Workspaces (${workspaceLane?.active ?? workspaceSessions.length}/${workspaceLane?.max ?? 10})`
        : activeTab === "ideation" && showIdeation
          ? `Ideation (${ideationLane?.active ?? activeIdeationCount}/${ideationLane?.max ?? ideationMax})`
          : `${hasLaneUsage ? "Tasks" : "Execution"} (${taskLane?.active ?? processes.length}/${taskLane?.max ?? maxConcurrent})`;

  const activeLaneMax =
    activeTab === "running" && hasLaneUsage
      ? effectiveMaxConcurrent
      : activeTab === "workspaces" && hasLaneUsage
        ? workspaceLane?.max ?? 10
        : activeTab === "ideation" && showIdeation
          ? ideationLane?.max ?? ideationMax
          : taskLane?.max ?? maxConcurrent;

  const tabButtonStyle = (selected: boolean) =>
    selected
      ? { backgroundColor: "var(--accent-primary)", color: "white" }
      : { color: "var(--text-muted)" };

  // Content for the execution tab
  const executionContent =
    processes.length === 0 ? (
      <div
        className="py-6 text-center text-xs"
        style={{ color: "var(--text-muted)" }}
      >
        No active execution processes
      </div>
    ) : (
      <>
        {processes.map((process) => (
          <ProcessCard
            key={process.taskId}
            process={process}
            onPause={onPauseProcess}
            onStop={onStopProcess}
            onNavigate={handleNavigate}
          />
        ))}
      </>
    );

  const workspaceContent =
    workspaceSessions.length === 0 ? (
      <div
        className="py-6 text-center text-xs"
        style={{ color: "var(--text-muted)" }}
      >
        No active workspace agents
      </div>
    ) : (
      <>
        {workspaceSessions.map((session) => (
          <WorkspaceSessionRow
            key={session.conversationId}
            session={session}
            {...(onNavigateToWorkspace
              ? { onClick: () => handleNavigateToWorkspace(session) }
              : {})}
          />
        ))}
      </>
    );

  const runningContent =
    lanes.length === 0 ? (
      executionContent
    ) : (
      <div className="space-y-1">
        {lanes
          .slice()
          .sort((left, right) => left.priorityRank - right.priorityRank)
          .map((lane) => (
            <button
              type="button"
              key={lane.lane}
              data-testid={`capacity-lane-${lane.lane}`}
              className="w-full rounded-md px-2 py-1.5 text-left transition-colors hover:bg-[var(--overlay-weak)]"
              style={{ backgroundColor: "var(--overlay-faint)" }}
              onClick={() => setActiveTab(LANE_TO_TAB[lane.lane])}
              disabled={lane.lane === "ideation" && !showIdeation}
            >
              <div className="flex items-center justify-between gap-3">
                <span className="text-xs font-medium" style={{ color: "var(--text-primary)" }}>
                  {LANE_LABELS[lane.lane]}
                </span>
                <span
                  className="text-xs tabular-nums"
                  style={{
                    color: lane.active > 0 ? "var(--accent-primary)" : "var(--text-secondary)",
                    fontFamily:
                      "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                  }}
                >
                  {lane.active}/{lane.max}
                </span>
              </div>
              <div className="mt-1 flex items-center gap-2 text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
                {lane.idle > 0 && <span>{lane.idle} idle</span>}
                {lane.waiting > 0 && <span>{lane.waiting} waiting</span>}
                {lane.borrowed > 0 && <span>{lane.borrowed} borrowed</span>}
                {lane.idle === 0 && lane.waiting === 0 && lane.borrowed === 0 && (
                  <span>No pressure</span>
                )}
              </div>
            </button>
          ))}
      </div>
    );

  // Content for the ideation tab
  const ideationContent =
    ideationSessions.length === 0 ? (
      <div
        className="py-6 text-center text-xs"
        style={{ color: "var(--text-muted)" }}
      >
        No active ideation sessions
      </div>
    ) : (
      <>
        {ideationSessions.map((session) => (
          <IdeationSessionCard
            key={session.sessionId}
            session={session}
            onClick={() => handleNavigateToSession(session.sessionId)}
          />
        ))}
      </>
    );

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverAnchor asChild>{children}</PopoverAnchor>
      <PopoverContent
        data-testid="running-process-popover"
        align="start"
        alignOffset={alignOffset}
        side="top"
        sideOffset={24}
        className="w-[420px] p-0 border-0"
        style={{
          backgroundColor: "var(--bg-surface)",
          border: "1px solid var(--overlay-weak)",
          borderRadius: "10px",
          boxShadow:
            "0 4px 16px var(--overlay-scrim), 0 12px 32px var(--overlay-scrim)",
        }}
        onInteractOutside={(e) => {
          // Preserve execution popovers while switching agent conversations from the sidebar.
          // The sidebar click updates the footer scope; it should not clear the selected popover.
          const target = e.target;
          if (shouldPreserveExecutionPopoverForTarget(target)) {
            e.preventDefault();
            return;
          }

          // Prevent Radix outside-click dismissal when clicking the ideation trigger button
          // This avoids close→reopen flicker when switching tabs via the external ideation button
          if (target instanceof HTMLElement && target.closest("[data-ideation-trigger]")) {
            e.preventDefault();
          }
        }}
      >
        {/* Header */}
        <div
          className="px-3 py-2.5"
          style={{
            borderBottom: "1px solid var(--overlay-weak)",
          }}
        >
          {/* Top row: tab-aware title + settings */}
          <div className="flex items-center justify-between mb-2">
            <h3
              className="text-xs font-semibold"
              style={{ color: "var(--text-secondary)" }}
            >
              {headerTitle}
            </h3>

            <button
              data-testid="open-settings-button"
              onClick={onOpenSettings}
              className={cn(
                "flex items-center gap-1 px-1.5 py-0.5 rounded text-[0.6875rem]",
                "transition-colors hover:bg-white/[0.05]"
              )}
              style={{ color: "var(--text-muted)" }}
            >
              <Settings className="w-3 h-3" />
              Max: {activeLaneMax}
            </button>
          </div>

          {/* Tab bar — rendered when multiple execution lanes are available */}
          {(hasLaneUsage || showIdeation) && (
            <div role="tablist" className="flex items-center gap-1">
              {hasLaneUsage && (
                <>
                  <button
                    role="tab"
                    aria-selected={activeTab === "running"}
                    onClick={() => setActiveTab("running")}
                    className={cn(
                      "px-2.5 py-0.5 rounded-full text-[0.6875rem] font-medium transition-colors"
                    )}
                    style={tabButtonStyle(activeTab === "running")}
                  >
                    Running ({effectiveRunningCount})
                  </button>
                  <button
                    role="tab"
                    aria-selected={activeTab === "workspaces"}
                    onClick={() => setActiveTab("workspaces")}
                    className={cn(
                      "px-2.5 py-0.5 rounded-full text-[0.6875rem] font-medium transition-colors"
                    )}
                    style={tabButtonStyle(activeTab === "workspaces")}
                  >
                    Workspaces ({workspaceLane?.active ?? workspaceSessions.length})
                  </button>
                </>
              )}
              <button
                role="tab"
                aria-selected={activeTab === "execution"}
                onClick={() => setActiveTab("execution")}
                className={cn(
                  "px-2.5 py-0.5 rounded-full text-[0.6875rem] font-medium transition-colors"
                )}
                style={tabButtonStyle(activeTab === "execution")}
              >
                {hasLaneUsage ? "Tasks" : "Execution"} ({taskLane?.active ?? processes.length})
              </button>
              {showIdeation && (
                <button
                  role="tab"
                  aria-selected={activeTab === "ideation"}
                  onClick={() => setActiveTab("ideation")}
                  className={cn(
                    "px-2.5 py-0.5 rounded-full text-[0.6875rem] font-medium transition-colors"
                  )}
                  style={tabButtonStyle(activeTab === "ideation")}
                >
                  Ideation ({ideationLane?.active ?? ideationSessions.length})
                </button>
              )}
            </div>
          )}
        </div>

        {/* Tab content panel */}
        <div
          role="tabpanel"
          className="max-h-[320px] overflow-y-auto p-1.5"
          style={{
            scrollbarWidth: "thin",
            scrollbarColor: "var(--overlay-moderate) transparent",
          }}
        >
          {hasLaneUsage || showIdeation ? (
            activeTab === "running" && hasLaneUsage
              ? runningContent
              : activeTab === "workspaces" && hasLaneUsage
                ? workspaceContent
                : activeTab === "ideation" && showIdeation
                  ? ideationContent
                  : executionContent
          ) : (
            executionContent
          )}
        </div>

        {/* Footer — tab-aware capacity text */}
        <div
          className="flex items-center justify-between px-3 py-2 text-[0.6875rem]"
          style={{
            borderTop: "1px solid var(--overlay-weak)",
            color: "var(--text-muted)",
          }}
        >
          <span>
            {activeTab === "running" && hasLaneUsage
              ? `Priority: ${capacity?.priority.map((lane) => LANE_LABELS[lane]).join(" > ") ?? "Workspaces > Tasks > Ideation"}. Borrowing ${capacity?.borrowingEnabled ? "enabled" : "disabled"}.`
              : activeTab === "workspaces" && hasLaneUsage
                ? `Workspace capacity: up to ${activeLaneMax} main agents.`
                : activeTab === "ideation" && showIdeation
                  ? `Ideation capacity: up to ${activeLaneMax} sessions.`
                  : `Concurrency runs up to ${activeLaneMax} tasks in parallel.`}
          </span>
          <button
            onClick={onOpenSettings}
            className="hover:underline transition-colors shrink-0 ml-2"
            style={{ color: "var(--accent-primary)" }}
          >
            Open Settings
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
