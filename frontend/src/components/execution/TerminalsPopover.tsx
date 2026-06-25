import { FolderOpen, GitBranch, Terminal } from "lucide-react";

import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { compactTerminalPath } from "@/components/agents/agentTerminalPaths";
import type { AgentTerminalCachedStatus } from "@/components/agents/agentTerminalStore";
import { cn } from "@/lib/utils";
import { shouldPreserveExecutionPopoverForTarget } from "./executionPopoverDismissal";

export interface ExecutionBarTerminalSession {
  conversationId: string;
  projectId: string;
  title: string;
  projectName: string;
  branchName: string | null;
  worktreePath: string | null;
  status: AgentTerminalCachedStatus;
}

interface TerminalsPopoverProps {
  sessions: ExecutionBarTerminalSession[];
  children: React.ReactNode;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigateToWorkspace?: (projectId: string, conversationId: string) => void;
  alignOffset?: number;
}

const STATUS_LABELS: Record<AgentTerminalCachedStatus, string> = {
  closed: "Open",
  running: "Running",
  exited: "Exited",
  error: "Error",
};

const STATUS_COLORS: Record<AgentTerminalCachedStatus, string> = {
  closed: "var(--text-muted)",
  running: "var(--accent-primary)",
  exited: "var(--text-muted)",
  error: "var(--status-error)",
};

function TerminalSessionRow({
  session,
  onClick,
}: {
  session: ExecutionBarTerminalSession;
  onClick?: () => void;
}) {
  const displayPath = session.worktreePath
    ? compactTerminalPath(session.worktreePath)
    : null;
  const className =
    "w-full rounded-md px-2 py-1.5 transition-colors hover:bg-[var(--overlay-faint)]";
  const content = (
    <>
      <div className="flex items-center gap-2">
        <Terminal
          className="h-3.5 w-3.5 shrink-0"
          style={{ color: STATUS_COLORS[session.status] }}
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
            color: STATUS_COLORS[session.status],
            backgroundColor: "var(--overlay-faint)",
          }}
        >
          {STATUS_LABELS[session.status]}
        </span>
      </div>
      <div
        className="mt-0.5 flex min-w-0 items-center gap-1.5 pl-[22px] text-[0.6875rem]"
        style={{ color: "var(--text-muted)" }}
      >
        <FolderOpen className="h-3 w-3 shrink-0" />
        <span className="min-w-0 truncate">{session.projectName}</span>
        {session.branchName && (
          <>
            <span className="shrink-0">·</span>
            <GitBranch className="h-3 w-3 shrink-0" />
            <span className="min-w-0 truncate">{session.branchName}</span>
          </>
        )}
      </div>
      {displayPath && (
        <div
          className="mt-0.5 truncate pl-[22px] font-mono text-[0.625rem]"
          style={{ color: "var(--text-muted)" }}
          title={session.worktreePath ?? undefined}
        >
          {displayPath}
        </div>
      )}
    </>
  );

  if (onClick) {
    return (
      <button
        type="button"
        data-testid={`terminal-session-${session.conversationId}`}
        className={cn(className, "text-left")}
        onClick={onClick}
      >
        {content}
      </button>
    );
  }

  return (
    <div
      data-testid={`terminal-session-${session.conversationId}`}
      className={className}
    >
      {content}
    </div>
  );
}

export function TerminalsPopover({
  sessions,
  children,
  open,
  onOpenChange,
  onNavigateToWorkspace,
  alignOffset = -24,
}: TerminalsPopoverProps) {
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent
        data-testid="terminals-popover"
        side="top"
        align="start"
        alignOffset={alignOffset}
        sideOffset={24}
        className="w-[400px] p-0"
        style={{
          backgroundColor: "var(--bg-surface)",
          border: "1px solid var(--overlay-weak)",
          borderRadius: "10px",
          boxShadow:
            "0 4px 16px var(--overlay-scrim), 0 12px 32px var(--overlay-scrim)",
        }}
        onInteractOutside={(event) => {
          if (shouldPreserveExecutionPopoverForTarget(event.target)) {
            event.preventDefault();
          }
        }}
      >
        <div
          className="flex items-center justify-between px-3 py-2.5"
          style={{ borderBottom: "1px solid var(--overlay-weak)" }}
        >
          <h3
            className="text-xs font-semibold"
            style={{ color: "var(--text-secondary)" }}
          >
            Terminals ({sessions.length})
          </h3>
        </div>

        <div
          className="max-h-[320px] overflow-y-auto p-1.5"
          style={{
            scrollbarWidth: "thin",
            scrollbarColor: "var(--overlay-moderate) transparent",
          }}
        >
          {sessions.length === 0 ? (
            <div
              className="py-6 text-center text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              No open terminals
            </div>
          ) : (
            sessions.map((session) => {
              const handleClick = onNavigateToWorkspace
                ? () => {
                    onOpenChange(false);
                    onNavigateToWorkspace(session.projectId, session.conversationId);
                  }
                : undefined;

              return (
                <TerminalSessionRow
                  key={session.conversationId}
                  session={session}
                  {...(handleClick !== undefined && { onClick: handleClick })}
                />
              );
            })
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
