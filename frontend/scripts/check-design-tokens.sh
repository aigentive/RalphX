#!/usr/bin/env bash
# check-design-tokens.sh
#
# Fail the build if components leak design-system rules. Run from the frontend
# directory (or via `npm run check:tokens`).
#
# Guards:
#   1. No primitive tokens in component files (tier-1 leaks).
#   2. No Tailwind default palette utilities (palette leaks).
#   3. No inline rgba/rgb literals in component files.
#   4. No brand hex literals in live code.
#   5. No raw hsl() / hsla() colour literals in component inline styles.
#   6. No `white/N` or `black/N` Tailwind opacity-modifier classes in component
#      files (they don't flip per theme; use overlay / text tokens instead).
#   7. No hardcoded hue families for status or accent (hsl(14 …), hsl(0 70 …),
#      hsl(45 … %), hsl(145 … %), hsl(220 80 … %)).
#
# Excluded paths (documented in specs/design/styleguide.md §12):
#   - src/components/WelcomeScreen/** — marketing splash
#   - *.test.tsx / *.test.ts — tests pin class/token names
#   - src/styles/** — token sources

set -euo pipefail

FRONTEND_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$FRONTEND_DIR"

EXCLUDE_PATHS=(
  --exclude='*.test.tsx'
  --exclude='*.test.ts'
  --exclude='*.snap'
  --exclude-dir='__snapshots__'
  --exclude-dir='WelcomeScreen'
  # Lightbox component intentionally paints black/white gradients regardless
  # of theme — it's a media viewer chrome, not a themed surface.
  --exclude-dir='ScreenshotGallery'
  # SVG data-URI backgrounds embed CSS in URL params — allow but they count
  # as components. Inline SVGs with color-literal fills for background-image
  # should use a theme CSS var in the stroke.
  --exclude='TaskFormFields.constants.ts'
)

FAIL=0

print_section() { printf "\n\033[1m%s\033[0m\n" "$1"; }
print_fail()    { printf "\033[31mFAIL\033[0m %s\n" "$1"; FAIL=1; }
print_pass()    { printf "\033[32mPASS\033[0m %s\n" "$1"; }

# ----------------------------------------------------------------------------
# Guard 1 — primitive-tier token leaks in components
# ----------------------------------------------------------------------------
print_section "Guard 1: primitive tokens in components"

PRIMITIVE_PATTERN='var\(--(gray|orange|amber|yellow-[0-9]|blue-[0-9]|cvd|hc|alpha-)'
hits=$(grep -rEn "${PRIMITIVE_PATTERN}" src/components "${EXCLUDE_PATHS[@]}" || true)

if [ -n "${hits}" ]; then
  print_fail "Primitive tokens must not appear in components (use the semantic or component tier)."
  echo "${hits}"
else
  print_pass "No primitive-tier leaks."
fi

# ----------------------------------------------------------------------------
# Guard 2 — Tailwind default palette utilities in components
# ----------------------------------------------------------------------------
print_section "Guard 2: Tailwind default palette in components"

PALETTE_PATTERN='\b(bg|text|border|ring|from|to|via)-(red|green|blue|amber|emerald|rose|yellow|indigo|purple|pink|sky|slate|zinc|neutral|stone)-[0-9]{2,3}\b'
hits=$(grep -rEn "${PALETTE_PATTERN}" src/components "${EXCLUDE_PATHS[@]}" || true)

if [ -n "${hits}" ]; then
  print_fail "Tailwind default palette classes bypass the theme system. Use text-status-error / bg-accent-primary / etc."
  echo "${hits}"
else
  print_pass "No Tailwind default-palette leaks."
fi

# ----------------------------------------------------------------------------
# Guard 3 — inline rgba/rgb literals in components
# ----------------------------------------------------------------------------
print_section "Guard 3: inline rgba/rgb in components"

# Allow `color-mix(...)` calls — they're the canonical dynamic-alpha pattern.
RGBA_HITS=$(grep -rEn 'rgba\(|rgb\(' src/components "${EXCLUDE_PATHS[@]}" \
  | grep -v 'color-mix' || true)

if [ -n "${RGBA_HITS}" ]; then
  print_fail "Inline rgba/rgb literals must be tokenised (use var(--token) or withAlpha())."
  echo "${RGBA_HITS}"
else
  print_pass "No inline rgba/rgb literals."
fi

# ----------------------------------------------------------------------------
# Guard 4 — brand-hex literals in live code (comments/docstrings exempt)
# ----------------------------------------------------------------------------
print_section "Guard 4: brand hex literals in live component code"

BRAND_HEX='#(ff6b35|FF6B35|34c759|34C759|64d2ff|64D2FF|0a84ff|0A84FF|ff9f0a|FF9F0A|ff3b30|FF3B30|30d158|30D158|ffd60a|FFD60A)'
BRAND_HITS=$(grep -rEn "${BRAND_HEX}" src/components "${EXCLUDE_PATHS[@]}" \
  | grep -vE ':[0-9]+:\s*\*' \
  | grep -vE ':[0-9]+:\s*//' || true)

if [ -n "${BRAND_HITS}" ]; then
  print_fail "Brand hex literals in live code — use var(--accent-primary) / var(--status-*)."
  echo "${BRAND_HITS}"
else
  print_pass "No brand hex in live code (docstrings exempt)."
fi

# ----------------------------------------------------------------------------
# Guard 5 — raw hsl() / hsla() in component inline styles
# ----------------------------------------------------------------------------
# Any hsl( ... ) / hsla( ... ) appearing in component code (not in token CSS
# sources) bypasses the theme system. The only allowed form is a var(--token)
# reference that itself resolves to hsl(). Allow hsl inside color-mix() since
# that's a legitimate pattern; the token the color-mix wraps is usually a var.
print_section "Guard 5: raw hsl() literals in component files"

HSL_HITS=$(grep -rEn 'hsla?\(' src/components "${EXCLUDE_PATHS[@]}" \
  | grep -v 'color-mix' \
  | grep -vE ':[0-9]+:\s*\*' \
  | grep -vE ':[0-9]+:\s*//' \
  | grep -vE 'var\(--[^)]*\)[^,]*hsla?' || true)

if [ -n "${HSL_HITS}" ]; then
  print_fail "Raw hsl()/hsla() bypass the theme token cascade. Use var(--bg-*/text-*/accent-*/status-*) instead."
  echo "${HSL_HITS}"
else
  print_pass "No raw hsl()/hsla() literals in components."
fi

# ----------------------------------------------------------------------------
# Guard 6 — white/black Tailwind opacity-modifier classes
# ----------------------------------------------------------------------------
# `text-white/40`, `bg-white/5`, `bg-black/50`, `border-white/10` etc. compile
# to `color-mix(var(--color-white) N% …)` — which resolves correctly on Dark
# but dissolves on Light (white-on-white) or paints inverted on HC.
# Use overlay tokens (--overlay-faint/-weak/-moderate), scrim tokens
# (--overlay-scrim/-med/-deep), or text-* alpha tokens instead.
print_section "Guard 6: Tailwind white/N + black/N opacity classes"

WB_PATTERN='\b(bg|text|border|ring|from|to|via)-(white|black)\/[0-9]+\b'
WB_HITS=$(grep -rEn "${WB_PATTERN}" src/components "${EXCLUDE_PATHS[@]}" || true)

if [ -n "${WB_HITS}" ]; then
  print_fail "text-/bg-/border-white|black opacity classes don't flip per theme. Use overlay-* or text-* tokens."
  echo "${WB_HITS}"
else
  print_pass "No white/N or black/N opacity classes in components."
fi

# ----------------------------------------------------------------------------
# Guard 7 — hardcoded accent / status hue families
# ----------------------------------------------------------------------------
# Specific hsl hue families that indicate brand / status drift:
#   14 100% …  → accent orange          (use --accent-primary)
#   0 70% …    → status error red       (use --status-error)
#   45 90% …   → status warning yellow  (use --status-warning)
#   145 60% …  → status success green   (use --status-success)
#   220 80% …  → status info blue       (use --status-info)
# Strips hits inside color-mix() and var() references.
print_section "Guard 7: hardcoded brand/status hue families"

HUE_PATTERN='hsla?\( *(14 +100%|0 +70%|45 +90%|145 +60%|220 +80%)'
HUE_HITS=$(grep -rEn "${HUE_PATTERN}" src/components "${EXCLUDE_PATHS[@]}" \
  | grep -v 'color-mix' \
  | grep -vE ':[0-9]+:\s*\*' \
  | grep -vE ':[0-9]+:\s*//' || true)

if [ -n "${HUE_HITS}" ]; then
  print_fail "Hardcoded brand/status hue families bypass --accent-primary / --status-* tokens."
  echo "${HUE_HITS}"
else
  print_pass "No hardcoded accent/status hue families."
fi

# ----------------------------------------------------------------------------
# Result
# ----------------------------------------------------------------------------
echo ""
if [ "$FAIL" -ne 0 ]; then
  echo -e "\033[31mDesign-token guards failed.\033[0m Fix the leaks above or update specs/design/styleguide.md §12 exclusions."
  exit 1
fi
echo -e "\033[32mAll 7 design-token guards passed.\033[0m"
