import { GitPullRequest, GitMerge, GitPullRequestClosed, Pen } from "lucide-react";
import { withAlpha } from "@/lib/theme-colors";

type PrStatus = "Draft" | "Open" | "Merged" | "Closed";

const STATUS_CONFIG = {
  Draft: {
    label: "Draft",
    icon: Pen,
    bg: withAlpha("var(--text-muted)", 15),
    color: "var(--text-muted)",
  },
  Open: {
    label: "Open",
    icon: GitPullRequest,
    bg: withAlpha("var(--text-muted)", 15),
    color: "var(--text-muted)",
  },
  Merged: {
    label: "Merged",
    icon: GitMerge,
    bg: withAlpha("var(--text-muted)", 15),
    color: "var(--text-muted)",
  },
  Closed: {
    label: "Closed",
    icon: GitPullRequestClosed,
    bg: withAlpha("var(--text-muted)", 15),
    color: "var(--text-muted)",
  },
} as const;

export function PrStatusBadge({ status }: { status: PrStatus }) {
  const config = STATUS_CONFIG[status];
  const Icon = config.icon;
  return (
    <span
      className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[0.6875rem] font-medium"
      style={{ backgroundColor: config.bg, color: config.color }}
    >
      <Icon className="w-3 h-3" />
      {config.label}
    </span>
  );
}
