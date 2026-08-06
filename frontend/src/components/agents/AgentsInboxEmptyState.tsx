import { Check, Plus, type LucideIcon } from "lucide-react";
import type { ReactElement } from "react";

export type AgentsInboxEmptyTone = "win" | "calm";

export interface AgentsInboxEmptyAction {
  label: string;
  onClick: () => void;
}

export function AgentsInboxZeroCard({
  testId,
  tone,
  icon: Icon,
  headline,
  subline,
  primaryAction,
  secondaryAction,
}: {
  testId: string;
  tone: AgentsInboxEmptyTone;
  icon: LucideIcon;
  headline: string;
  subline: string;
  primaryAction?: AgentsInboxEmptyAction;
  secondaryAction?: AgentsInboxEmptyAction;
}): ReactElement {
  const isWin = tone === "win";

  return (
    <div
      className="flex flex-1 items-center justify-center px-1 pb-12 pt-4"
      data-testid={testId}
    >
      <div
        className="flex w-full flex-col items-center gap-[10px] rounded-[10px] px-[18px] py-5 text-center"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <span
          className="grid h-[34px] w-[34px] place-items-center rounded-full"
          style={{
            backgroundColor: isWin ? "var(--accent-muted)" : "var(--overlay-faint)",
            borderColor: isWin ? "var(--accent-border)" : "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: isWin ? "var(--accent-primary)" : "var(--text-subtle)",
          }}
        >
          <Icon aria-hidden="true" className="h-[17px] w-[17px]" />
        </span>
        <div
          className="text-[0.8438rem] font-semibold"
          style={{ color: "var(--text-primary)" }}
        >
          {headline}
        </div>
        <div
          className="text-xs leading-[1.5]"
          style={{ color: "var(--text-muted)" }}
        >
          {subline}
        </div>
        {primaryAction && (
          <button
            type="button"
            className="mt-1 inline-flex h-7 items-center gap-1.5 rounded-[6px] px-3 text-[0.7812rem] font-semibold transition-colors duration-[120ms] outline-none hover:bg-[var(--accent-hover)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:1px]"
            style={{
              backgroundColor: "var(--accent-primary)",
              borderColor: "var(--accent-hover)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-on-accent)",
            }}
            onClick={primaryAction.onClick}
          >
            <Plus aria-hidden="true" className="h-[13px] w-[13px]" />
            {primaryAction.label}
          </button>
        )}
        {secondaryAction && (
          <button
            type="button"
            className="border-0 text-[0.7188rem] font-medium transition-colors duration-[120ms] outline-none hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:1px]"
            style={{
              backgroundColor: "transparent",
              color: "var(--text-muted)",
            }}
            onClick={secondaryAction.onClick}
          >
            {secondaryAction.label}
          </button>
        )}
      </div>
    </div>
  );
}

export function AgentsInboxGroupEmptyStrip({
  testId,
  label,
}: {
  testId: string;
  label: string;
}): ReactElement {
  return (
    <div
      className="mx-0.5 mt-0.5 flex items-center gap-2 rounded-[6px] px-2.5 py-[9px]"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid={testId}
    >
      <Check
        aria-hidden="true"
        className="h-3.5 w-3.5 shrink-0"
        style={{ color: "var(--status-success)" }}
      />
      <span className="text-[0.7188rem]" style={{ color: "var(--text-muted)" }}>
        {label}
      </span>
    </div>
  );
}
