/**
 * DetailCard - macOS Tahoe-inspired card
 *
 * Tahoe principles:
 * - NO decorative borders
 * - Flat neutral background
 * - Minimal shadows (only for floating elements)
 */

import type { ReactNode } from "react";

type CardVariant = "default" | "success" | "warning" | "error" | "info" | "accent";

interface DetailCardProps {
  children: ReactNode;
  variant?: CardVariant;
  className?: string;
  noPadding?: boolean;
}

export function DetailCard({
  children,
  variant: _variant = "default",
  className = "",
  noPadding = false,
}: DetailCardProps) {
  return (
    <div
      className={`rounded-xl ${className}`}
      style={{
        backgroundColor: "var(--bg-surface)",
        padding: noPadding ? undefined : "14px 16px",
      }}
    >
      {children}
    </div>
  );
}
