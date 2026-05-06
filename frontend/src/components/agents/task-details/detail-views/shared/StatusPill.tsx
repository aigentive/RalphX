/**
 * StatusPill - macOS Tahoe-inspired compact status indicator
 *
 * Features:
 * - Frosted vibrancy background
 * - Bold system colors
 * - Compact, pill-shaped design
 */

import type { LucideIcon } from "lucide-react";
import { statusTint, withAlpha } from "@/lib/theme-colors";

type PillVariant = "success" | "warning" | "error" | "info" | "accent" | "neutral";

interface StatusPillProps {
  icon?: LucideIcon;
  label: string;
  variant: PillVariant;
  animated?: boolean;
  size?: "sm" | "md";
}

// Theme-token backed variants — see specs/design/styleguide.md §5
const VARIANT_STYLES: Record<PillVariant, { bg: string; text: string }> = {
  success: {
    bg: statusTint("success", 18),
    text: "var(--status-success)",
  },
  warning: {
    bg: statusTint("warning", 18),
    text: "var(--status-warning)",
  },
  error: {
    bg: statusTint("error", 18),
    text: "var(--status-error)",
  },
  info: {
    bg: statusTint("info", 18),
    text: "var(--status-info)",
  },
  accent: {
    bg: statusTint("accent", 18),
    text: "var(--accent-primary)",
  },
  neutral: {
    bg: withAlpha("var(--text-muted)", 18),
    text: "var(--text-muted)",
  },
};

export function StatusPill({
  icon: Icon,
  label,
  variant,
  animated = false,
  size = "sm",
}: StatusPillProps) {
  const styles = VARIANT_STYLES[variant];
  const sizeClasses = size === "sm"
    ? "px-2.5 py-1 text-[0.625rem] gap-1.5"
    : "px-3 py-1.5 text-[0.6875rem] gap-2";
  const iconSize = size === "sm" ? "w-3 h-3" : "w-3.5 h-3.5";

  return (
    <div
      className={`inline-flex items-center ${sizeClasses} rounded-full font-semibold`}
      style={{
        backgroundColor: styles.bg,
        color: styles.text,
        backdropFilter: "blur(12px) saturate(150%)",
        WebkitBackdropFilter: "blur(12px) saturate(150%)",
      }}
    >
      {Icon && (
        <Icon
          className={`${iconSize} ${animated ? "animate-pulse" : ""}`}
          style={{ color: styles.text }}
        />
      )}
      <span className="tracking-tight">{label}</span>
    </div>
  );
}
