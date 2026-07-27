import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

export type NoticeBannerTone =
  | "warning"
  | "error"
  | "success"
  | "neutral"
  | "accent";

interface NoticeBannerToneStyle {
  color: string;
  backgroundColor: string;
  borderColor: string;
}

const TONE_STYLES: Record<NoticeBannerTone, NoticeBannerToneStyle> = {
  warning: {
    color: "var(--status-warning, #e0b341)",
    backgroundColor: "var(--status-warning-muted)",
    borderColor: "var(--status-warning-border)",
  },
  error: {
    color: "var(--status-error, #d55e00)",
    backgroundColor: "var(--status-error-muted)",
    borderColor: "var(--status-error-border)",
  },
  success: {
    color: "var(--status-success, #3fbf7f)",
    backgroundColor: "var(--status-success-muted)",
    borderColor: "var(--status-success-border)",
  },
  neutral: {
    color: "var(--text-secondary, #c7c7cc)",
    backgroundColor: "var(--bg-surface, #1e1e23)",
    borderColor: "var(--border-default, #393940)",
  },
  accent: {
    color: "var(--accent-primary, #ff6a35)",
    backgroundColor: "var(--accent-muted)",
    borderColor: "var(--accent-border)",
  },
};

export interface NoticeBannerProps {
  tone: NoticeBannerTone;
  icon?: ReactNode;
  title?: ReactNode;
  children: ReactNode;
  action?: ReactNode;
  testId?: string;
  className?: string;
}

export function NoticeBanner({
  tone,
  icon,
  title,
  children,
  action,
  testId,
  className,
}: NoticeBannerProps) {
  const style = TONE_STYLES[tone];

  return (
    <div
      className={cn("flex items-start gap-2 rounded-md px-3 py-2.5", className)}
      style={{
        backgroundColor: style.backgroundColor,
        borderColor: style.borderColor,
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-tone={tone}
      {...(testId ? { "data-testid": testId } : {})}
    >
      {icon ? (
        <span className="mt-0.5 shrink-0" style={{ color: style.color }}>
          {icon}
        </span>
      ) : null}
      <div className="min-w-0 flex-1 text-sm font-normal" style={{ color: "var(--text-secondary, #c7c7cc)" }}>
        {title ? (
          <strong className="font-semibold" style={{ color: style.color }}>
            {title}
          </strong>
        ) : null}
        {title && children ? " " : null}
        {children}
      </div>
      {action ? <div className="ml-auto shrink-0">{action}</div> : null}
    </div>
  );
}
