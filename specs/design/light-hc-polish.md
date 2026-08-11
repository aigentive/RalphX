# Light + HC polish — 2026-04-19

Dark baseline is the reference. This document catalogs concrete drift on
Light and under-polished areas on HC, with specific remediation. Builds on
`specs/design/page-by-page-review.md` — does **not** re-flag already-fixed
items (panel bleed, Select trigger descriptions, Reviews count badge neutral
chip, HC icon-tile accent-on-accent, placeholder contrast in HC).

Source: 24 screenshots at `.artifacts/theme-switch-audit/{dark,light,high-contrast}/*.png`.

## Summary

| Theme | Overall state | Critical | High | Medium |
|---|---|---|---|---|
| Light | Close to ship — core token semantics correct; elevation cues too faint in a few places and one border token choice violates Light readability | 1 | 4 | 4 |
| HC    | Most polished rules are landed (icon-tile override, placeholder bump, dialog black+white). Main gaps are that a handful of floating/docked surfaces bypass the "2px solid white border" contract and a few interactive-state HC rules (nav indicator, count badge, segmented tab active-yellow) aren't honored | 2 | 4 | 3 |

## Light glitches

### CRITICAL

1. **Graph floating filter panel is nearly invisible on Light**
   - What: The "Status / Vertical / Standard / Plan + Tier" floating island on the Graph view blends into the main canvas on Light because its border token is `--overlay-weak` (a 5% translucent black on Light, reading as ~#EFEFF0 hairline against a #FBFBFC canvas) and the retuned `--shadow-xs` equivalent is barely perceptible. On Dark the same `--overlay-weak` renders as white/10 on black — visible. Result: the panel reads as uncontained; Status and Vertical rows appear as floating options with no container.
   - Where: `src/components/TaskGraph/controls/FloatingGraphFilters.tsx:114-124` — `GLASS_STYLE` uses `border: "1px solid var(--overlay-weak)"` and `boxShadow: "0 4px 16px var(--overlay-scrim), 0 12px 32px var(--overlay-scrim)"`.
   - Fix: Replace `border: 1px solid var(--overlay-weak)` with `border: 1px solid var(--border-subtle)` (a real semantic border token that has theme-tuned values) and lift the boxShadow to `var(--shadow-md)` so the floating island has unambiguous separation on near-white. Removing the hardcoded `color-mix` and opaque overlay expressions also flips HC correctly (see HC #1 below).

### HIGH

2. **Workflow card feels glued to page on Light**
   - What: In Extensibility, the single workflow card has only a 1px `--border-subtle` edge — `--shadow-xs` on Light sits at `0.07 / 0.04` alpha (already bumped from 0.03 per the prior audit) but is still imperceptible. The page reads as flat; "card" vs "background" isn't obvious.
   - Where: `src/styles/themes/light.css:84` — `--shadow-xs: 0 1px 2px hsla(220 10% 20% / 0.07), 0 1px 3px hsla(220 10% 20% / 0.04)`.
   - Fix: Bump `--shadow-xs` to `0 1px 2px hsla(220 10% 20% / 0.09), 0 2px 4px hsla(220 10% 20% / 0.05)`. Stays well under the 0.10 ceiling from `themes/light.md §5` but gives card affordance enough lift to read as elevated on white. Also helps Graph filter panel, Reviews hover card, and Kanban chat send-button chip.

3. **Plans sidebar icon tile is invisible on Light (Ideation)**
   - What: The "Plans / 0 plans" tile at the top of the Ideation sidebar uses `--accent-muted` bg (hsla(14 100% 60% / 0.12) — a very faint peach at ~#FEF0EA on Light) and renders with no discernible separation from the bg. The sparkle icon inside is visible but the tile outline disappears.
   - Where: PlanBrowser header tile (`src/components/Ideation/PlanBrowser.tsx` — likely around the icon-box `div` at ~line 290-310).
   - Fix: Add a 1px `--accent-border` (hsla(14 100% 60% / 0.30)) border to the icon tile so the container outline is visible on white. Same change cascades to HC (yellow-30%-alpha outline).

4. **Reviews card uses hardcoded `border-white/10` on hover (token-discipline + Light invisibility)**
   - What: On hover the review card gains `border-white/10` (via tailwind utility). On Dark this renders as a faint white edge — fine. On Light it adds a translucent white border on a white card — invisible. Hover affordance is lost.
   - Where: `src/components/reviews/ReviewsPanel.tsx:91` — `isHovered && "border-white/10"`.
   - Fix: Replace with `isHovered && "border-[var(--border-default)]"` so hover emphasis flips themes correctly.

5. **Chat panel lacks left-edge shadow on Light (reads as merged with bg)**
   - What: The Chat dock panel sits on a near-white main canvas. `--shadow-md` on Light is `0 2px 4px hsla(220 10% 20% / 0.04), 0 6px 12px hsla(220 10% 20% / 0.04)` — very subtle. The panel's left shadow that should detach it from the workflow area is essentially absent; the panel reads as being the same surface as the Extensibility canvas, separated only by the 1px hairline.
   - Where: `src/styles/themes/light.css:86` — `--shadow-md`.
   - Fix: Bump to `0 2px 4px hsla(220 10% 20% / 0.05), 0 8px 16px hsla(220 10% 20% / 0.06)`. Gives right-dock panels a proper detached affordance while staying under the 0.10 ceiling.

### MEDIUM

6. **Activity Live/History inactive pill invisible on Light**
   - What: On the Activity view the "Live / History" segmented toggle shows the active "History" pill with a filled bg — but the inactive "Live" segment has no visible container. On Dark both have subtle tinted containers so the group reads as a control; on Light only "History" reads, "Live" looks like orphan text.
   - Where: Activity `ActivityFilters.tsx` ViewModeToggle.
   - Fix: Add `border: 1px solid var(--border-subtle)` around the full toggle group on Light (theme-agnostic would be fine too — a 1px subtle border always frames the two-segment group; in Dark the current tonal shift is enough but the outline doesn't hurt).

7. **Theme dropdown descriptions truncated in Settings Select (Light variant confirmation)**
   - What: Prior audit flagged this and marked it fixed. In the current Light screenshot the trigger now shows "Light" as a clean single label — confirmed fixed. No action needed; listed here only to close the loop.
   - Where: `AccessibilitySection.tsx`.
   - Fix: None — confirmed resolved.

8. **Close X on Settings dialog fades into rail on Light**
   - What: Top-right dialog close `X` button on Light uses `--text-secondary` (`hsl(220 10% 35%)` → ~#4F555F) on a near-white dialog header. Technically meets 7.8:1 but visually reads as quite faint because the icon is only 14-16px and weight 2px stroke.
   - Where: `SettingsDialog.tsx` header close button.
   - Fix: Give the close button a hover `--bg-hover` pill so on hover it gains clear affordance, and consider stroking the icon slightly heavier (2.5px) on Light only via a class-scoped `[data-theme="light"]` override. Lowest-effort alternative: leave color as-is but ensure `hover:bg-[var(--bg-hover)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]` is wired.

## High-Contrast improvements

### CRITICAL

1. **Graph floating filter panel has no 2px solid white border in HC**
   - What: The floating filter island on Graph uses `border: 1px solid var(--overlay-weak)` (a 10% white-on-black line, ~0.5 effective px) and a `--shadow-md`-style soft stack. Per HC spec (`themes/high-contrast.md §4 "Cards/notices"`) every elevated surface must carry **2px solid white** — "Dialog: #000000 bg, 2px solid #FFFFFF border". The island is visible only because of the internal separator line; the actual container edge is a ghost hairline.
   - Where: `src/components/TaskGraph/controls/FloatingGraphFilters.tsx:114-124` — `GLASS_STYLE` uses `border: 1px solid var(--overlay-weak)`.
   - Fix (same token swap as Light #1): Replace `border: 1px solid var(--overlay-weak)` with `border: 1px solid var(--border-subtle)`. Because HC overrides `--border-width-default: 2px` and `--border-subtle: rgba(255,255,255,0.5)`, the expression will auto-resolve to a 2px-rgba(255,255,255,0.5) line in HC and a 1px `--gray-300` line in Light. If a fully solid white edge is wanted in HC (matching spec language), use `border: var(--border-width-default) solid var(--border-default)` which HC resolves to `2px solid #FFFFFF`.

### HIGH

2. **Reviews segmented tabs — active state is not yellow in HC**
   - What: HC spec (`themes/high-contrast.md §4 "Tabs"`) mandates `Trigger active: #FFDD00 bg, #000000 text, +2px border bottom`. In the current Reviews screenshot the active "All (0)" tab appears to have a faint gray fill rather than a yellow pill; "AI (0)" and "Human (0)" are unmarked. Users can't tell which filter is active by color alone.
   - Where: `ReviewsPanel.utils.tsx` FilterTabs component (or wherever `data-state=active` styling for the three tabs is set).
   - Fix: Add an HC-scoped override. In `src/styles/themes/high-contrast.css` add `[data-theme="high-contrast"] [data-state="active"].segmented-tab { background: var(--accent-primary); color: var(--text-inverse); border-bottom: var(--border-width-default) solid var(--border-default); }` — OR scope via a class on the FilterTabs component. Also applies to the Activity Live/History toggle (Light #6) for HC.

3. **Nav item active in Settings HC lacks 4px black left indicator**
   - What: HC spec §4 Nav explicitly says `Item active: #FFDD00 bg, #000000 text, font-weight 600 + left border: 4px solid #000000 (draws over the yellow, adds shape cue)`. The current "Accessibility" active rail item shows yellow bg + black text + bold — but no 4px black left bar. Shape cue missing.
   - Where: Left rail nav item in `SettingsDialog.tsx` / shared nav component.
   - Fix: Add `[data-theme="high-contrast"] [aria-current="page"] { border-left: 4px solid var(--color-black); padding-left: calc(var(--padding-inline) - 4px); }` to `high-contrast.css`. (The 4px overlay on yellow nav pills is the documented shape reinforcement.)

4. **Reviews count badge "(0)" still peach/orange-muted in HC**
   - What: The prior audit noted the badge switches to neutral on zero — confirmed working on Dark/Light. In HC screenshot the badge renders with a peach-tinted bg (accent-muted at 15% yellow) inside the Reviews panel header. HC should use `#000` bg + 2px white border + white text for a neutral count, or yellow bg + black text if showing active count. Currently it reads as an ambiguous tinted chip.
   - Where: `ReviewsPanel.tsx` header count-badge component.
   - Fix: Replace the accent-muted fallback with explicit HC styling: `[data-theme="high-contrast"] .count-badge-neutral { background: transparent; border: var(--border-width-default) solid var(--border-default); color: var(--text-primary); }` — count of 0 renders as a white-outlined neutral chip, non-zero uses `bg-[var(--accent-primary)] text-[var(--text-inverse)]`.

5. **Ideation ghost-link actions ("Seed from Draft Task" / "Import Session") have no border affordance in HC**
   - What: Under the primary "Start New Session" CTA, the two ghost-link rows render as plain white text on black with no border. HC spec §6 "What to avoid" says "Placeholder-as-label / Soft drop shadows for elevation / Icon-only status indicators" — effectively an affordance must be visible. The existing `.bg-secondary` HC override at `high-contrast.css:202` applies to secondary buttons but not to these ghost-link rows (they appear to be plain `<button>` elements without `bg-secondary`).
   - Where: Agents Plan/Ideation artifact empty-state ghost actions and similar ghost rows elsewhere.
   - Fix: Extend the existing HC ghost-button override. Add `[data-theme="high-contrast"] button.btn-ghost, [data-theme="high-contrast"] a.btn-ghost { border: var(--border-width-default) solid var(--border-default); padding: var(--space-2) var(--space-3); border-radius: var(--radius-md); }` — if the ghost class is different, use whatever selector the components actually attach (e.g., search for any button with `variant="ghost"` on empty-states and class them).

### MEDIUM

6. **Kanban placeholder contrast passes AAA but reads as ghost on Light-ish HC surfaces**
   - What: "Send a message…" in the right project chat panel on HC is the baseline `--text-muted: #B0B0B0` by default. The global placeholder bump at `high-contrast.css:167-171` forces placeholders to `--text-secondary`. Verify the `textarea` placeholder in ChatInput actually picks up this rule — if the component uses a custom placeholder color (via `color-mix` or a component-scoped class), the global rule may be bypassed.
   - Where: `ChatInput.tsx` placeholder color — confirm it resolves to `--text-secondary` on HC.
   - Fix: Audit `ChatInput.tsx` to ensure no component-level placeholder color override beats the theme rule. If any, remove.

7. **"+ New Workflow" in Extensibility HC has proper 2px border — good precedent**
   - What: In the Extensibility HC screenshot the "+ New Workflow" button has a clean 2px white border. Confirmed working (likely via the existing `bg-secondary` HC override at `high-contrast.css:202-204`). This is the pattern to mirror for the Ideation ghost-links (HC #6) and any other ghost CTA.
   - Where: N/A — reference, no action.
   - Fix: None — use as pattern reference for other ghost buttons.

8. **Chat panel header icons (history / dock-side / close) read very faint in HC**
   - What: The three icon-buttons in the Chat panel header use `--text-secondary` (#E0E0E0 on HC) but visually appear faded. AAA-passing (16:1) but reads as low emphasis against the solid white 2px border frame, which draws the eye first.
   - Where: `ChatPanel.tsx` header icon-button styling.
   - Fix: Give the header icons `color: var(--text-primary)` (`#FFFFFF`) on HC explicitly, or bump the icon stroke weight to 2px on HC (lucide default is 2; verify no override to 1.5). Matches `themes/high-contrast.md §9 "Open decisions"` note about considering 2.5px stroke — relevant if this reads too weak.

## Cross-theme patterns

Issues worth fixing once in both themes:

1. **`--overlay-weak` is overloaded as a border token** — `FloatingGraphFilters.GLASS_STYLE` uses `border: 1px solid var(--overlay-weak)`. Overlays and borders are different role families; `--overlay-weak` is for tinting surfaces, `--border-subtle` / `--border-default` are for edges. The single swap fixes both Light (CRIT #1) and HC (CRIT #1). Audit other components for the same pattern: grep `border.*var\(--overlay-` in `src/components/`.

2. **Hardcoded tailwind alphas on hover borders** — `border-white/10` in `ReviewsPanel.tsx:91` is a utility-style alpha that only reads on Dark. Audit `grep -rn "border-white/\|border-black/" src/components/` for similar violations; each one needs a semantic-token replacement that flips themes.

3. **`--shadow-xs` on Light is still sub-threshold for flat cards** — Despite the prior bump from 0.03 to 0.07/0.04, cards still read as flush. Single-token bump to 0.09/0.05 (Light #2) carries across Extensibility, Reviews, and Graph-filter island without introducing "sticker halo" artifacts. Verify against `themes/light.md §5` ceiling (0.10 max).

4. **Ghost CTAs lack a universal HC border rule** — HC's button-secondary rule at `high-contrast.css:202-204` catches `button.bg-secondary` but not `button.btn-ghost` or `a.btn-ghost`. Ideation empty-state links slip through. A single HC rule covering `button[class*="ghost"]:not([class*="border-"])` would catch the class.

5. **Segmented tab active state has no HC-specific override anywhere** — Reviews tabs, Activity Live/History, and any future segmented control all need the HC spec's yellow-bg + black-text + 2px-bottom pattern. Centralize via `[data-state="active"]` rule in `high-contrast.css`, or mandate a shared `SegmentedControl` component that owns the active styling.

## Priority action list (top 10 ranked by visual impact × effort)

| # | Theme(s) | View | What | Where | Effort |
|---|---|---|---|---|---|
| 1 | Light + HC | Graph | Swap `GLASS_STYLE` border from `--overlay-weak` to `--border-subtle` (+ lift shadow to `--shadow-md`) — fixes both themes with one diff | `FloatingGraphFilters.tsx:114-124` | 5 min |
| 2 | Light | Extensibility / Reviews / cards globally | Bump `--shadow-xs` Light alphas from `0.07 / 0.04` to `0.09 / 0.05` | `light.css:84` | 1 min |
| 3 | HC | Reviews / Activity | Add segmented-tab active-state HC override (yellow bg + black text + 2px bottom) | `high-contrast.css` (new rule) | 10 min |
| 4 | HC | Settings left rail | Add 4px black left indicator on `[aria-current="page"]` for nav active items | `high-contrast.css` (new rule) | 5 min |
| 5 | Light + HC | Reviews card hover | Replace `border-white/10` with `border-[var(--border-default)]` | `ReviewsPanel.tsx:91` | 2 min |
| 6 | HC | Ideation empty-state ghost links | Extend `high-contrast.css:202` ghost-button border rule to cover `button.btn-ghost` / variants without bg-secondary | `high-contrast.css` (edit rule) | 10 min |
| 7 | Light | Chat dock panel | Bump `--shadow-md` Light alphas (`0.04/0.04` → `0.05/0.06`) to restore left-edge lift against near-white | `light.css:86` | 1 min |
| 8 | HC | Reviews count badge | Force neutral-zero badge styling in HC to transparent bg + 2px white border + white text | `high-contrast.css` (new rule) | 5 min |
| 9 | Light | Ideation Plans sidebar tile | Add 1px `--accent-border` outline to tile so container is visible on white | `PlanBrowser.tsx` icon tile | 10 min |

---

### Ship-readiness assessment

- **Light**: one more pass. Items 1, 2, 5, 7 are token or one-line component edits and land most of the elevation + hover-affordance gaps. After that Light is ship-ready.
- **HC**: one more pass. Items 1, 2, and 3 close the remaining HC spec violations (floating-surface borders, yellow-bg active tabs, and the 4px black nav indicator). Once landed HC matches spec.

Both are close — the open items are narrow, token-level or single-selector CSS changes. No structural rework required.
