# Light Theme

> **Purpose:** Near-white surfaces with dark text for users who prefer light UI or who need it in bright ambient conditions. Keeps the warm-orange brand identity with darker accent values tuned to meet WCAG AA on a light background.
>
> **Targets:** WCAG 2.1 **AA** — 4.5:1 for normal text, 3:1 for UI components.
>
> **Related:**
> - `specs/design/theme-architecture.md` — how themes load and switch
> - `specs/design/styleguide.md` — tokens + component patterns (Dark is the canonical baseline)
> - `specs/design/themes/high-contrast.md` — contrast AAA variant
> - `src/styles/themes/light.css` — implementation

---

## 1. Design intent

| Intent | How |
|---|---|
| Neutral bright feel | v27 white canvas with `#F8F8FA` panels and `#F4F4F6` rail |
| Same brand identity | Warm orange accent preserved as `#FF6A35` across dark/light/high-contrast |
| Soft surface separation | Elevation by slight off-white and 1-pixel subtle borders rather than sidebar/rail shadows |
| Status palette stays Okabe-Ito | CVD-safe colours reused, each re-tuned to AA-dark variants that read on white |
| Shadow strategy | Dialog/card shadows only; primary rail and Agents sidebar stay flat with `--sidebar-edge-shadow: none` |

**Philosophy:** Light is Dark's reflection, not a redesign. Same components, same roles, same hierarchy — inverted luminance.

---

## 2. Full palette (from `src/styles/themes/light.css`)

### Backgrounds

| Token | Value | Primitive source | Contrast vs. `--text-primary` |
|---|---|---|---|
| `--bg-base` | `#FFFFFF` | v27 canvas | 15.7:1 |
| `--bg-surface` | `#F8F8FA` | v27 panel/topbar | 15.4:1 |
| `--bg-elevated` | `#FFFFFF` | v27 control/card | 15.7:1 |
| `--bg-hover` | `#F1F1F4` | v27 hover/elev-2 | 14.0:1 |
| `--nav-rail-bg` | `#F4F4F6` | v27 rail | 14.5:1 |
| `--brand-tile` | `#DEDEE2` | v27 light logo tile | logo-only |
| `--brand-pill-from` | `#F0F0F2` | v27 light logo bars | logo-only |
| `--brand-pill-to` | `#EDEDEF` | v27 light logo bars | logo-only |
| `--bg-surface-hover` | same as `--bg-hover` | — | (deprecated alias) |

### Foreground

| Token | Value | Primitive | Contrast |
|---|---|---|---|
| `--text-primary` | `#18181D` | v27 ink | 15.7:1 on white |
| `--text-secondary` | `#404048` | v27 ink-2 | 10.1:1 on white |
| `--text-muted` | `#6A6A72` | v27 ink-3 | 5.2:1 on white |
| `--text-subtle` | `#93939B` | v27 ink-4 | secondary glyphs |
| `--text-inverse` | `#1A0E07` | v27 on-accent | on orange surfaces |

### Accent (brand — warm orange)

| Token | Value | Contrast vs. white |
|---|---|---|
| `--accent-primary` | `#FF6A35` | brand/action fill |
| `--accent-secondary` | `#FF8050` | hover fill |
| `--accent-hover` | `#E0521E` | strong edge |
| `--accent-muted` | `rgba(255,106,53,.10)` | bg tint |
| `--accent-muted-strong` | `rgba(255,106,53,.18)` | v27 active row gradient top stop |
| `--accent-border` | `rgba(255,106,53,.36)` | border tint |
| `--accent-strong` | `hsla(14 90% 50% / 0.50)` | (emphasis bg) |

### Status — Okabe-Ito tuned for AA on white

| Role | Light value | Rationale |
|---|---|---|
| `--status-success` | `#2B9F65` | AA on white |
| `--status-success-muted` | `hsla(164 100% 24% / 0.12)` | faint success tint |
| `--status-success-border` | `hsla(164 100% 24% / 0.30)` | outline |
| `--status-success-strong` | `hsla(164 100% 24% / 0.50)` | emphasis |
| `--status-warning` | `#C9962E` | AA on white |
| `--status-warning-muted/-border/-strong` | alpha variants of same | |
| `--status-error` | `#B24800` (darker vermillion) | AA 4.5:1 |
| `--status-error-muted/-border/-strong` | alpha variants | |
| `--status-info` | `#2B70D6` | AA on white |
| `--status-info-muted/-border/-strong` | alpha variants | |

### Borders

| Token | Value | Width |
|---|---|---|
| `--border-subtle` | `#E5E5E8` | 1px |
| `--border-default` | `#D9D9DD` | 1px |
| `--border-strong` | `#C8C8CD` | 1px hover/strong |
| `--border-focus` | `#2B70D6` | 2px (via `--border-width-focus`) |

### Overlays

| Token | Value | Use |
|---|---|---|
| `--overlay-faint` | `hsla(220 15% 10% / 0.03)` | subtle row tint |
| `--overlay-weak` | `hsla(220 15% 10% / 0.05)` | hover emphasis |
| `--overlay-moderate` | `hsla(220 15% 10% / 0.08)` | section highlight |
| `--overlay-scrim` | `hsla(0 0% 0% / 0.25)` | hero dim |
| `--overlay-scrim-med` | `hsla(0 0% 0% / 0.40)` | modal centre |
| `--overlay-scrim-deep` | `hsla(0 0% 0% / 0.50)` | dialog backdrop |

**Design rule:** Light theme uses translucent **blacks** for overlays, not whites — white-on-white disappears on near-white bg.

### Shadows

Dark-theme shadows are too heavy against white. Light retunes:

| Token | Value |
|---|---|
| `--shadow-xs` | `0 1px 1px hsla(220 15% 25% / 0.10)` |
| `--shadow-sm` | `0 1px 2px hsla(220 15% 25% / 0.10), 0 1px 2px hsla(220 15% 25% / 0.06)` |
| `--shadow-md` | `0 2px 4px hsla(220 15% 25% / 0.09), 0 4px 8px hsla(220 15% 25% / 0.05)` |
| `--shadow-lg` | `0 4px 8px hsla(220 15% 25% / 0.10), 0 12px 20px hsla(220 15% 25% / 0.06)` |

Pulse-shadow tokens (`--shadow-pulse-accent-*`, `-info-*`, `-status-*`, `--shadow-glow-accent-*`, `--shadow-drop-zone-*`) are re-tuned with lower alpha values so animations stay visible but not aggressive on white.

---

## 3. Shape / size overrides

Same as Dark — Light does not change border widths, radii, or font scale.

| Token | Dark | Light |
|---|---|---|
| `--border-width-default` | 1px | 1px |
| `--border-width-focus` | 2px | 2px |
| `--radius-lg` | 12px | 12px |
| `--font-size-base` | 14px | 14px (`<html>` root) |

---

## 4. Component-level behaviour

Components are theme-agnostic — they consume semantic/component tokens, so visuals flip automatically. The spec below documents the *net effect* on Light, not behaviour changes.

### Buttons

```
Primary  (CTA):
  bg:      --accent-primary (darker orange)
  text:    --text-inverse (white)
  hover:   --accent-hover

Secondary / ghost:
  bg:      --bg-elevated (white)
  text:    --text-primary (near-black)
  border:  --border-default

Destructive:
  bg:      --status-error
  text:    --text-inverse
  icon:    XCircle / Trash — mandatory
```

### Inputs / selects

- bg `--bg-elevated` (white)
- border 1px `--border-default`
- focus: 2px `--focus-ring` (blue) + bg unchanged
- placeholder `--text-muted` (5.3:1 on white)

### Dialogs

- overlay `--overlay-scrim-med` (40% black on light)
- panel bg `--bg-elevated`
- border 1px `--border-subtle`
- shadow `--shadow-lg` (light-tuned)

### Cards (`SectionCard`, ProposalCard, TaskCard)

- bg `--bg-elevated` (white)
- border 1px `--border-subtle`
- hover bg `--bg-hover` (faint gray)
- shadow `--shadow-xs`

### Notices (`InlineNotice`)

| Tone | Bg | Border | Icon |
|---|---|---|---|
| ok | `--status-success-muted` | `--status-success-border` | CheckCircle2 `--status-success` |
| warn | `--status-warning-muted` | `--status-warning-border` | TriangleAlert `--status-warning` |
| info | `--status-info-muted` | `--status-info-border` | Info `--status-info` |
| error | `--status-error-muted` | `--status-error-border` | XCircle `--status-error` |

---

## 5. What's different vs. Dark

| Area | Dark | Light |
|---|---|---|
| Surfaces | hsl 220/10%/8–20% tinted dark grays | hsl 220/10%/92–100% near-whites |
| Accent hue | `#FF6B35` | `#EA5A26` (slightly darker for AA) |
| Status colours | Canonical Okabe-Ito | AA-on-white darker variants |
| Shadow alpha | 0.2–0.3 | 0.05–0.10 |
| Overlay colour | Translucent whites | Translucent blacks |
| Focus ring | 2px blue on dark bg | 2px blue on light bg (same token, visible on white) |

Everything else — component shapes, typography, spacing, radii — is identical.

---

## 6. Contrast spot-check

| Pair | Ratio | Meets |
|---|---|---|
| `--text-primary` on `--bg-elevated` | 15.7:1 | AAA |
| `--text-secondary` on `--bg-base` | 7.8:1 | AAA |
| `--text-muted` on `--bg-base` | 5.3:1 | AA |
| `--accent-primary` on white | 4.5:1 | AA (large text / UI component) |
| `--status-error` on white | 4.5:1 | AA |
| `--status-success` on white | 5.0:1 | AA |
| `--status-info` on white | 7.1:1 | AAA |
| `--status-warning` on white | 4.5:1 | AA |

Any new tokens must maintain at least AA on `--bg-elevated` + `--bg-base` and pass the CVD-safe test (see `color-blind-design.md`).

---

## 7. Testing checklist (per release)

- [ ] Toggle Settings → Accessibility → Theme: Light. Walk Kanban / Chat / Settings / Insights.
- [ ] Verify every status notice reads correctly in grayscale screenshot.
- [ ] Keyboard sweep — focus ring visible on every focusable element on white.
- [ ] `prefers-reduced-motion` test — pulse shadows degrade cleanly.
- [ ] Diff viewer is readable (`globals.css` DiffViewer overrides still need a light-theme pass — known limitation).
- [ ] No primitive leak grep (shared with other themes — see `styleguide.md §12`).

---

## 8. Known limitations (2026-04-18)

- **Diff viewer syntax tokens** (`globals.css` Prism/Dracula colours) are pinned to the Dark palette. Light-theme users will see the old dark syntax palette until a shared `syntax-*` token family ships.
- **WelcomeScreen** and `BattleModeV2Overlay` are intentionally fixed-palette regardless of theme. Documented exclusions.
