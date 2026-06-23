// src/components/ticketing/ticket-select-styles.ts
//
// Canonical select-control treatment for the ticketing surface.
// Exported as a className builder + a static style object so it drops into the
// existing heterogeneous select hosts (label-wrapped filters, tooltip-wrapped
// status select, grid dialog pickers) with no structural change.
//
// WKWebView-safe:
//  - paint/border use longhands in the style object (no background/border shorthand) — Rule 6
//  - all tokens resolve to a per-theme literal at the final hop — Rule 1
//  - the chevron caret is painted with token-driven stylesheet gradients — Rule 7
//  - focus ring uses the app focus token --border-focus (visible in all 3 themes)
import type { CSSProperties } from "react";

export type TicketSelectSize = "sm" | "md";
interface TicketSelectClassNameOptions {
  nativeCaret?: boolean | undefined;
}

// Token-colored chevron drawn with stylesheet background layers so component
// code does not embed raw color literals.
const CARET_CLASSES =
  "appearance-none " +
  "bg-[linear-gradient(45deg,transparent_45%,var(--text-muted)_45%_55%,transparent_55%),linear-gradient(135deg,transparent_45%,var(--text-muted)_45%_55%,transparent_55%)] " +
  "bg-[length:7px_7px,7px_7px] " +
  "bg-[position:calc(100%_-_17px)_50%,calc(100%_-_12px)_50%] " +
  "bg-no-repeat pr-9";

// Visible focus ring using the app focus token; works in Dark/Light/High-Contrast.
const FOCUS_CLASSES =
  "outline-none " +
  "focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]";

const DISABLED_CLASSES = "disabled:cursor-not-allowed disabled:opacity-50";

const SIZE_CLASSES: Record<TicketSelectSize, string> = {
  sm: "h-8 text-sm",
  md: "h-9 text-sm",
};

/**
 * Canonical className for a ticketing select or select-like trigger.
 * @param size  "sm" (h-8 compact filter bars) | "md" (h-9 dialog pickers)
 * @param extra optional caller classes appended last (e.g. min-w / max-w utilities)
 */
export function ticketSelectClassName(
  size: TicketSelectSize = "sm",
  extra = "",
  options: TicketSelectClassNameOptions = {},
): string {
  const includeNativeCaret = options.nativeCaret ?? true;
  return [
    SIZE_CLASSES[size],
    "rounded-md px-2 cursor-pointer transition-colors duration-150",
    includeNativeCaret ? CARET_CLASSES : "appearance-none",
    FOCUS_CLASSES,
    DISABLED_CLASSES,
    extra,
  ]
    .filter(Boolean)
    .join(" ")
    .trim();
}

/**
 * Canonical inline style for a ticketing select or select-like trigger.
 * Longhands only (WKWebView Rule 6). Tokens resolve to per-theme literals.
 * Used as the base; spread caller overrides AFTER if ever needed.
 */
export const ticketSelectStyle: CSSProperties = {
  backgroundColor: "var(--bg-elevated)",
  borderColor: "var(--border-default)",
  borderStyle: "solid",
  borderWidth: "1px",
  color: "var(--text-primary)",
};

/**
 * Canonical inline style for an <option> inside a ticketing select.
 * Honored by browsers/themes that style the in-list options. NOTE: the native
 * macOS WKWebView popup menu ignores per-option background and partially ignores
 * color; this is a known native-control limitation shared by all app <select>s.
 */
export const ticketSelectOptionStyle: CSSProperties = {
  backgroundColor: "var(--bg-elevated)",
  color: "var(--text-primary)",
};
