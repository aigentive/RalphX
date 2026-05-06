/**
 * IdeationSessionCard - Compact row for a running ideation session
 *
 * Line 1: chat icon | title | "Ideation" badge
 * Line 2: elapsed time · team mode badge
 */

import { MessageSquare, Loader2, Pause } from "lucide-react";
import type { RunningIdeationSession } from "@/api/running-processes";
import { useElapsedTimer } from "@/hooks/useElapsedTimer";
import { formatElapsedTime } from "@/lib/formatters";

interface IdeationSessionCardProps {
  session: RunningIdeationSession;
  onClick?: () => void;
  showTeamModeBadge?: boolean;
}

export function IdeationSessionCard({
  session,
  onClick,
  showTeamModeBadge = true,
}: IdeationSessionCardProps) {
  const elapsedTime = useElapsedTimer(session.elapsedSeconds, session.sessionId);

  return (
    <div
      data-testid={`ideation-card-${session.sessionId}`}
      className={`px-2 py-1.5 rounded-md hover:bg-[var(--overlay-faint)] transition-colors${onClick ? " cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-border)]" : ""}`}
      {...(onClick
        ? {
            role: "button" as const,
            tabIndex: 0,
            onClick,
            onKeyDown: (e: React.KeyboardEvent) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick();
              }
            },
          }
        : {})}
    >
      {/* Line 1: Icon | Title | Badge */}
      <div className="flex items-center gap-2">
        {session.isGenerating ? (
          <Loader2
            className="w-3.5 h-3.5 animate-spin shrink-0"
            style={{ color: "var(--accent-primary)" }}
          />
        ) : (
          <Pause
            className="w-3.5 h-3.5 shrink-0"
            style={{ color: "var(--text-muted)" }}
          />
        )}
        <span
          className="flex-1 text-xs font-medium truncate min-w-0 text-left"
          style={{ color: "var(--text-primary)" }}
          title={session.title}
        >
          {session.title}
        </span>
        <span
          className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded shrink-0"
          style={{
            color: "var(--accent-primary)",
            backgroundColor: "var(--accent-muted)",
          }}
        >
          Ideation
        </span>
      </div>

      {/* Line 2: Elapsed · Team mode */}
      <div
        className="flex items-center gap-1.5 mt-0.5 pl-[22px] text-[0.6875rem] min-w-0"
        style={{ color: "var(--text-muted)" }}
      >
        <MessageSquare className="w-3 h-3 shrink-0" style={{ color: "var(--text-muted)" }} />
        <span className="shrink-0 tabular-nums">
          {formatElapsedTime(elapsedTime)}
        </span>
        {showTeamModeBadge && session.teamMode && (
          <>
            <span className="shrink-0" style={{ color: "var(--text-muted)" }}>
              ·
            </span>
            <span
              className="text-[0.625rem] font-medium px-1 rounded shrink-0"
              style={{
                color: "var(--text-secondary)",
                backgroundColor: "var(--overlay-moderate)",
              }}
            >
              {session.teamMode}
            </span>
          </>
        )}
      </div>
    </div>
  );
}
