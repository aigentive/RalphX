import type { Priority } from "@/types/ideation";

export const PRIORITY_CONFIG: Record<
  Priority,
  { gradient: string; glow: string; label: string }
> = {
  critical: {
    gradient: "from-status-error/20 to-status-error/10",
    glow: "shadow-[0_0_12px_var(--status-error-muted)]",
    label: "Critical",
  },
  high: {
    gradient: "from-accent-primary/20 to-accent-primary/10",
    glow: "shadow-[0_0_12px_var(--accent-muted)]",
    label: "High",
  },
  medium: {
    gradient: "from-status-warning/15 to-status-warning/5",
    glow: "",
    label: "Medium",
  },
  low: {
    gradient: "from-text-muted/10 to-text-muted/5",
    glow: "",
    label: "Low",
  },
};
