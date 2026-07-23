import {
  AlertTriangle,
  CheckCircle2,
  Info,
  Loader2,
  XCircle,
} from "lucide-react";
import type { ReactNode } from "react";

import {
  StatusPill,
  type StatusPillTone,
} from "@/components/ui/status-pill";

export type AgentsPublishActionTone =
  | "neutral"
  | "success"
  | "warning"
  | "error";

export interface AgentsPublishActionPresentation {
  title: string;
  summary: string;
  tone: AgentsPublishActionTone;
  busy?: boolean;
}

export interface AgentsPublishChangeFacts {
  fileCount: number;
  additions: number;
  deletions: number;
}

export interface AgentsPublishAutomationStatus {
  label: string;
  tone: StatusPillTone;
  live?: boolean;
}

const TONE_COLORS: Record<AgentsPublishActionTone, string> = {
  neutral: "var(--text-secondary, #c7c7cc)",
  success: "var(--status-success, #2eb867)",
  warning: "var(--status-warning, #e8a33d)",
  error: "var(--status-error, #e5484d)",
};

function ActionStatusIcon({
  busy,
  tone,
}: {
  busy: boolean;
  tone: AgentsPublishActionTone;
}) {
  const StatusIcon = busy
    ? Loader2
    : tone === "success"
      ? CheckCircle2
      : tone === "warning"
        ? AlertTriangle
        : tone === "error"
          ? XCircle
          : Info;

  return (
    <span
      className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full"
      style={{
        backgroundColor: "var(--bg-elevated, #292930)",
        color: TONE_COLORS[tone],
      }}
      aria-hidden="true"
    >
      <StatusIcon
        className={`h-4 w-4${busy ? " animate-spin" : ""}`}
        data-testid="agents-publish-status-icon"
      />
    </span>
  );
}

export function AgentsPublishActionBar({
  presentation,
  changeFacts,
  automationStatus,
  primaryAction,
  overflowAction,
}: {
  presentation: AgentsPublishActionPresentation;
  changeFacts?: AgentsPublishChangeFacts | null;
  automationStatus?: AgentsPublishAutomationStatus | null;
  primaryAction: ReactNode;
  overflowAction?: ReactNode;
}) {
  return (
    <section
      className="-mx-4 grid max-w-[calc(100%+2rem)] grid-cols-1 gap-3 px-4 py-3"
      data-testid="agents-publish-actionbar"
      data-tone={presentation.tone}
      style={{
        backgroundColor: "var(--bg-surface, #212127)",
        borderColor: "var(--border-subtle, #33333b)",
        borderStyle: "solid",
        borderWidth: "0 0 1px",
      }}
    >
      <div className="flex min-w-0 w-full items-center gap-2.5">
        <ActionStatusIcon
          busy={presentation.busy ?? false}
          tone={presentation.tone}
        />
        <div className="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-2 gap-y-0.5">
          <h2 className="min-w-0 truncate text-sm font-semibold text-[var(--text-primary)]">
            {presentation.title}
          </h2>
          <p className="min-w-0 flex-[1_1_12rem] truncate text-xs text-[var(--text-muted)]">
            {presentation.summary}
          </p>
        </div>
      </div>

      <div className="flex min-w-0 w-full max-w-full flex-wrap items-center justify-start gap-2">
        {changeFacts ? (
          <div
            className="flex shrink-0 items-center gap-1.5 text-[11px] font-medium text-[var(--text-muted)]"
            data-testid="agents-publish-change-facts"
          >
            <span>
              {changeFacts.fileCount} file
              {changeFacts.fileCount === 1 ? "" : "s"}
            </span>
            <span aria-hidden="true">·</span>
            <span
              className="text-[var(--status-success)]"
              data-testid="agents-publish-additions"
            >
              +{changeFacts.additions}
            </span>
            <span
              className="text-[var(--status-error)]"
              data-testid="agents-publish-deletions"
            >
              −{changeFacts.deletions}
            </span>
          </div>
        ) : null}
        {automationStatus ? (
          <StatusPill
            label={automationStatus.label}
            tone={automationStatus.tone}
            live={automationStatus.live ?? false}
            testId="agents-pr-supervision-status"
          />
        ) : null}
        <div className="flex max-w-full flex-wrap items-center gap-2">
          {primaryAction}
          {overflowAction}
        </div>
      </div>
    </section>
  );
}
