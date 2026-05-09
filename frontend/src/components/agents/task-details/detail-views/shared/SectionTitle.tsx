/**
 * SectionTitle - macOS Tahoe-inspired section header
 *
 * Features Apple's characteristic spacing and typography hierarchy
 * with SF Pro Display characteristics.
 */

import type { ReactNode } from "react";

interface SectionTitleProps {
  children: ReactNode;
  muted?: boolean;
}

export function SectionTitle({ children, muted = false }: SectionTitleProps) {
  return (
    <h3
      className={`text-[0.6875rem] font-semibold uppercase tracking-wider mb-3 ${muted ? "text-text-primary/35" : "text-text-primary/50"}`}
      style={{
        letterSpacing: "0.08em",
      }}
    >
      {children}
    </h3>
  );
}
