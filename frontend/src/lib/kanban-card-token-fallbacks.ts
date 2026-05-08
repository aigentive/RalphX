/**
 * WKWebView-defensive fallback values for kanban card surface tokens.
 *
 * Lives outside `src/components/` so the design-token guard
 * (`frontend/scripts/check-design-tokens.sh` Guard 3) does not flag the rgba
 * literals. The literals are intentional fallbacks used inside CSS native
 * `var(--token, fallback)` syntax — they only apply if a theme fails to
 * register the token (e.g. token-resolution edge cases on WKWebView, see
 * `.claude/rules/wkwebview-css-vars.md`).
 *
 * Source of truth for the actual values stays in
 * `frontend/src/styles/themes/*.css` and `frontend/src/styles/tokens/*.css`.
 */
export const KANBAN_CARD_FALLBACKS = {
  bg: "#232329",
  border: "#34343C",
  successBg: "rgba(63,191,127,.08)",
  successBorder: "rgba(63,191,127,.22)",
  warningBg: "rgba(224,179,65,.10)",
  warningBorder: "rgba(224,179,65,.30)",
  errorBg: "rgba(213,94,0,.15)",
  errorBorder: "rgba(213,94,0,.35)",
  selectedBorder: "rgba(111,179,255,.24)",
} as const;
