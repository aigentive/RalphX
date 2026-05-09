/**
 * RunningProcessPopover - Compact running processes list with tabbed view
 *
 * Dense row-based layout matching macOS Activity Monitor style.
 * Tabs: Execution (processes + team groups) | Ideation (ideation sessions)
 * Controlled mode: uses PopoverAnchor (not PopoverTrigger) for external open control.
 */

import { useEffect, useState } from "react";
import {
  Popover,
  PopoverContent,
  PopoverAnchor,
} from "@/components/ui/popover";
import { Settings } from "lucide-react";
import { ProcessCard } from "./ProcessCard";
import { TeamProcessGroup } from "./TeamProcessGroup";
import { IdeationSessionCard } from "./IdeationSessionCard";
import type { RunningProcess, RunningIdeationSession } from "@/api/running-processes";
import { useUiStore } from "@/stores/uiStore";
import { cn } from "@/lib/utils";

type TabType = "execution" | "ideation";

interface RunningProcessPopoverProps {
  /** List of currently running processes */
  processes: RunningProcess[];
  /** List of running ideation sessions */
  ideationSessions?: RunningIdeationSession[];
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
  /** Children (anchor element — NOT a trigger, controlled externally) */
  children: React.ReactNode;
  /** Optional horizontal alignment offset for popover content */
  alignOffset?: number;
  /** Initial tab to show — synced on every change to allow pre-selection and external switching */
  initialTab?: TabType;
  /** Whether to show the Ideation tab (false hides it entirely when ideationMax=0) */
  showIdeation?: boolean;
  /** Whether execution team-specific UI should be shown. */
  showExecutionTeamUi?: boolean;
  /** Whether ideation team-specific UI should be shown. */
  showIdeationTeamUi?: boolean;
}

export function RunningProcessPopover({
  processes,
  ideationSessions = [],
  runningCount,
  maxConcurrent,
  ideationMax = 0,
  open,
  onOpenChange,
  onPauseProcess,
  onStopProcess,
  onOpenSettings,
  onNavigateToSession,
  children,
  alignOffset = -24,
  initialTab = "execution",
  showIdeation = false,
  showExecutionTeamUi = true,
  showIdeationTeamUi = true,
}: RunningProcessPopoverProps) {
  const [activeTab, setActiveTab] = useState<TabType>(initialTab);
  const navigateToTask = useUiStore((s) => s.navigateToTask);

  // Sync tab whenever initialTab changes — handles external switching while popover is open
  useEffect(() => {
    setActiveTab(initialTab);
  }, [initialTab]);

  const activeIdeationCount = ideationSessions.filter((s) => s.isGenerating).length;
  const effectiveRunningCount = runningCount ?? processes.length;

  const handleNavigate = (taskId: string) => {
    onOpenChange(false);
    navigateToTask(taskId);
  };

  const handleNavigateToSession = (sessionId: string) => {
    onOpenChange(false);
    onNavigateToSession?.(sessionId);
  };

  // Tab-aware header title
  const headerTitle =
    activeTab === "execution" || !showIdeation
      ? `Execution (${effectiveRunningCount}/${maxConcurrent})`
      : `Ideation (${activeIdeationCount}/${ideationMax})`;

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
        {processes.map((process) =>
          process.teamName && showExecutionTeamUi ? (
            <TeamProcessGroup
              key={process.taskId}
              process={process}
              onPause={onPauseProcess}
              onStop={onStopProcess}
              onNavigate={handleNavigate}
            />
          ) : (
            <ProcessCard
              key={process.taskId}
              process={process}
              onPause={onPauseProcess}
              onStop={onStopProcess}
              onNavigate={handleNavigate}
            />
          )
        )}
      </>
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
            showTeamModeBadge={showIdeationTeamUi}
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
          // Prevent Radix outside-click dismissal when clicking the ideation trigger button
          // This avoids close→reopen flicker when switching tabs via the external ideation button
          const target = e.target as HTMLElement;
          if (target.closest("[data-ideation-trigger]")) {
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
              Max: {activeTab === "ideation" && showIdeation ? ideationMax : maxConcurrent}
            </button>
          </div>

          {/* Tab bar — only rendered when ideation is enabled */}
          {showIdeation && (
            <div role="tablist" className="flex items-center gap-1">
              <button
                role="tab"
                aria-selected={activeTab === "execution"}
                onClick={() => setActiveTab("execution")}
                className={cn(
                  "px-2.5 py-0.5 rounded-full text-[0.6875rem] font-medium transition-colors"
                )}
                style={
                  activeTab === "execution"
                    ? { backgroundColor: "var(--accent-primary)", color: "white" }
                    : { color: "var(--text-muted)" }
                }
              >
                Execution ({processes.length})
              </button>
              <button
                role="tab"
                aria-selected={activeTab === "ideation"}
                onClick={() => setActiveTab("ideation")}
                className={cn(
                  "px-2.5 py-0.5 rounded-full text-[0.6875rem] font-medium transition-colors"
                )}
                style={
                  activeTab === "ideation"
                    ? { backgroundColor: "var(--accent-primary)", color: "white" }
                    : { color: "var(--text-muted)" }
                }
              >
                Ideation ({ideationSessions.length})
              </button>
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
          {showIdeation ? (
            activeTab === "execution" ? executionContent : ideationContent
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
            {activeTab === "ideation" && showIdeation
              ? `Ideation capacity: up to ${ideationMax} sessions.`
              : `Concurrency runs up to ${maxConcurrent} tasks in parallel.`}
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
