// src/components/ticketing/ticket-select-styles.ts
//
// Canonical native <select> treatment for the ticketing surface.
// Exported as a className builder + a static style object so it drops into the
// existing heterogeneous select hosts (label-wrapped filters, tooltip-wrapped
// status select, grid dialog pickers) with no structural change.
//
// WKWebView-safe:
//  - paint/border use longhands in the style object (no background/border shorthand) — Rule 6
//  - all tokens resolve to a per-theme literal at the final hop — Rule 1
//  - the chevron caret is a background-image data-URI with a LITERAL hsl() stroke
//    (not var()) delivered via a stylesheet class — Rule 7
//  - focus ring uses the app focus token --border-focus (visible in all 3 themes)
import type { CSSProperties } from "react";

export type TicketSelectSize = "sm" | "md";

// Literal-stroke chevron, identical color to the canonical selectBaseStyles
// (TaskFormFields.constants.ts). hsl(220 10% 50%) ≈ #72787F reads on dark, light,
// and high-contrast surfaces. Must stay literal: WKWebView drops var() inside
// background-image SVG.
const CARET_CLASSES =
  "appearance-none " +
  "bg-[url('data:image/svg+xml;charset=utf-8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2216%22%20height%3D%2216%22%20viewBox%3D%220%200%2024%2024%22%20fill%3D%22none%22%20stroke%3D%22hsl(220%2010%25%2050%25)%22%20stroke-width%3D%222%22%3E%3Cpath%20d%3D%22M6%209l6%206%206-6%22%2F%3E%3C%2Fsvg%3E')] " +
  "bg-[length:16px_16px] bg-[right_12px_center] bg-no-repeat pr-9";

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
 * Canonical className for a ticketing native <select>.
 * @param size  "sm" (h-8 compact filter bars) | "md" (h-9 dialog pickers)
 * @param extra optional caller classes appended last (e.g. min-w / max-w utilities)
 */
export function ticketSelectClassName(
  size: TicketSelectSize = "sm",
  extra = "",
): string {
  return [
    SIZE_CLASSES[size],
    "rounded-md px-2 cursor-pointer transition-colors duration-150",
    CARET_CLASSES,
    FOCUS_CLASSES,
    DISABLED_CLASSES,
    extra,
  ]
    .filter(Boolean)
    .join(" ")
    .trim();
}

/**
 * Canonical inline style for a ticketing native <select>.
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
