/**
 * BranchBadge — Monospace branch name with tooltip showing full name.
 * BranchFlow — Source → Target branch flow using two BranchBadge components.
 *
 * Replaces: abbreviateBranch(), shortBranch(), inline .split("/").pop()
 */

import { formatBranchDisplay } from "@/lib/branch-utils";
import { withAlpha } from "@/lib/theme-colors";

// ============================================================================
// BranchBadge
// ============================================================================

type BranchVariant = "default" | "source" | "target" | "muted";
type BranchSize = "sm" | "md";

interface BranchBadgeProps {
  branch: string;
  variant?: BranchVariant;
  size?: BranchSize;
}

const VARIANT_COLORS: Record<BranchVariant, string> = {
  default: withAlpha("var(--text-primary)", 70),
  source: withAlpha("var(--text-primary)", 70),
  target: withAlpha("var(--text-primary)", 85),
  muted: withAlpha("var(--text-primary)", 40),
};

const SIZE_CONFIG: Record<BranchSize, { fontSize: string }> = {
  sm: { fontSize: "0.6875rem" },
  md: { fontSize: "0.8125rem" },
};

export function BranchBadge({
  branch,
  variant = "default",
  size = "md",
}: BranchBadgeProps) {
  const { short, full } = formatBranchDisplay(branch);
  const sizeConf = SIZE_CONFIG[size];

  return (
    <span
      className="font-mono truncate"
      style={{
        fontSize: sizeConf.fontSize,
        color: VARIANT_COLORS[variant],
      }}
      title={full}
    >
      {short}
    </span>
  );
}

// ============================================================================
// BranchFlow
// ============================================================================

interface BranchFlowProps {
  source: string;
  target: string;
  size?: BranchSize;
}

export function BranchFlow({
  source,
  target,
  size = "md",
}: BranchFlowProps) {
  return (
    <span className="inline-flex items-center gap-1.5 min-w-0">
      <BranchBadge branch={source} variant="source" size={size} />
      <span
        className="text-text-primary/40"
        style={{
          fontSize: SIZE_CONFIG[size].fontSize,
        }}
      >
        &rarr;
      </span>
      <BranchBadge branch={target} variant="target" size={size} />
    </span>
  );
}
