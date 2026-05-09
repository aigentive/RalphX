/**
 * StatusBanner - macOS Tahoe-inspired status header
 *
 * Tahoe principles:
 * - NO decorative borders
 * - Flat background with subtle color tint
 * - NO glows (except completion celebration - handled separately)
 */

import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import { statusTint, withAlpha } from "@/lib/theme-colors";

type BannerVariant = "success" | "warning" | "error" | "info" | "accent" | "neutral";

interface StatusBannerProps {
  icon: LucideIcon;
  title: string;
  subtitle?: string;
  badge?: ReactNode;
  variant: BannerVariant;
  animated?: boolean;
}

// Flat backgrounds only - no borders, no glows
const VARIANT_CONFIG: Record<BannerVariant, {
  bgColor: string;
  iconBg: string;
  iconColor: string;
  titleColor: string;
}> = {
  success: {
    bgColor: "var(--status-success-muted)",
    iconBg: statusTint("success", 18),
    iconColor: "var(--status-success)",
    titleColor: "var(--status-success)",
  },
  warning: {
    bgColor: "var(--status-warning-muted)",
    iconBg: statusTint("warning", 18),
    iconColor: "var(--status-warning)",
    titleColor: "var(--status-warning)",
  },
  error: {
    bgColor: "var(--status-error-muted)",
    iconBg: statusTint("error", 18),
    iconColor: "var(--status-error)",
    titleColor: "var(--status-error)",
  },
  info: {
    bgColor: "var(--status-info-muted)",
    iconBg: statusTint("info", 18),
    iconColor: "var(--status-info)",
    titleColor: "var(--status-info)",
  },
  accent: {
    bgColor: "var(--accent-muted)",
    iconBg: statusTint("accent", 18),
    iconColor: "var(--accent-primary)",
    titleColor: "var(--accent-primary)",
  },
  neutral: {
    bgColor: withAlpha("var(--text-muted)", 12),
    iconBg: withAlpha("var(--text-muted)", 18),
    iconColor: "var(--text-muted)",
    titleColor: "var(--text-muted)",
  },
};

export function StatusBanner({
  icon: Icon,
  title,
  subtitle,
  badge,
  variant,
  animated = false,
}: StatusBannerProps) {
  const config = VARIANT_CONFIG[variant];

  return (
    <div
      className="relative flex items-center gap-3.5 px-4 py-3.5 rounded-xl overflow-hidden"
      style={{
        backgroundColor: config.bgColor,
      }}
    >
      {/* Icon container - flat, no glow */}
      <div
        className="flex items-center justify-center w-9 h-9 rounded-xl shrink-0"
        style={{
          backgroundColor: config.iconBg,
        }}
      >
        <Icon
          className={`w-5 h-5 ${animated ? "animate-pulse" : ""}`}
          style={{ color: config.iconColor }}
        />
      </div>

      {/* Content */}
      <div className="flex-1 min-w-0">
        <span
          className="text-[0.875rem] font-semibold tracking-tight block"
          style={{ color: config.titleColor }}
        >
          {title}
        </span>
        {subtitle && (
          <span
            className="text-[12px] mt-0.5 block truncate"
            style={{ color: "var(--text-secondary)" }}
          >
            {subtitle}
          </span>
        )}
      </div>

      {/* Badge slot */}
      {badge}
    </div>
  );
}
