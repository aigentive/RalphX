> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# WKWebView CSS Custom-Property Rules

## Context

RalphX ships in Tauri (macOS WKWebView). WKWebView drops chained `var()` references on certain document-canvas properties, leaving `body` transparent while the attribute + CSS file are loaded. The web build (`npm run dev:web` in Chromium) doesn't reproduce this — Playwright audits pass while the Tauri window shows white. Root-caused 2026-04-19.

Incident: HC theme `--bg-base: var(--color-black)` failed to cascade to `body` in WKWebView even though `--color-black` resolved correctly on `<html>`. Fixed in `bd9ee48d2`.

## Rules (NON-NEGOTIABLE)

| # | Rule |
|---|------|
| 1 | **No chained `var()` for canvas paint tokens.** `--bg-base`, `--bg-surface`, `--bg-elevated`, and any other token painted directly on `html` / `body` / `main` MUST use a literal color value (`#rrggbb`, `hsl(...)`, `hsla(...)`) — ❌ `var(--primitive)`. |
| 2 | **Defensive canvas paint on new themes.** Every theme file adding a new `[data-theme="X"]` block MUST include an explicit `html[data-theme="X"], html[data-theme="X"] body, html[data-theme="X"] main { background-color: <literal> !important; }` rule so the canvas paints even if a later var-chain breaks. |
| 3 | **Token-chain depth ≤ 1 for role tokens consumed by `background-color`.** `--card-bg: var(--bg-elevated)` is fine (one hop). `--card-bg: var(--bg-elevated)` where `--bg-elevated: var(--color-black)` (two hops into a primitive) is not — flatten to literal on the final hop. |
| 4 | **Verify in Tauri, not just `dev:web`.** Any theme or canvas token change MUST be verified inside `npm run tauri dev`. ❌ Shipping purely on Playwright/web screenshots. |
| 5 | **Light/Dark use literals too.** Uniform rule — Light uses `hsl(35 12% 97%)`, Dark uses `hsl(220 10% 8%)` — both literals. Don't reintroduce primitives for canvas tokens just because they "work" in Light/Dark today; the next WKWebView build might drop those too. |
| 6 | **Themed surfaces use paint/border longhands.** Cards, sidebars, nav bars, and Kanban/task surfaces MUST set explicit `background-color` / `border-color` / `border-width` / `border-style` (or React `backgroundColor` / `borderColor` / `borderWidth` / `borderStyle`) with fallbacks. ❌ Relying on `background` / `border` shorthand after theme-token changes; WKWebView can render transparent or default-border surfaces while Chromium looks correct. |
| 7 | **No `var()` inside inline-style gradients.** `linear-gradient(...)` / `radial-gradient(...)` / `conic-gradient(...)` delivered via React inline `style` (or any `style="..."` attribute) MUST use literal color values — ❌ `style={{ backgroundImage: "linear-gradient(180deg, var(--x), var(--y))" }}`. WKWebView's CSSOM does not always substitute custom properties inside the `<image>`-typed gradient function when parsed from the inline-style attribute, even though the same vars resolve in stylesheet rules and in plain `backgroundColor: "var(--x)"`. Fix: use literal `rgba(...)` / `hsl(...)` / `#rrggbb` inline, or move the gradient into a stylesheet rule and toggle via `data-*` / `aria-current` selectors. Root-caused 2026-05-05 (agents sidebar active-project highlight). |

## Quick reference

```css
/* ❌ Breaks on WKWebView — canvas tokens rely on a chained var hop */
[data-theme="high-contrast"] {
  --bg-base:     var(--color-black);
  --bg-surface:  var(--color-black);
  --bg-elevated: var(--color-black);
}

/* ✅ Works everywhere — literals on canvas tokens */
[data-theme="high-contrast"] {
  --bg-base:     #000000;
  --bg-surface:  #000000;
  --bg-elevated: #000000;
}

/* ✅ Defensive canvas paint */
html[data-theme="high-contrast"],
html[data-theme="high-contrast"] body,
html[data-theme="high-contrast"] main {
  background-color: #000000 !important;
}
```

## Diagnostic recipe

If a future theme renders "right attribute, wrong color" in Tauri:

```js
const cs = getComputedStyle(document.body);
({
  theme: document.documentElement.dataset.theme,
  bodyBg: cs.backgroundColor,
  bodyBgBase: cs.getPropertyValue("--bg-base").trim(),
  htmlBgBase: getComputedStyle(document.documentElement).getPropertyValue("--bg-base").trim(),
})
```

| Symptom | Cause | Fix |
|---------|-------|-----|
| `bodyBg: "rgba(0, 0, 0, 0)"` + `bodyBgBase: ""` | Chained `var()` dropped by WKWebView | Flatten per Rule 1 + add defensive canvas paint per Rule 2 |
| `bodyBg: "rgb(...)"` matches theme intent | Working correctly | — |
| `theme` mismatches active selector | `themeStore` desync | Fix store; out of scope for this rule |
| Element renders transparent in Tauri but `getPropertyValue('--x')` returns the right value, AND the failing prop is a gradient set via inline `style` | WKWebView dropped `var()` substitution inside inline-style gradient | Apply Rule 7: literal colors inline, or move gradient to a stylesheet rule |
